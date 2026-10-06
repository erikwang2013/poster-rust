//! 进程内存储（默认后端）。进程重启即失效，单机多线程安全。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use crate::error::Result;

use super::{Record, StoredEntry, Storage, b64_decode, b64_encode, expiry_of, now_millis};

/// 进程内存储：`HashMap` + 互斥锁，过期条目读取时惰性清除。
///
/// 多进程部署（多个 worker 进程）不共享计数，请改用 `RedisStorage`（feature = `redis`）。
#[derive(Debug, Default)]
pub struct MemoryStorage {
    entries: Mutex<HashMap<String, Record>>,
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

    /// 加锁；锁中毒（持锁线程 panic）后取回内部数据继续用，避免整个验证码功能瘫在一次 panic 上。
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Record>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Storage for MemoryStorage {
    fn get(&self, key: &str) -> Result<Option<StoredEntry>> {
        let now = now_millis();
        let mut entries = self.lock();
        let Some(record) = entries.get(key) else {
            return Ok(None);
        };
        if record.e <= now {
            entries.remove(key);
            return Ok(None);
        }
        let Some(value) = b64_decode(&record.v) else {
            entries.remove(key);
            return Ok(None);
        };
        Ok(Some(StoredEntry {
            value,
            ttl: Some(Duration::from_millis(record.e - now)),
        }))
    }

    fn set(&self, key: &str, value: &[u8], ttl: Duration) -> Result<()> {
        self.lock().insert(
            key.to_string(),
            Record {
                v: b64_encode(value),
                // set 重置计数（同 PHP）
                a: 0,
                e: expiry_of(ttl),
            },
        );
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<()> {
        self.lock().remove(key);
        Ok(())
    }

    fn increment(&self, key: &str) -> Result<Option<u64>> {
        let now = now_millis();
        let mut entries = self.lock();
        let Some(record) = entries.get_mut(key) else {
            return Ok(None);
        };
        if record.e <= now {
            entries.remove(key);
            return Ok(None);
        }
        record.a += 1;
        Ok(Some(record.a))
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
        s.delete("k").unwrap();
        assert!(s.get("k").unwrap().is_none());
        s.delete("k").unwrap(); // 重复删除不算错误
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
    fn reported_ttl_shrinks() {
        let s = storage();
        s.set("k", b"v", Duration::from_millis(200)).unwrap();
        let ttl = s.get("k").unwrap().unwrap().ttl.unwrap();
        assert!(ttl <= Duration::from_millis(200) && ttl > Duration::from_millis(100));
    }
}
