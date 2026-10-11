//! 进程内存储（默认后端）。进程重启即失效，单机多线程安全。

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use crate::error::{PosterError, Result};

use super::{Record, StoredEntry, Storage, b64_decode, b64_encode, expiry_of, now_millis};

/// 值 + 键名的直接占用预算（记录以 base64 存，膨胀后即真实占用）。
const DEFAULT_BUDGET: usize = 64 << 20;

/// 每 N 次 `set` 摊还一次全扫，清掉没人再碰的过期条目（读取路径只清被读到的那个）。
const SWEEP_EVERY: u64 = 256;

/// 进程内存储：`HashMap` + 互斥锁，过期条目读取时惰性清除，写入路径摊还全扫。
///
/// 多进程部署（多个 worker 进程）不共享计数，请改用 `RedisStorage`（feature = `redis`）。
#[derive(Debug)]
pub struct MemoryStorage {
    entries: Mutex<HashMap<String, Record>>,
    /// 所有条目「键名 + 值」的总字节数，随写删惰性清除同步维护（恒在 `entries` 锁内改动）。
    bytes: AtomicUsize,
    /// `set` 次数，驱动摊还扫描。
    sets: AtomicU64,
    /// 写入预算：`bytes + 新值` 超预算即拒绝写入（失败关闭，挡住无界增长）。
    budget: usize,
}

impl Default for MemoryStorage {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            bytes: AtomicUsize::new(0),
            sets: AtomicU64::new(0),
            budget: DEFAULT_BUDGET,
        }
    }
}

impl MemoryStorage {
    /// 新建空存储。
    pub fn new() -> Self {
        Self::default()
    }

    /// 当前条目数（含尚未清理的过期条目），测试与监控用。
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    /// 全扫一遍：清掉已过期条目并重算占用。`set` 每 [`SWEEP_EVERY`] 次自动调用，
    /// 测试则直接调它（生产路径不暴露，免得成为未使用的公共面）。
    #[cfg(test)]
    pub(crate) fn sweep(&self) {
        let mut entries = self.lock();
        sweep_map(&mut entries, &self.bytes, now_millis());
    }

    /// 测试用：指定一个小预算，免得用例真去分配 64 MiB。
    #[cfg(test)]
    pub(crate) fn with_budget(budget: usize) -> Self {
        Self {
            budget,
            ..Self::default()
        }
    }

    /// 加锁；锁中毒（持锁线程 panic）后取回内部数据继续用，避免整个验证码功能瘫在一次 panic 上。
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Record>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 在已持有的锁内删一条：同步占用计数。
    fn remove_locked(&self, entries: &mut HashMap<String, Record>, key: &str) -> bool {
        match entries.remove(key) {
            Some(record) => {
                self.bytes.fetch_sub(record_cost(key, &record), Ordering::Relaxed);
                true
            }
            None => false,
        }
    }
}

/// 单条记录的占用（键名 + 值）。
fn record_cost(key: &str, record: &Record) -> usize {
    key.len() + record.v.len()
}

/// 清过期条目并重算总占用（重算顺带抹平任何计数漂移）。
fn sweep_map(entries: &mut HashMap<String, Record>, bytes: &AtomicUsize, now: u64) {
    entries.retain(|_, record| record.e > now);
    bytes.store(
        entries.iter().map(|(key, record)| record_cost(key, record)).sum(),
        Ordering::Relaxed,
    );
}

/// 写入 `cost` 后是否仍在预算内（覆盖同名键时先退掉旧的量）。
fn fits(
    entries: &HashMap<String, Record>,
    bytes: &AtomicUsize,
    key: &str,
    cost: usize,
    budget: usize,
) -> bool {
    let replaced = entries.get(key).map_or(0, |old| record_cost(key, old));
    // saturating：计数若因 bug 漂移也绝不 panic，宁可多判一次「超预算」
    bytes.load(Ordering::Relaxed).saturating_sub(replaced) + cost <= budget
}

impl Storage for MemoryStorage {
    fn get(&self, key: &str) -> Result<Option<StoredEntry>> {
        let now = now_millis();
        let mut entries = self.lock();
        let Some(record) = entries.get(key) else {
            return Ok(None);
        };
        if record.e <= now {
            self.remove_locked(&mut entries, key);
            return Ok(None);
        }
        let Some(value) = b64_decode(&record.v) else {
            self.remove_locked(&mut entries, key);
            return Ok(None);
        };
        Ok(Some(StoredEntry {
            value,
            ttl: Some(Duration::from_millis(record.e - now)),
        }))
    }

    fn set(&self, key: &str, value: &[u8], ttl: Duration) -> Result<()> {
        let record = Record {
            v: b64_encode(value),
            // set 重置计数（同 PHP）
            a: 0,
            e: expiry_of(ttl),
        };
        let cost = record_cost(key, &record);

        let mut entries = self.lock();
        if self.sets.fetch_add(1, Ordering::Relaxed) % SWEEP_EVERY == 0 {
            sweep_map(&mut entries, &self.bytes, now_millis());
        }
        if !fits(&entries, &self.bytes, key, cost, self.budget) {
            // 可能是过期条目还占着预算：先回收再看，仍不够才失败关闭
            sweep_map(&mut entries, &self.bytes, now_millis());
            if !fits(&entries, &self.bytes, key, cost, self.budget) {
                return Err(PosterError::Storage(format!(
                    "进程内存储预算已满（{} MiB）：拒绝写入，请调大 TTL/清理目录或改用 RedisStorage",
                    self.budget >> 20
                )));
            }
        }
        // 覆盖同名键时先退掉旧记录的量（只在写入成功路径上生效）
        let replaced = entries.get(key).map_or(0, |old| record_cost(key, old));
        if replaced > 0 {
            self.bytes.fetch_sub(replaced, Ordering::Relaxed);
        }
        entries.insert(key.to_string(), record);
        self.bytes.fetch_add(cost, Ordering::Relaxed);
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<bool> {
        let mut entries = self.lock();
        Ok(self.remove_locked(&mut entries, key))
    }

    fn increment(&self, key: &str) -> Result<Option<u64>> {
        let now = now_millis();
        let mut entries = self.lock();
        let Some(record) = entries.get_mut(key) else {
            return Ok(None);
        };
        if record.e <= now {
            self.remove_locked(&mut entries, key);
            return Ok(None);
        }
        record.a += 1;
        Ok(Some(record.a))
    }

    fn increment_or_init(&self, key: &str, ttl: Duration) -> Result<Option<u64>> {
        let now = now_millis();
        let mut entries = self.lock();
        match entries.get_mut(key) {
            // 过期条目等同不存在：原地重置成第 1 次（避免为重建再扫一次表）
            Some(record) if record.e <= now => {
                let cost = record_cost(key, record);
                *record = Record {
                    v: String::new(),
                    a: 1,
                    e: expiry_of(ttl),
                };
                let new_cost = record_cost(key, record);
                self.bytes.fetch_sub(cost, Ordering::Relaxed);
                self.bytes.fetch_add(new_cost, Ordering::Relaxed);
                Ok(Some(1))
            }
            Some(record) => {
                record.a += 1;
                Ok(Some(record.a))
            }
            None => {
                let record = Record {
                    v: String::new(),
                    a: 1,
                    e: expiry_of(ttl),
                };
                let cost = record_cost(key, &record);
                if self.bytes.load(Ordering::Relaxed) + cost > self.budget {
                    return Err(PosterError::Storage(format!(
                        "进程内存储预算已满（{} MiB）：拒绝计数键",
                        self.budget >> 20
                    )));
                }
                entries.insert(key.to_string(), record);
                self.bytes.fetch_add(cost, Ordering::Relaxed);
                Ok(Some(1))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn storage() -> MemoryStorage {
        MemoryStorage::new()
    }

    #[test]
    fn set_get_delete_round_trip() {
        let s = storage();
        assert!(s.get("k").unwrap().is_none());
        s.set("k", b"hello", Duration::from_secs(60)).unwrap();
        assert_eq!(s.get("k").unwrap().unwrap().value, b"hello");
        assert!(s.delete("k").unwrap(), "删到了");
        assert!(s.get("k").unwrap().is_none());
        assert!(!s.delete("k").unwrap(), "重复删除：键本就不存在"); // 不算错误
    }

    #[test]
    fn expired_entries_are_gone() {
        let s = storage();
        s.set("k", b"v", Duration::from_millis(30)).unwrap();
        std::thread::sleep(Duration::from_millis(60));
        assert!(s.get("k").unwrap().is_none());
        assert!(s.increment("k").unwrap().is_none());
    }

    #[test]
    fn set_resets_attempts() {
        let s = storage();
        s.set("k", b"v", Duration::from_secs(60)).unwrap();
        assert_eq!(s.increment("k").unwrap(), Some(1));
        assert_eq!(s.increment("k").unwrap(), Some(2));
        s.set("k", b"v2", Duration::from_secs(60)).unwrap();
        assert_eq!(s.increment("k").unwrap(), Some(1), "set 应重置计数");
    }

    #[test]
    fn increment_missing_key_is_none_not_one() {
        let s = storage();
        assert_eq!(s.increment("missing").unwrap(), None);
    }

    #[test]
    fn concurrent_increments_do_not_lose_counts() {
        use std::sync::Arc;
        let s = Arc::new(storage());
        s.set("k", b"v", Duration::from_secs(60)).unwrap();
        let mut handles = Vec::new();
        for _ in 0..8 {
            let s = Arc::clone(&s);
            handles.push(std::thread::spawn(move || {
                for _ in 0..50 {
                    s.increment("k").unwrap();
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(s.increment("k").unwrap(), Some(401));
    }

    #[test]
    fn increment_or_init_creates_then_counts() {
        let s = storage();
        let ttl = Duration::from_secs(60);
        assert_eq!(s.increment_or_init("w", ttl).unwrap(), Some(1), "键不存在→创建");
        assert_eq!(s.increment_or_init("w", ttl).unwrap(), Some(2), "存在→自增");

        // 过期后重新从 1 起，而不是接着涨
        s.set("short", b"v", Duration::from_millis(30)).unwrap();
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(s.increment_or_init("short", ttl).unwrap(), Some(1));
    }

    #[test]
    fn concurrent_increment_or_init_do_not_lose_counts() {
        use std::sync::Arc;
        let s = Arc::new(storage());
        let ttl = Duration::from_secs(60);
        let mut handles = Vec::new();
        for _ in 0..8 {
            let s = Arc::clone(&s);
            handles.push(std::thread::spawn(move || {
                for _ in 0..50 {
                    s.increment_or_init("w", ttl).unwrap();
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(s.increment_or_init("w", ttl).unwrap(), Some(401));
    }

    #[test]
    fn sweep_reclaims_expired_entries_and_their_budget() {
        // 预算刚好装下 7 条 10 字节上下的记录（键 + base64 后的值）
        let s = MemoryStorage::with_budget(74);
        s.set("live", b"1234", Duration::from_secs(60)).unwrap(); // 键 4 + 值 8
        for i in 0..6 {
            s.set(&format!("k{i}"), b"1234", Duration::from_millis(30)).unwrap();
        }
        assert_eq!(s.len(), 7);
        assert!(
            s.set("extra", b"1234", Duration::from_secs(60)).is_err(),
            "预算已被未过期的条目占满"
        );

        std::thread::sleep(Duration::from_millis(60));
        s.sweep();
        assert_eq!(s.len(), 1, "只有未过期的留下");
        // 预算随清理一起回收（否则这里仍会因超预算失败）
        s.set("extra", b"1234", Duration::from_secs(60)).unwrap();
    }

    #[test]
    fn set_refuses_over_budget_and_keeps_old_value() {
        let s = MemoryStorage::with_budget(64);
        s.set("k", b"1234567890", Duration::from_secs(60)).unwrap(); // 键 1 + 值 10 = 11
        let error = s
            .set("k", &[0u8; 100], Duration::from_secs(60))
            .expect_err("超预算应失败关闭");
        assert!(error.to_string().contains("预算"), "错误信息要说明预算：{error}");
        assert_eq!(
            s.get("k").unwrap().unwrap().value,
            b"1234567890",
            "拒绝写入时旧值必须原样保留"
        );

        // 另一条键也超预算，同样拒绝
        assert!(s.set("other", &[0u8; 100], Duration::from_secs(60)).is_err());
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn reported_ttl_shrinks() {
        let s = storage();
        s.set("k", b"v", Duration::from_millis(200)).unwrap();
        let ttl = s.get("k").unwrap().unwrap().ttl.unwrap();
        assert!(ttl <= Duration::from_millis(200) && ttl > Duration::from_millis(100));
    }
}
