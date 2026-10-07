#![forbid(unsafe_code)]
//! poster-rust —— Rust 图片验证码与海报生成工具包。
//!
//! 参照 PHP 版 [poster-php](https://github.com/erikwang2013/poster-php) 移植，只做两件事，
//! 并且做到够用：
//!
//! - **验证码**：点击 / 旋转 / 滑块三种人机校验 + 随机切换，纯 Rust 生成图片与答案，
//!   不依赖第三方服务。
//! - **海报生成**：链式 Builder API，14 种元素覆盖文字、图片、二维码、表格、图表、
//!   日历等排版需求。
//!
//! 核心不依赖任何 Web 框架；原生 [`Guard`] 请求守卫 + 8 个框架集成
//! （axum / actix-web / rocket / poem / salvo / warp / bee-rust / e-cat）以可选 feature 提供。
//!
//! 项目宠物 **Posty** 随包分发（[`assets::PET_PNG`] / [`assets::PET_SVG`]），
//! 可用 `PosterBuilder::add_pet()` 画进海报，也可配置为缺图占位图。
//!
//! ```
//! use poster::mascot;
//! assert_eq!(mascot::NAME, "Posty");
//! ```

use std::sync::{Arc, OnceLock};

use captcha::{Answer, CaptchaBuilder, CaptchaManager};

pub mod assets;
pub mod captcha;
pub mod config;
pub mod drivers;
pub mod error;
pub mod guard;
pub mod integrations;
pub mod mascot;
pub mod poster;
pub mod qrcode;
pub mod storage;

pub use config::PosterConfig;
pub use drivers::{Font, ImageDriver};
pub use error::{PosterError, Result};
pub use guard::{CaptchaImage, Guard};
pub use poster::{PosterBuilder, PosterTemplate};

/// 进程级默认管理器（`captcha_create` / `captcha_verify` 之间共享同一存储，
/// 对应 PHP 版 `StorageFactory` 的静态缓存语义）。
///
/// `captcha.file_path` 配了就用文件存储（跨进程共享），否则进程内存储。
pub fn default_manager() -> Result<Arc<CaptchaManager>> {
    static MANAGER: OnceLock<Result<Arc<CaptchaManager>>> = OnceLock::new();
    match MANAGER.get_or_init(|| CaptchaManager::new().map(Arc::new)) {
        Ok(manager) => Ok(Arc::clone(manager)),
        Err(err) => Err(PosterError::Captcha(format!(
            "默认验证码管理器初始化失败: {err}"
        ))),
    }
}

/// 生成一张验证码（等价 PHP `captcha_create()`）。
///
/// `captcha_type` 为 `None` 时取配置默认类型；返回的
/// [`CaptchaBuilder`] 链式设置后调 `generate()`。
pub fn captcha_create(captcha_type: Option<&str>) -> Result<CaptchaBuilder> {
    default_manager()?.create(captcha_type)
}

/// 校验用户答案（等价 PHP `captcha_verify()`；限流身份为默认身份，多用户服务
/// 请用 [`Guard::verify_as`] 传入 IP / uid）。
pub fn captcha_verify(key: &str, answer: Answer) -> Result<bool> {
    default_manager()?.verify(key, answer)
}

/// 新建海报 Builder（等价 PHP `poster_create()`）；宽高为 `None` 时取配置默认（750×1334）。
pub fn poster_create(width: Option<u32>, height: Option<u32>) -> Result<PosterBuilder> {
    let mut builder = PosterBuilder::new()?;
    if let Some(w) = width {
        builder.width(w);
    }
    if let Some(h) = height {
        builder.height(h);
    }
    Ok(builder)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_share_one_default_manager() {
        let a = default_manager().unwrap();
        let b = default_manager().unwrap();
        assert!(Arc::ptr_eq(&a, &b), "辅助函数之间必须共享同一管理器/存储");
    }

    #[test]
    fn captcha_create_verify_roundtrip_through_helpers() {
        let result = captcha_create(Some("slider")).unwrap().generate().unwrap();
        // 错误答案必须失败（真实答案不返回给调用方）
        assert!(!captcha_verify(&result.key, Answer::Slider(-999.0)).unwrap());
    }

    #[test]
    fn poster_create_defaults_to_config_size() {
        let builder = poster_create(None, None).unwrap();
        let canvas = builder.render().unwrap();
        assert_eq!(canvas.size(), (750, 1334));
    }
}
