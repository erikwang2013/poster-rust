//! Redis 存储（`redis` feature）：键前缀取 `captcha.redis_prefix`，多机共享、计数原子。
//!
//! 键布局：
//! - `{prefix}{key}` —— 值（二进制安全的原始字节，不再套 JSON/base64）；
//! - `{prefix}{key}:att` —— 尝试计数，`INCR` 原子自增，首次自增时补上过期时间；
//! - `{prefix}rate:…`（限流计数键，见 [`Storage::increment_or_init`]）：计数即键本身，`INCR` + `EXPIRE`。
//!
//! 计数键在**主键已不存在**时会被清掉并返回 `None`（失败关闭）：直接 `INCR` 会
//! 凭空建键并返回 1，把「键已被并发删除」记成了一次有效尝试；各后端统一成
//! 失败关闭，语义更紧。
//!
//! 连接管理：读写都带超时（无超时的连接会把请求线程永久挂住）；操作遇错即丢弃连接，
//! 下次操作重建。**不重试**——`INCR` 之类命令重试可能把「执行成功但响应丢失」记两次，
//! 重建只发生在下一次独立请求上。没有用 `redis::ConnectionManager`：它属于
//! `connection-manager` feature，本 crate 的 `redis` feature 只带 `dep:redis`，
//! 不为连接复用引入新的编译开关（见 Cargo.toml）。

use std::sync::Mutex;
use std::time::Duration;

use redis::Commands;

use crate::error::Result;

use super::{StoredEntry, Storage};

/// 建连与单次命令的超时上限。
const OP_TIMEOUT: Duration = Duration::from_secs(5);

/// Redis 存储。
pub struct RedisStorage {
    /// 连接不是 `Sync`，且断了要能重建，所以持有 client + 可丢的连接。
    client: redis::Client,
    prefix: String,
    /// `None` = 上次操作把连接弄丢了，下次操作重建。
    conn: Mutex<Option<redis::Connection>>,
}

impl RedisStorage {
    /// 按 URL 连接（如 `redis://127.0.0.1:6379/`）。接线期即建连，连不上直接报错。
    pub fn new(url: &str, prefix: &str) -> Result<Self> {
        let client = redis::Client::open(url)?;
        let conn = connect(&client)?;
        Ok(Self {
            client,
            prefix: prefix.to_string(),
            conn: Mutex::new(Some(conn)),
        })
    }

    /// 按配置连接：前缀取 `captcha.redis_prefix`，地址取环境变量 `REDIS_URL`，
    /// 缺省 `redis://127.0.0.1:6379/`。
    pub fn from_config(config: &crate::config::PosterConfig) -> Result<Self> {
        let url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379/".into());
        Self::new(&url, &config.captcha.redis_prefix)
    }

    /// 跑一次操作：连接丢了就重建（重建失败上抛），操作失败则丢弃连接让下次重建。
    fn with_conn<T>(
        &self,
        op: impl FnOnce(&mut redis::Connection) -> redis::RedisResult<T>,
    ) -> Result<T> {
        let mut guard = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            *guard = Some(connect(&self.client)?);
        }
        let conn = guard.as_mut().expect("上面刚补齐");
        match op(conn) {
            Ok(value) => Ok(value),
            Err(error) => {
                *guard = None;
                Err(error.into())
            }
        }
    }

    fn value_key(&self, key: &str) -> String {
        format!("{}{key}", self.prefix)
    }

    fn counter_key(&self, key: &str) -> String {
        format!("{}{key}:att", self.prefix)
    }
}

/// 建连接并设置读写超时。
fn connect(client: &redis::Client) -> Result<redis::Connection> {
    let conn = client.get_connection_with_timeout(OP_TIMEOUT)?;
    conn.set_write_timeout(Some(OP_TIMEOUT))?;
    conn.set_read_timeout(Some(OP_TIMEOUT))?;
    Ok(conn)
}

impl Storage for RedisStorage {
    fn get(&self, key: &str) -> Result<Option<StoredEntry>> {
        let value_key = self.value_key(key);
        let value: Option<Vec<u8>> = self.with_conn(|conn| conn.get(&value_key))?;
        // TTL 由 Redis 自己维护，`pttl` 是额外一次往返，收益不抵成本，故报 None
        Ok(value.map(|value| StoredEntry { value, ttl: None }))
    }

    fn set(&self, key: &str, value: &[u8], ttl: Duration) -> Result<()> {
        let seconds = ttl_seconds(ttl);
        let value_key = self.value_key(key);
        let counter_key = self.counter_key(key);
        self.with_conn(|conn| -> redis::RedisResult<()> {
            conn.set_ex::<_, _, ()>(&value_key, value, seconds)?;
            // 计数单独放一个键：与值同键的读改写不原子，INCR 没有这个问题
            conn.set_ex::<_, _, ()>(&counter_key, 0u64, seconds)?;
            Ok(())
        })
    }

    fn delete(&self, key: &str) -> Result<bool> {
        let keys = [self.value_key(key), self.counter_key(key)];
        let deleted: u64 = self.with_conn(|conn| conn.del(&keys[..]))?;
        Ok(deleted > 0)
    }

    fn increment(&self, key: &str) -> Result<Option<u64>> {
        let value_key = self.value_key(key);
        let counter_key = self.counter_key(key);
        self.with_conn(|conn| -> redis::RedisResult<Option<u64>> {
            let count: u64 = conn.incr(&counter_key, 1u64)?;
            if count == 1 {
                // 主键已不在（并发校验成功 / 过期 / 从没写过）：这次自增不算数，清掉计数键
                let exists: u64 = conn.exists(&value_key)?;
                if exists == 0 {
                    let _: u64 = conn.del(&counter_key)?;
                    return Ok(None);
                }
                // 计数键是刚建出来的，过期时间跟主键走
                let ttl: i64 = conn.ttl(&value_key)?;
                if ttl > 0 {
                    let _: i64 = conn.expire(&counter_key, ttl)?;
                }
            }
            Ok(Some(count))
        })
    }

    fn increment_or_init(&self, key: &str, ttl: Duration) -> Result<Option<u64>> {
        let seconds = ttl_seconds(ttl);
        // 计数即全部：直接用主键位（没有载荷键可挂 `:att`）
        let counter_key = self.value_key(key);
        self.with_conn(|conn| -> redis::RedisResult<Option<u64>> {
            let count: u64 = conn.incr(&counter_key, 1u64)?;
            if count == 1 {
                // 键必须带上 TTL，否则永远不过期 = 泄漏（每个窗口序号一个键）。
                // EXPIRE 失败宁可当这次计数不可用（失败关闭），并尽力删掉刚建的键。
                if conn.expire::<_, ()>(&counter_key, seconds as i64).is_err() {
                    let _: redis::RedisResult<u64> = conn.del(&counter_key);
                    return Ok(None);
                }
            }
            Ok(Some(count))
        })
    }
}

/// TTL 秒数：截断取整，但下限 1 秒（`SETEX` 不接受 0）。
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
