//! 文件存储：key → `{目录}/{安全化 key}.json`。
//!
//! 并发约定：
//! - 写：同目录临时文件 + `rename()` 原子替换，读方只会看到完整旧内容或完整新内容；
//! - `set` / `increment` / `increment_or_init` 都是读改写，进程内用互斥锁串行化
//!   （见下方 `ponytail` 注）。
//!
//! 回收：`set` 每 [`SWEEP_EVERY`] 次摊还扫一遍目录，清掉过期文件与孤儿 `*.tmp`；
//! 存活文件数达 [`FILE_BUDGET`] 后 `set` 失败关闭（无界增长 DoS 的封顶）。
//!
//! 文件名安全化：key 里除 `[A-Za-z0-9_-]` 外的字节一律 `%XX` 转义（`%` 自身也转义），
//! 编码是单射的——不同 key 绝不会撞到同一个文件，`../`、绝对路径、分隔符、空字节
//! 全部落进转义字符里，路径穿越不成立；编码后长度上限 200 字节。

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Deserialize;

use crate::error::{PosterError, Result};

use super::{Record, Storage, StoredEntry, b64_decode, b64_encode, expiry_of, now_millis};

/// 编码后文件名的长度上限（含 `%XX` 展开后的字符数）。
const MAX_NAME_LEN: usize = 200;

/// 每 N 次 `set` 摊还一次目录扫描。
const SWEEP_EVERY: u64 = 256;

/// 目录里的存活文件数上限，达到后 `set` 一律失败关闭。
const FILE_BUDGET: usize = 65536;

/// 读不出 JSON 的文件无法证明已死，只在明显陈旧（超过这个年龄）时才清。
const STALE_FALLBACK_AGE: Duration = Duration::from_secs(24 * 3600);

/// 临时文件序号（同一进程内保证唯一）。
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// 文件存储。
#[derive(Debug)]
pub struct FileStorage {
    dir: PathBuf,
    /// 读改写（`set` / `increment*`）的进程内串行锁；`Arc<FileStorage>` 被多线程共享时即生效。
    ///
    /// ponytail: 只覆盖进程内并发，多进程共享同一目录仍可能丢计数。
    /// 多进程 / 多机部署请用 `RedisStorage`（INCR 原子）。
    lock: Mutex<()>,
    /// `set` 次数，驱动摊还扫描。
    sets: AtomicU64,
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
            sets: AtomicU64::new(0),
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

    /// 全扫一遍目录：清过期文件与孤儿 `*.tmp`，返回存活文件数。`set` 每 [`SWEEP_EVERY`]
    /// 次自动调用（摊还成本），测试则直接调它（生产路径不暴露，免得成为未使用的公共面）。
    #[cfg(test)]
    pub(crate) fn sweep(&self) -> Result<usize> {
        let _guard = self.lock();
        self.sweep_locked()
    }

    /// [`sweep`](Self::sweep) 的加锁前置版（调用方必须已持锁）。
    fn sweep_locked(&self) -> Result<usize> {
        let now = now_millis();
        let mut live = 0usize;
        for entry in std::fs::read_dir(&self.dir)? {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            // 记录文件名里的 `.` 全被 encode_name 转义，`.tmp` / `.json` 后缀只会是我们拼出来的
            if name.ends_with(".tmp") {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            if !name.ends_with(".json") {
                continue; // 不是我们写的文件，不动
            }
            match read_expiry(&path) {
                Some(e) if e <= now => {
                    let _ = std::fs::remove_file(&path);
                }
                Some(_) => live += 1,
                None => {
                    // 读不出 JSON：证明不了已死，退回 mtime，只在明显陈旧时才清
                    let stale = std::fs::metadata(&path)
                        .and_then(|meta| meta.modified())
                        .ok()
                        .and_then(|modified| modified.elapsed().ok())
                        .is_some_and(|age| age > STALE_FALLBACK_AGE);
                    if stale {
                        let _ = std::fs::remove_file(&path);
                    } else {
                        live += 1;
                    }
                }
            }
        }
        Ok(live)
    }
}

/// 只取到期时刻的轻量探针：serde 会跳过未知字段，不为 PNG / base64 大字段建字符串。
#[derive(Deserialize)]
struct ExpiryProbe {
    e: u64,
}

/// 读一条记录的到期时刻；文件读不出或不是我们的 JSON 时返回 `None`。
fn read_expiry(path: &Path) -> Option<u64> {
    let file = std::fs::File::open(path).ok()?;
    let probe: ExpiryProbe = serde_json::from_reader(std::io::BufReader::new(file)).ok()?;
    Some(probe.e)
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
        // 与 increment 的读改写同锁：否则并发下 increment 可能基于被覆盖的旧记录写回，丢计数
        let _guard = self.lock();
        let path = self.file_path(key)?;
        if self.sets.fetch_add(1, Ordering::Relaxed) % SWEEP_EVERY == 0 {
            let live = self.sweep_locked()?;
            if live >= FILE_BUDGET {
                return Err(PosterError::Storage(format!(
                    "存储目录存活文件数已达上限（{FILE_BUDGET}）：拒绝写入"
                )));
            }
        }
        let record = Record {
            v: b64_encode(value),
            a: 0, // set 重置计数
            e: expiry_of(ttl),
        };
        self.write_record(&path, &record)
    }

    fn delete(&self, key: &str) -> Result<bool> {
        match std::fs::remove_file(self.file_path(key)?) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
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

    fn increment_or_init(&self, key: &str, ttl: Duration) -> Result<Option<u64>> {
        let _guard = self.lock();
        let path = self.file_path(key)?;
        let now = now_millis();
        let mut record = match self.read_record(&path) {
            Some(record) if record.e > now => record,
            // 不存在 / 已过期：从 1 起（set 的重置语义）
            _ => {
                let fresh = Record {
                    v: String::new(),
                    a: 1,
                    e: expiry_of(ttl),
                };
                return match self.write_record(&path, &fresh) {
                    Ok(()) => Ok(Some(1)),
                    // 写失败 = 计数不可用，调用方失败关闭
                    Err(_) => Ok(None),
                };
            }
        };
        record.a += 1;
        let count = record.a;
        match self.write_record(&path, &record) {
            Ok(()) => Ok(Some(count)),
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
        assert_eq!(
            encode_name("../../etc/passwd").unwrap(),
            "%2E%2E%2F%2E%2E%2Fetc%2Fpasswd"
        );
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
        let files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(files.len(), 1);
        assert!(files[0].path().starts_with(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn round_trip_and_expiry() {
        let dir = temp_dir("ttl");
        let s = FileStorage::new(&dir).unwrap();
        s.set("k", b"\x00\x01binary", Duration::from_secs(60))
            .unwrap();
        assert_eq!(s.get("k").unwrap().unwrap().value, b"\x00\x01binary");
        s.set("short", b"v", Duration::from_millis(30)).unwrap();
        std::thread::sleep(Duration::from_millis(60));
        assert!(s.get("short").unwrap().is_none());
        assert!(s.increment("short").unwrap().is_none());
        assert!(s.delete("k").unwrap());
        assert!(!s.delete("k").unwrap(), "键已不在：删不到");
        assert!(s.get("k").unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_holds_the_lock_so_increment_sees_the_fresh_record() {
        let dir = temp_dir("set-lock");
        let s = FileStorage::new(&dir).unwrap();
        // set 重置计数：写入后第一次 increment 必须是 1
        s.set("k", b"v", Duration::from_secs(60)).unwrap();
        s.increment("k").unwrap();
        s.increment("k").unwrap();
        s.set("k", b"v2", Duration::from_secs(60)).unwrap();
        assert_eq!(s.increment("k").unwrap(), Some(1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn increment_or_init_creates_then_counts() {
        let dir = temp_dir("incr-init");
        let s = FileStorage::new(&dir).unwrap();
        let ttl = Duration::from_secs(60);
        assert_eq!(s.increment_or_init("w", ttl).unwrap(), Some(1));
        assert_eq!(s.increment_or_init("w", ttl).unwrap(), Some(2));

        // 过期后重新从 1 起
        let dir2 = temp_dir("incr-init-expired");
        let s2 = FileStorage::new(&dir2).unwrap();
        s2.set("w", b"v", Duration::from_millis(30)).unwrap();
        assert_eq!(
            s2.increment_or_init("w", Duration::from_secs(60)).unwrap(),
            Some(1)
        );
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(
            s2.increment_or_init("w", ttl).unwrap(),
            Some(1),
            "过期即重置"
        );
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
    }

    #[test]
    fn sweep_removes_expired_and_orphan_tmp_files() {
        let dir = temp_dir("sweep");
        let s = FileStorage::new(&dir).unwrap();
        s.set("gone", b"v", Duration::from_millis(30)).unwrap();
        s.set("live", b"v", Duration::from_secs(60)).unwrap();
        // 写一半崩掉留下的孤儿临时文件
        std::fs::write(dir.join("orphan.json.999.1.tmp"), b"{}").unwrap();
        // 损坏的记录（读不出 JSON）：不能证明已死，留着
        std::fs::write(dir.join("broken.json"), b"not json").unwrap();

        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(s.sweep().unwrap(), 2, "live + broken 计入存活");

        assert!(!dir.join("gone.json").exists(), "过期文件应被清掉");
        assert!(dir.join("live.json").exists());
        assert!(
            !dir.join("orphan.json.999.1.tmp").exists(),
            "孤儿 tmp 应被清掉"
        );
        assert!(dir.join("broken.json").exists(), "读不出的文件不轻动");
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
