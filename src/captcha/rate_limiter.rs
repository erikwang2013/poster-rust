//! 会话 / 账号级固定窗口限流（跨 key 生效），对应 PHP `RateLimiter`。
//!
//! 单 key 的 `captcha.max_attempts` 只按 key 计数，挡不住「每次先领新 key 再猜一次」的盲猜：
//! slider 固定猜某个 x、rotate 固定猜某个角度，命中率 = 命中区间 / 参数空间，与 key 数无关。
//! 这里按「身份」（登录用户可用 uid，默认由框架层注入客户端 IP 等）在窗口内计数，
//! 超出 `captcha.rate_limit.max` 即拒绝。
//!
//! 计数键为 `rate:{身份哈希}`，窗口序号写进记录：窗口滚动即重新计数，TTL 取窗口长度，
//! 过期由存储自动回收。计数取自 `Storage::increment()` 的原子自增返回值。
//!
//! `max <= 0` 或 `window_secs == 0` 视为关闭限流（全量放行，同 PHP）。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;

use crate::config::RateLimitOptions;
use crate::error::Result;
use crate::storage::{Storage, now_secs};

/// 计数键前缀（同 PHP `RateLimiter::KEY_PREFIX`）。
pub(crate) const KEY_PREFIX: &str = "rate:";

/// 记录一次校验尝试。
///
/// `Ok(false)` = 本窗口内已超限，调用方应直接判失败。
/// 存储故障会以 `Err` 上抛（PHP 版一律吞成 `false`；Rust 侧让部署问题可见），
/// 但「计数不可用」这类**语义**状态仍然失败关闭，不会放行。
pub(crate) fn allow(
    config: &RateLimitOptions,
    storage: &dyn Storage,
    identity: &str,
) -> Result<bool> {
    if config.max == 0 || config.window_secs == 0 {
        return Ok(true);
    }

    let key = format!("{KEY_PREFIX}{:016x}", hash_identity(identity));
    let window_index = now_secs() / config.window_secs;

    // 窗口滚动即重置：用新窗口序号覆写记录（set 会同时把计数清零）
    let stale = match storage.get(&key)? {
        None => true,
        Some(entry) => entry.value != window_index.to_string().as_bytes(),
    };
    if stale {
        storage.set(
            &key,
            window_index.to_string().as_bytes(),
            Duration::from_secs(config.window_secs),
        )?;
    }

    // 自增不可用（键刚过期 / 被并发清掉 / 写失败）时拿不到可信计数，
    // 此时必须失败关闭而不是当作第 1 次放行
    let Some(count) = storage.increment(&key)? else {
        return Ok(false);
    };

    Ok(count <= config.max as u64)
}

/// 身份 → 定长计数键。用 `DefaultHasher`（`SipHash-1-3`，固定种子）等价 PHP 的 `md5($identity)`：
/// 只为定长，不为抗碰撞——攻击者控制不了「被限流者是不是自己」这件事。
fn hash_identity(identity: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    identity.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStorage;

    fn config(max: u32, window_secs: u64) -> RateLimitOptions {
        RateLimitOptions { max, window_secs }
    }

    #[test]
    fn allows_up_to_max_then_rejects() {
        let s = MemoryStorage::new();
        let c = config(3, 60);
        for i in 1..=3 {
            assert!(allow(&c, &s, "alice").unwrap(), "第 {i} 次应放行");
        }
        assert!(!allow(&c, &s, "alice").unwrap(), "超出窗口上限");
    }

    #[test]
    fn counters_are_per_identity() {
        let s = MemoryStorage::new();
        let c = config(1, 60);
        assert!(allow(&c, &s, "alice").unwrap());
        assert!(!allow(&c, &s, "alice").unwrap());
        assert!(allow(&c, &s, "bob").unwrap(), "另一个身份不受影响");
    }

    #[test]
    fn window_rollover_resets_counter() {
        let s = MemoryStorage::new();
        // 窗口 1 秒：等窗口滚过去后计数应重新开始
        let c = config(1, 1);
        assert!(allow(&c, &s, "alice").unwrap());
        assert!(!allow(&c, &s, "alice").unwrap());
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            if allow(&c, &s, "alice").unwrap() {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "窗口未滚动");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn zero_config_disables_limiting() {
        let s = MemoryStorage::new();
        for _ in 0..100 {
            assert!(allow(&config(0, 60), &s, "alice").unwrap());
            assert!(allow(&config(30, 0), &s, "alice").unwrap());
        }
    }
}
