//! Poem 集成：`Guard` 提取器 + `GET {path}/{key} → image/png` 路由 + 校验端点。
//!
//! ```no_run
//! # use std::sync::Arc;
//! use poem::EndpointExt;
//! use poster::{Guard, captcha::CaptchaManager};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let guard = Guard::from_manager(Arc::new(CaptchaManager::new()?))?;
//! let app = poem::Route::new()
//!     .nest("/", poster::integrations::poem::routes())
//!     .data(guard);          // `data()` 来自 `EndpointExt`
//! # Ok(())
//! # }
//! ```

use std::net::IpAddr;

use poem::error::ReadBodyError;
use poem::web::{Path, RemoteAddr};
use poem::{
    Body, FromRequest, Request, RequestBody, Response, Route, RouteMethod, get, http::StatusCode,
    post,
};

use crate::captcha::Answer;
use crate::guard::{Guard, client_identity};

use super::MAX_VERIFY_BODY;

/// 对端 IP：只有 TCP 连接有（Unix socket / 自定义连接返回 `None`）。
fn peer_ip(addr: &RemoteAddr) -> Option<IpAddr> {
    match &addr.0 {
        poem::Addr::SocketAddr(addr) => Some(addr.ip()),
        _ => None,
    }
}

/// `Guard` 的 Poem 提取器：从 `Route::data(guard)` 取。
impl<'a> FromRequest<'a> for Guard {
    async fn from_request(req: &'a Request, _body: &mut RequestBody) -> poem::Result<Self> {
        req.data::<Guard>()
            .cloned()
            .ok_or_else(|| poem::Error::from_status(StatusCode::INTERNAL_SERVER_ERROR))
    }
}

/// `GET {path}/{key}` → PNG。
#[poem::handler]
pub async fn captcha_image(guard: Guard, Path(key): Path<String>) -> Response {
    match super::run_blocking(move || guard.image(&key)).await {
        Ok(Some(img)) => Response::builder()
            .content_type(img.content_type)
            .header("cache-control", img.cache_control)
            .body(img.bytes),
        Ok(None) => Response::builder().status(StatusCode::NOT_FOUND).finish(),
        Err(_) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .finish(),
    }
}

/// 生成端点：`GET {path}/new?type=click` → JSON。
#[poem::handler]
pub async fn captcha_new(
    guard: Guard,
    poem::web::Query(params): poem::web::Query<std::collections::HashMap<String, String>>,
) -> Response {
    let kind = params.get("type").cloned();
    match super::run_blocking(move || guard.create_json(kind.as_deref())).await {
        Ok(value) => Response::builder()
            .content_type("application/json")
            .body(serde_json::to_string(&value).unwrap_or_default()),
        Err(_) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .finish(),
    }
}

/// 校验请求体：`{ "key": "…", "answer": … }`。
#[derive(Debug, serde::Deserialize)]
pub struct VerifyRequest {
    pub key: String,
    pub answer: Answer,
}

/// `POST {path}/verify` → `{"pass": true|false}`。
///
/// 限流身份按请求派生（[`client_identity`]：对端 IP → `"unknown"`）。
/// `X-Forwarded-For` 默认**不**采信（可伪造），仅在 [`Guard::with_trust_forwarded_for`] 显式
/// 开启后才作为身份参与分桶。
/// 请求体自己按 [`MAX_VERIFY_BODY`](super::MAX_VERIFY_BODY) 读（不用 `Json` 提取器：
/// 它会无上限全量读进内存），超限 413，JSON 解析失败 400。
#[poem::handler]
pub async fn captcha_verify(guard: Guard, req: &Request, payload: Body) -> Response {
    let data = match payload.into_bytes_limit(MAX_VERIFY_BODY).await {
        Ok(data) => data,
        Err(ReadBodyError::PayloadTooLarge) => {
            return Response::builder()
                .status(StatusCode::PAYLOAD_TOO_LARGE)
                .finish();
        }
        Err(_) => return Response::builder().status(StatusCode::BAD_REQUEST).finish(),
    };
    let body: VerifyRequest = match serde_json::from_slice(&data) {
        Ok(body) => body,
        Err(_) => return Response::builder().status(StatusCode::BAD_REQUEST).finish(),
    };
    let xff = if guard.trust_forwarded_for() {
        req.header("x-forwarded-for")
    } else {
        None
    };
    let identity = client_identity(xff, peer_ip(req.remote_addr()));
    match super::run_blocking(move || guard.verify_as(&body.key, body.answer, &identity)).await {
        Ok(pass) => Response::builder()
            .content_type("application/json")
            .body(serde_json::json!({ "pass": pass }).to_string()),
        Err(_) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .finish(),
    }
}

/// 路由：`/captcha/new`、`/captcha/verify` 与 `/captcha/{key}`（具名段优先于参数段）。
pub fn routes() -> Route {
    Route::new()
        .at("/captcha/new", get(captcha_new))
        .at("/captcha/verify", post(captcha_verify))
        .at("/captcha/:key", get(captcha_image))
}

/// 同 [`routes`]，自定义路径前缀。
pub fn routes_at(prefix: &str) -> Route {
    Route::new()
        .at(format!("{prefix}/new"), get(captcha_new))
        .at(format!("{prefix}/verify"), post(captcha_verify))
        .at(format!("{prefix}/:key"), get(captcha_image))
}

/// 供手工拼装时使用：`MethodRouter` 形式的出图路由。
pub fn captcha_image_route() -> RouteMethod {
    get(captcha_image)
}
