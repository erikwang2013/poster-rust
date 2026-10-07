//! Poem 集成：`Guard` 提取器 + `GET {path}/{key} → image/png` 路由 + 校验端点。
//!
//! ```no_run
//! # use std::sync::Arc;
//! use poster::{Guard, captcha::CaptchaManager};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let guard = Guard::from_manager(Arc::new(CaptchaManager::new()?))?;
//! let app = poem::Route::new()
//!     .nest("/", poster::integrations::poem::routes())
//!     .data(guard);
//! # Ok(())
//! # }
//! ```

use std::net::IpAddr;

use poem::web::{Json, Path, RemoteAddr};
use poem::{
    FromRequest, Request, RequestBody, Response, Route, RouteMethod, get, http::StatusCode, post,
};

use crate::captcha::Answer;
use crate::guard::{Guard, client_identity};

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
    match guard.image(&key) {
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
    let kind = params.get("type").map(String::as_str);
    match guard.create_json(kind) {
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
/// 限流身份按请求派生（[`client_identity`]：`X-Forwarded-For` 第一段 → 对端 IP → `"unknown"`）。
/// 请求体 JSON 解析失败时由 [`Json`] 提取器返回 400（超大 413）。
#[poem::handler]
pub async fn captcha_verify(
    guard: Guard,
    req: &Request,
    Json(body): Json<VerifyRequest>,
) -> Response {
    let identity = client_identity(req.header("x-forwarded-for"), peer_ip(req.remote_addr()));
    match guard.verify_as(&body.key, body.answer, &identity) {
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
