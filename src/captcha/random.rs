//! 随机切换：从 click / rotate / slider 里随机选一种，对应 PHP `CaptchaFactory` 的
//! `'random'` 分支（PHP 没有独立的 RandomCaptcha 类，随机就发生在工厂里）。

use rand::Rng;

use super::CaptchaType;

/// 三选一。
pub(crate) fn pick() -> CaptchaType {
    const TYPES: [CaptchaType; 3] = [CaptchaType::Click, CaptchaType::Rotate, CaptchaType::Slider];
    TYPES[rand::rng().random_range(0..TYPES.len())]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn covers_all_three_types() {
        let seen: HashSet<CaptchaType> = (0..200).map(|_| pick()).collect();
        assert_eq!(seen.len(), 3, "三种类型都应被抽到: {seen:?}");
    }
}
