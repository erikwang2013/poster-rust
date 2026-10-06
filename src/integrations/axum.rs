//! axum 集成：`Guard` 提取器 + `GET {path}/{key} → image/png` 出图路由 + 校验端点。
//!
//! ```no_run
//! use std::sync::Arc;
//! use axum::{Router, routing::get};
//! use poster::{Guard, captcha::CaptchaManager, integrations::axum::GuardState};
//!
//! #[derive(Clone)]
//! struct AppState { captcha: Guard }
//! impl GuardState for AppState {
//!     fn guard(&self) -> &Guard { &self.captcha }
//! }
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let guard = Guard::from_manager(Arc::new(CaptchaManager::new()?))?;
//! let app: Router = Router::new()
//!     .merge(poster::integrations::axum::captcha_routes::<AppState>())
//!     .with_state(AppState { captcha: guard });
//! # Ok(())
//! # }
//! ```

use axum::Json;
use axum::extract::{FromRequestParts, Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;

use crate::captcha::Answer;
use crate::guard::Guard;

/// 应用状态需要提供的挂钩：返回 `Guard`（通常是一次 `Clone`）。
pub trait GuardState {
    fn guard(&self) -> &Guard;
}

/// `Guard` 的 axum 提取器：请求期克隆（两次原子计数），不可失败。
impl<S> FromRequestParts<S> for Guard
where
    S: GuardState + Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(_parts: &mut axum::http::request::Parts, state: &S) -> Result<Self, Self::Rejection> {
        Ok(state.guard().clone())
    }
}

/// `GET {path}/{key}` → PNG（`Cache-Control: no-store`），等价 PHP 版 `captcha.route`。
pub async fn captcha_image<S>(State(state): State<S>, Path(key): Path<String>) -> Response
where
    S: GuardState + Clone + Send + Sync + 'static,
{
    match state.guard().image(&key) {
        Ok(Some(img)) => (
            [
                (header::CONTENT_TYPE, img.content_type),
                (header::CACHE_CONTROL, img.cache_control),
            ],
            img.bytes,
        )
            .into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// 校验请求体：`{ "key": "…", "answer": … }`，`answer` 形状同 [`Answer`] 的 serde 表示。
#[derive(Debug, serde::Deserialize)]
pub struct VerifyRequest {
    pub key: String,
    pub answer: Answer,
}

/// `POST {path}/verify` → `{"pass": true|false}`。
pub async fn captcha_verify<S>(State(state): State<S>, Json(req): Json<VerifyRequest>) -> Response
where
    S: GuardState + Clone + Send + Sync + 'static,
{
    match state.guard().verify(&req.key, req.answer) {
        Ok(pass) => Json(serde_json::json!({ "pass": pass })).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// 生成端点：`GET {path}/new?type=click` → `CaptchaResult` JSON。
pub async fn captcha_new<S>(
    State(state): State<S>,
    axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response
where
    S: GuardState + Clone + Send + Sync + 'static,
{
    let kind = query.get("type").map(String::as_str);
    match state.guard().create_json(kind) {
        Ok(value) => Json(value).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// 一次性注册三条路由：`GET {path}/{key}`、`POST {path}/verify`、`GET {path}/new`。
///
/// `path` 传前缀（默认 `/captcha`），与 PHP 版 `captcha.route.path` 对应。
pub fn captcha_routes<S>() -> Router<S>
where
    S: GuardState + Clone + Send + Sync + 'static,
{
    captcha_routes_at("/captcha")
}

/// 同上，自定义路径前缀。
pub fn captcha_routes_at<S>(path: &str) -> Router<S>
where
    S: GuardState + Clone + Send + Sync + 'static,
{
    Router::new()
        .route(&format!("{path}/new"), get(captcha_new::<S>))
        .route(&format!("{path}/verify"), post(captcha_verify::<S>))
        .route(&format!("{path}/{{key}}"), get(captcha_image::<S>))
}
