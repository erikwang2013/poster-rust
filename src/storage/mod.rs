//! 验证码存储后端：带 TTL 的字节存储 + 原子计数。
//!
//! 对应 PHP 版 `src/Storage/*`，语义逐项对齐：
//! - `set` 写入新值并**重置**该 key 的尝试计数（PHP `set()` 里 `attempts => 0`）；
//! - `increment` 是「读改写」，必须原子；键不存在 / 已过期 / 写入失败时返回 `None`，
//!   调用方（Manager / RateLimiter）据此**失败关闭**（PHP 用返回值 0 表达同一状态）；
//! - 过期条目在读取时惰性清除。
//!
//! 与 PHP 的差异：PHP 用 `bool`/`int` 兼作错误码，Rust 侧错误走 `Result`，
//! 「计数不可用」走 `Option`，不再让 0/false 承担两种含义。

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::Result;

mod file;
mod memory;
#[cfg(feature = "redis")]
mod redis;

pub use file::{FileStorage, default_dir};
pub use memory::MemoryStorage;
#[cfg(feature = "redis")]
pub use redis::RedisStorage;

/// 存储条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEntry {
    /// 原始字节（验证码模块存 JSON 载荷，图片端点存 PNG）。
    pub value: Vec<u8>,
    /// 剩余有效期；后端不报告时为 `None`（如 Redis，过期由服务端自行处理）。
    pub ttl: Option<Duration>,
}

impl StoredEntry {
    /// 解析载荷 JSON。
    pub fn json(&self) -> Result<serde_json::Value> {
        Ok(serde_json::from_slice(&self.value)?)
    }
}

/// 存储后端。所有实现必须 `Send + Sync`（管理器以 `Arc<dyn Storage>` 共享给各 Web 框架的请求守卫）。
pub trait Storage: Send + Sync {
    /// 读取；不存在或已过期返回 `None`。
    fn get(&self, key: &str) -> Result<Option<StoredEntry>>;

    /// 写入并重置计数；`ttl` 为有效期（`Duration::ZERO` 视为立即过期）。
    fn set(&self, key: &str, value: &[u8], ttl: Duration) -> Result<()>;

    /// 删除；键不存在不算错误。
    fn delete(&self, key: &str) -> Result<()>;

    /// 原子自增该 key 的尝试计数并返回新值（从 1 开始）。
    ///
    /// `None` = 键不存在 / 已过期 / 存储不可写——**绝不**等价于「第 1 次」，
    /// 调用方拿到 `None` 必须判失败，否则并发下的「读计数 → 校验 → 累加」
    /// 窗口会被用来放大放行次数。
    fn increment(&self, key: &str) -> Result<Option<u64>>;
}

/// 文件/内存后端共用的记录格式：值 + 尝试计数 + 绝对到期时刻（Unix 毫秒）。
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Record {
    /// 值（base64，让二进制 PNG 也能塞进 JSON）。
    pub v: String,
    /// 尝试计数。
    #[serde(default)]
    pub a: u64,
    /// 到期时刻（Unix 毫秒）；`u64::MAX` = 永不过期（TTL 溢出时的兜底）。
    pub e: u64,
}

/// 当前 Unix 毫秒。
pub(crate) fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 当前 Unix 秒。
pub(crate) fn now_secs() -> u64 {
    now_millis() / 1000
}

/// 到期时刻：`ttl` 溢出可表示范围时返回 `u64::MAX`（永不过期）。
pub(crate) fn expiry_of(ttl: Duration) -> u64 {
    now_millis().saturating_add(ttl.as_millis() as u64)
}

/// base64 编码（记录格式用）。
pub(crate) fn b64_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// base64 解码；非法输入返回 `None`。
pub(crate) fn b64_decode(text: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expiry_uses_ttl_and_saturates() {
        let e = expiry_of(Duration::from_secs(300));
        assert!(e > now_millis() + 299_000);
        assert_eq!(expiry_of(Duration::from_secs(u64::MAX)), u64::MAX);
    }

    #[test]
    fn base64_round_trips_binary() {
        let raw = [0u8, 1, 2, 255, 254];
        assert_eq!(b64_decode(&b64_encode(&raw)).unwrap(), raw);
        assert!(b64_decode("!!!not base64!!!").is_none());
    }
}
