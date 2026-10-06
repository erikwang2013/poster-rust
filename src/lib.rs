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
//! 核心不依赖任何 Web 框架；axum / actix-web 集成以可选 feature 提供。
//!
//! 项目宠物 **Posty** 随包分发（[`assets::PET_PNG`] / [`assets::PET_SVG`]），
//! 可用 `PosterBuilder::add_pet()` 画进海报，也可配置为缺图占位图。
//!
//! ```
//! use poster::mascot;
//! assert_eq!(mascot::NAME, "Posty");
//! ```

pub mod assets;
pub mod captcha;
pub mod config;
pub mod drivers;
pub mod error;
pub mod mascot;
pub mod poster;
pub mod qrcode;
pub mod storage;

pub use config::PosterConfig;
pub use drivers::{Font, ImageDriver};
pub use error::{PosterError, Result};
