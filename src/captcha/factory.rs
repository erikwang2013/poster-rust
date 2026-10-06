//! 验证码工厂，对应 PHP `CaptchaFactory`：类型字符串 → Builder。

use std::sync::Arc;

use crate::error::Result;

use super::{CaptchaBuilder, CaptchaType, Ctx, Difficulty};

/// 验证码工厂（内部：对外入口是 [`super::CaptchaManager::create`]）。
pub(crate) struct CaptchaFactory;

impl CaptchaFactory {
    /// 建 Builder。
    ///
    /// `captcha_type` 为 `None` 时取配置的 `captcha.default_type`；
    /// `random` 在这里就解析成 click / rotate / slider 之一（同 PHP 工厂）。
    pub(crate) fn create(ctx: Arc<Ctx>, captcha_type: Option<&str>) -> Result<CaptchaBuilder> {
        let requested = match captcha_type {
            Some(text) => CaptchaType::parse(text)?,
            None => CaptchaType::parse(&ctx.config.captcha.default_type)?,
        };
        let difficulty = Difficulty::parse(&ctx.config.captcha.default_difficulty)?;
        Ok(CaptchaBuilder::new(ctx, requested.resolve(), difficulty))
    }
}
