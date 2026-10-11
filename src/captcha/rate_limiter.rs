//! 会话 / 账号级固定窗口限流（跨 key 生效）。
//!
//! 单 key 的 `captcha.max_attempts` 只按 key 计数，挡不住「每次先领新 key 再猜一次」的盲猜：
//! slider 固定猜某个 x、rotate 固定猜某个角度，命中率 = 命中区间 / 参数空间，与 key 数无关。
//! 这里按「身份」（登录用户可用 uid，默认由框架层注入客户端 IP 等）在窗口内计数，
//! 超出 `captcha.rate_limit.max` 即拒绝。
//!
//! 计数键为 `rate:{身份哈希}:{窗口序号}`：窗口序号写进键名，滚动即换键、旧键靠 TTL 自然过期；
//! 键的创建 + 自增走 `Storage::increment_or_init()` 一次原子操作，没有「读到旧窗口 → 覆写 →
//! 自增」三步里的并发突刺（两次 `set` 之间读到的计数会互相覆盖）。
//!
//! `max <= 0` 或 `window_secs == 0` 视为关闭限流（全量放行）。

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::sync::OnceLock;
use std::time::Duration;

use crate::config::RateLimitOptions;
use crate::error::Result;
use crate::storage::{Storage, now_secs};

/// 计数键前缀。
pub(crate) const KEY_PREFIX: &str = "rate:";

/// 进程级哈希种子。
///
/// `DefaultHasher::new()` 是固定种子：身份哈希可离线复算，攻击者能拼出别人的计数键
/// （历史上正是靠这一点删掉了自己的限流记录）。`RandomState` 每进程随机；
/// 必须 `static`——每次调用新建会让同一身份的哈希各不相同，分桶直接失效。
static HASH_STATE: OnceLock<RandomState> = OnceLock::new();

/// 记录一次校验尝试。
///
/// `Ok(false)` = 本窗口内已超限，调用方应直接判失败。
/// 存储故障会以 `Err` 上抛（让部署问题可见），
/// 但「计数不可用」这类**语义**状态仍然失败关闭，不会放行。
pub(crate) fn allow(
    config: &RateLimitOptions,
    storage: &dyn Storage,
    identity: &str,
) -> Result<bool> {
    if config.max == 0 || config.window_secs == 0 {
        return Ok(true);
    }

    let window_index = now_secs() / config.window_secs;
    let key = format!(
        "{KEY_PREFIX}{:016x}:{window_index}",
        hash_identity(identity)
    );
    let ttl = Duration::from_secs(config.window_secs);

    // 键不存在→创建为 1，存在→原子自增（窗口滚动即换键，无覆写竞态）。
    // 计数不可用（写失败 / TTL 落不上）时拿不到可信计数，必须失败关闭。
    let Some(count) = storage.increment_or_init(&key, ttl)? else {
        return Ok(false);
    };

    Ok(count <= config.max as u64)
}

/// 身份 → 定长计数键。用进程级 `RandomState`（随机种子的 SipHash-1-3）：
/// 只为定长，不为抗碰撞——但要防「可离线复算」。
fn hash_identity(identity: &str) -> u64 {
    HASH_STATE.get_or_init(RandomState::new).hash_one(identity)
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
