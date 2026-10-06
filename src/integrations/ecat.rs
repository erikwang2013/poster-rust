//! e-cat 集成：e-cat 的 HTTP 传输层基于 axum，
//! `Guard` 提取器与出图路由直接复用 axum 适配器（本模块即其别名）。
//!
//! ```no_run
//! # use std::sync::Arc;
//! use poster::{Guard, captcha::CaptchaManager};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # #[derive(Clone)]
//! # struct AppState { captcha: Guard }
//! # impl poster::integrations::ecat::GuardState for AppState {
//! #     fn guard(&self) -> &Guard { &self.captcha }
//! # }
//! let guard = Guard::from_manager(Arc::new(CaptchaManager::new()?))?;
//! let app: axum::Router = poster::integrations::ecat::captcha_routes::<AppState>()
//!     .with_state(AppState { captcha: guard });
//! # Ok(())
//! # }
//! ```

pub use super::axum::*;
