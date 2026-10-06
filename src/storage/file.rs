//! 文件存储：key → `{目录}/{安全化 key}.json`。
//!
//! 并发约定（同 PHP 版 `FileStorage`）：
//! - 写：同目录临时文件 + `rename()` 原子替换，读方只会看到完整旧内容或完整新内容；
//! - `increment()` 是读改写，进程内用互斥锁串行化（见下方 `ponytail` 注）。
//!
//! 文件名安全化：key 里除 `[A-Za-z0-9_-]` 外的字节一律 `%XX` 转义（`%` 自身也转义），
//! 编码是单射的——不同 key 绝不会撞到同一个文件，`../`、绝对路径、分隔符、空字节
//! 全部落进转义字符里，路径穿越不成立；编码后长度上限 200 字节。

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::error::{PosterError, Result};

use super::{Record, StoredEntry, Storage, b64_decode, b64_encode, expiry_of, now_millis};

/// 编码后文件名的长度上限（含 `%XX` 展开后的字符数）。
const MAX_NAME_LEN: usize = 200;

/// 临时文件序号（同一进程内保证唯一）。
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// 文件存储。
#[derive(Debug)]
pub struct FileStorage {
    dir: PathBuf,
    /// 读改写（`increment`）的进程内串行锁；`Arc<FileStorage>` 被多线程共享时即生效。
    ///
    /// ponytail: 只覆盖进程内并发，多进程共享同一目录仍可能丢计数（PHP 版用 flock 跨进程）。
    /// 多进程 / 多机部署请用 `RedisStorage`（INCR 原子）。
    lock: Mutex<()>,
}

impl FileStorage {
    /// 打开（必要时创建）存储目录。
    pub fn new(path: impl Into<PathBuf>) -> Result<Self> {
        let dir = path.into();
        if dir.exists() && !dir.is_dir() {
            return Err(PosterError::Storage(format!(
                "路径不是目录: {}",
                dir.display()
            )));
        }
        create_dir(&dir)?;
        Ok(Self {
            dir,
            lock: Mutex::new(()),
        })
    }

    /// 按配置打开：目录取 `captcha.file_path`，未配置时用系统临时目录下的 `poster-captcha`。
    pub fn from_config(config: &crate::config::PosterConfig) -> Result<Self> {
        Self::new(default_dir(config))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ()> {
        self.lock.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// key → 文件路径（安全化后的名字）。
    fn file_path(&self, key: &str) -> Result<PathBuf> {
        Ok(self.dir.join(format!("{}.json", encode_name(key)?)))
    }

    fn read_record(&self, path: &Path) -> Option<Record> {
        let bytes = std::fs::read(path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// 临时文件 + `rename()` 原子替换。
    fn write_record(&self, path: &Path, record: &Record) -> Result<()> {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("record.json");
        let tmp = self.dir.join(format!(
            "{name}.{}.{}.tmp",
            std::process::id(),
            TMP_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let bytes = serde_json::to_vec(record)?;
        write_private(&tmp, &bytes)?;
        if let Err(e) = std::fs::rename(&tmp, path) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.into());
        }
        Ok(())
    }
}

impl Storage for FileStorage {
    fn get(&self, key: &str) -> Result<Option<StoredEntry>> {
        let path = self.file_path(key)?;
        let Some(record) = self.read_record(&path) else {
            return Ok(None);
        };
        let now = now_millis();
        if record.e <= now {
            let _ = std::fs::remove_file(&path);
            return Ok(None);
        }
        let Some(value) = b64_decode(&record.v) else {
            let _ = std::fs::remove_file(&path);
            return Ok(None);
        };
        Ok(Some(StoredEntry {
            value,
            ttl: Some(Duration::from_millis(record.e - now)),
        }))
    }

    fn set(&self, key: &str, value: &[u8], ttl: Duration) -> Result<()> {
        let path = self.file_path(key)?;
        let record = Record {
            v: b64_encode(value),
            a: 0, // set 重置计数（同 PHP）
            e: expiry_of(ttl),
        };
        self.write_record(&path, &record)
    }

    fn delete(&self, key: &str) -> Result<()> {
        match std::fs::remove_file(self.file_path(key)?) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    fn increment(&self, key: &str) -> Result<Option<u64>> {
        let _guard = self.lock();
        let path = self.file_path(key)?;
        let Some(mut record) = self.read_record(&path) else {
            return Ok(None);
        };
        if record.e <= now_millis() {
            let _ = std::fs::remove_file(&path);
            return Ok(None);
        }
        record.a += 1;
        let attempts = record.a;
        // 写失败不能把「没记上数」当成一次有效尝试，调用方据 None 失败关闭
        match self.write_record(&path, &record) {
            Ok(()) => Ok(Some(attempts)),
            Err(_) => Ok(None),
        }
    }
}

/// 默认存储目录：配置优先，否则系统临时目录下的 `poster-captcha`。
pub fn default_dir(config: &crate::config::PosterConfig) -> PathBuf {
    config
        .captcha
        .file_path
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join("poster-captcha"))
}

/// 目录名安全化：只保留 `[A-Za-z0-9_-]`，其余字节 `%XX` 转义（单射，无路径穿越）。
fn encode_name(key: &str) -> Result<String> {
    let mut out = String::with_capacity(key.len());
    for byte in key.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    if out.is_empty() {
        return Err(PosterError::Storage("存储 key 不能为空".into()));
    }
    if out.len() > MAX_NAME_LEN {
        return Err(PosterError::Storage(format!(
            "存储 key 过长（编码后 {} > {MAX_NAME_LEN} 字节）",
            out.len()
        )));
    }
    Ok(out)
}

/// 建目录（Unix 下 0700：验证码答案不宜被同机其他用户读到）。
fn create_dir(dir: &Path) -> Result<()> {
    if dir.is_dir() {
        return Ok(());
    }
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(dir)
        .map_err(|e| PosterError::Storage(format!("无法创建目录 {}: {e}", dir.display())))
}

/// 写文件（Unix 下 0600）。
fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(path)?;
    file.write_all(bytes)?;
    file.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "poster-captcha-test-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    #[test]
    fn name_encoding_blocks_traversal_and_is_injective() {
        assert_eq!(encode_name("abc-123_XY").unwrap(), "abc-123_XY");
        assert_eq!(encode_name("../../etc/passwd").unwrap(), "%2E%2E%2F%2E%2E%2Fetc%2Fpasswd");
        assert_eq!(encode_name("rate:abc").unwrap(), "rate%3Aabc");
        assert_ne!(encode_name("a/b").unwrap(), encode_name("a%2Fb").unwrap());
        assert_ne!(encode_name("a%2F").unwrap(), encode_name("a/2F").unwrap());
        assert!(encode_name("").is_err());
        assert!(encode_name(&"x".repeat(MAX_NAME_LEN + 1)).is_err());
    }

    #[test]
    fn traversal_key_stays_inside_dir() {
        let dir = temp_dir("traversal");
        let s = FileStorage::new(&dir).unwrap();
        s.set("../../evil", b"v", Duration::from_secs(30)).unwrap();
        let files: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).collect();
        assert_eq!(files.len(), 1);
        assert!(files[0].path().starts_with(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn round_trip_and_expiry() {
        let dir = temp_dir("ttl");
        let s = FileStorage::new(&dir).unwrap();
        s.set("k", b"\x00\x01binary", Duration::from_secs(60)).unwrap();
        assert_eq!(s.get("k").unwrap().unwrap().value, b"\x00\x01binary");
        s.set("short", b"v", Duration::from_millis(30)).unwrap();
        std::thread::sleep(Duration::from_millis(60));
        assert!(s.get("short").unwrap().is_none());
        assert!(s.increment("short").unwrap().is_none());
        s.delete("k").unwrap();
        assert!(s.get("k").unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn path_that_is_a_file_is_rejected() {
        let dir = temp_dir("notdir");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("file");
        std::fs::write(&file, b"x").unwrap();
        assert!(FileStorage::new(&file).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
