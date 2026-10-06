//! Poem 集成：`Guard` 提取器 + `GET {path}/{key} → image/png` 路由。
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

use poem::web::Path;
use poem::{
    FromRequest, Request, RequestBody, Response, Route, RouteMethod, get, http::StatusCode,
};

use crate::guard::Guard;

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

/// 路由：`/captcha/new` 与 `/captcha/{key}`。
pub fn routes() -> Route {
    Route::new()
        .at("/captcha/new", get(captcha_new))
        .at("/captcha/:key", get(captcha_image))
}

/// 同 [`routes`]，自定义路径前缀。
pub fn routes_at(prefix: &str) -> Route {
    Route::new()
        .at(format!("{prefix}/new"), get(captcha_new))
        .at(format!("{prefix}/:key"), get(captcha_image))
}

/// 供手工拼装时使用：`MethodRouter` 形式的出图路由。
pub fn captcha_image_route() -> RouteMethod {
    get(captcha_image)
}
