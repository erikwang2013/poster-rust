//! bee-rust 集成：`bee_router` 的路由器是 axum Router 的封装，
//! 因此直接复用 axum 提取器与路由，只按 bee 的风格包一层路由组（`RouteGroup`）。
//!
//! ```no_run
//! # use std::sync::Arc;
//! use poster::{Guard, captcha::CaptchaManager, integrations::bee};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let guard = Guard::from_manager(Arc::new(CaptchaManager::new()?))?;
//! let app = bee_router::Router::new()
//!     .ns("/", |group| bee::register(group))
//!     .with_state(guard);
//! # Ok(())
//! # }
//! ```

use bee_router::router::RouteGroup;

use super::axum::{GuardState, captcha_image, captcha_new};
use crate::guard::Guard;

/// 在 bee_router 的路由组上注册出图与生成两条路由（前缀由调用方的 `ns()` 决定）。
pub fn register<S>(group: RouteGroup<S>) -> RouteGroup<S>
where
    S: GuardState + Clone + Send + Sync + 'static,
{
    group
        .get("/captcha/new", captcha_new::<S>)
        .get("/captcha/{key}", captcha_image::<S>)
}

/// 生成路由组所需的 axum 状态挂钩：`impl GuardState for AppState`。
pub use super::axum::GuardState as BeeGuardState;

/// 便捷构造：把 `Guard` 直接作为状态构建 axum Router（bee 的 `build()` 产物同类型）。
pub fn router<S>(guard: Guard) -> axum::Router<S>
where
    S: GuardState + Clone + Send + Sync + 'static,
{
    let _ = guard;
    super::axum::captcha_routes_at("/captcha")
}
