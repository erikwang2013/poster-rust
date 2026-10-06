//! Redis 存储（`redis` feature）：键前缀取 `captcha.redis_prefix`，多机共享、计数原子。
//!
//! 键布局（同 PHP 版 `RedisStorage`）：
//! - `{prefix}{key}` —— 值（二进制安全的原始字节，不再套 JSON/base64）；
//! - `{prefix}{key}:att` —— 尝试计数，`INCR` 原子自增，首次自增时补上过期时间。
//!
//! 与 PHP 的差异：计数键在**主键已不存在**时会被清掉并返回 `None`（失败关闭）。
//! PHP 的 `INCR` 会凭空建键并返回 1，把「键已被并发删除」记成了一次有效尝试；
//! 各后端统一成失败关闭，语义更紧。

use std::sync::Mutex;
use std::time::Duration;

use redis::Commands;

use crate::error::Result;

use super::{StoredEntry, Storage};

/// Redis 存储。
pub struct RedisStorage {
    /// 连接不是 `Sync`，加锁后可随 `Arc<dyn Storage>` 共享。
    conn: Mutex<redis::Connection>,
    prefix: String,
}

impl RedisStorage {
    /// 按 URL 连接（如 `redis://127.0.0.1:6379/`）。
    pub fn new(url: &str, prefix: &str) -> Result<Self> {
        let client = redis::Client::open(url)?;
        let conn = client.get_connection()?;
        Ok(Self {
            conn: Mutex::new(conn),
            prefix: prefix.to_string(),
        })
    }

    /// 按配置连接：前缀取 `captcha.redis_prefix`，地址取环境变量 `REDIS_URL`，
    /// 缺省 `redis://127.0.0.1:6379/`（PHP 侧对应 `captcha.redis.connection`）。
    pub fn from_config(config: &crate::config::PosterConfig) -> Result<Self> {
        let url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379/".into());
        Self::new(&url, &config.captcha.redis_prefix)
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, redis::Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn value_key(&self, key: &str) -> String {
        format!("{}{key}", self.prefix)
    }

    fn counter_key(&self, key: &str) -> String {
        format!("{}{key}:att", self.prefix)
    }
}

impl Storage for RedisStorage {
    fn get(&self, key: &str) -> Result<Option<StoredEntry>> {
        let value: Option<Vec<u8>> = self.conn().get(self.value_key(key))?;
        // TTL 由 Redis 自己维护，`pttl` 是额外一次往返，收益不抵成本，故报 None
        Ok(value.map(|value| StoredEntry { value, ttl: None }))
    }

    fn set(&self, key: &str, value: &[u8], ttl: Duration) -> Result<()> {
        let seconds = ttl_seconds(ttl);
        let mut conn = self.conn();
        conn.set_ex::<_, _, ()>(self.value_key(key), value, seconds)?;
        // 计数单独放一个键：与值同键的读改写不原子，INCR 没有这个问题
        conn.set_ex::<_, _, ()>(self.counter_key(key), 0u64, seconds)?;
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<()> {
        let _: u64 = self
            .conn()
            .del(&[self.value_key(key), self.counter_key(key)][..])?;
        Ok(())
    }

    fn increment(&self, key: &str) -> Result<Option<u64>> {
        let value_key = self.value_key(key);
        let counter_key = self.counter_key(key);
        let mut conn = self.conn();
        let count: u64 = conn.incr(&counter_key, 1u64)?;
        if count == 1 {
            // 主键已不在（并发校验成功 / 过期 / 从没写过）：这次自增不算数，清掉计数键
            let exists: u64 = conn.exists(&value_key)?;
            if exists == 0 {
                let _: u64 = conn.del(&counter_key)?;
                return Ok(None);
            }
            // 计数键是刚建出来的，过期时间跟主键走（同 PHP）
            let ttl: i64 = conn.ttl(&value_key)?;
            if ttl > 0 {
                let _: i64 = conn.expire(&counter_key, ttl)?;
            }
        }
        Ok(Some(count))
    }
}

/// TTL 秒数：截断取整（同 PHP `intval`），但 Redis 的 `SETEX` 不接受 0，故下限 1 秒。
fn ttl_seconds(ttl: Duration) -> u64 {
    ttl.as_secs().max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ttl_rounds_up_to_whole_seconds() {
        assert_eq!(ttl_seconds(Duration::ZERO), 1);
        assert_eq!(ttl_seconds(Duration::from_millis(1)), 1);
        assert_eq!(ttl_seconds(Duration::from_millis(1500)), 1);
        assert_eq!(ttl_seconds(Duration::from_secs(300)), 300);
    }
}
