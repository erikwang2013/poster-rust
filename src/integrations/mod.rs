//! 框架集成（feature 门控）。
//!
//! 八个适配器全部产出 [`crate::guard::Guard`]：
//! axum / actix-web / rocket / poem / salvo / warp / bee-rust / e-cat。
//! 每个适配器都提供「请求守卫提取」与「出图路由」（`GET {path}/{key} → image/png`）。

#[cfg(feature = "axum")]
pub mod axum;

#[cfg(feature = "actix")]
pub mod actix;

#[cfg(feature = "rocket")]
pub mod rocket;

#[cfg(feature = "poem")]
pub mod poem;

#[cfg(feature = "salvo")]
pub mod salvo;

#[cfg(feature = "warp")]
pub mod warp;

#[cfg(feature = "bee-rust")]
pub mod bee_rust;

#[cfg(feature = "ecat")]
pub mod ecat;
