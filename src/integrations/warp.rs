//! Warp 集成：把 `Guard` 装进 filter 链 + `GET {path}/{key} → image/png` 路由。
//!
//! ```no_run
//! # use std::sync::Arc;
//! use poster::{Guard, captcha::CaptchaManager};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let guard = Guard::from_manager(Arc::new(CaptchaManager::new()?))?;
//! let routes = poster::integrations::warp::routes(guard);
//! // warp::serve(routes).run(([127,0,0,1], 8080)).await;
//! # Ok(())
//! # }
//! ```

use std::convert::Infallible;

use warp::Filter;
use warp::http::{StatusCode, header};

use crate::guard::Guard;

/// 把 `Guard` 注入 filter 链：`warp::path("api").and(guard_filter(g)).and_then(handler)`
pub fn guard_filter(guard: Guard) -> impl Filter<Extract = (Guard,), Error = Infallible> + Clone {
    warp::any().map(move || guard.clone())
}

/// PNG 出图回复（含 `Cache-Control: no-store`）。
fn image_reply(guard: &Guard, key: &str) -> Box<dyn warp::Reply + Send> {
    match guard.image(key) {
        Ok(Some(img)) => Box::new(warp::reply::with_header(
            warp::reply::with_status(img.bytes, StatusCode::OK),
            header::CACHE_CONTROL,
            "no-store",
        )),
        Ok(None) => Box::new(StatusCode::NOT_FOUND),
        Err(_) => Box::new(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// 路由：`GET /captcha/new`、`GET /captcha/{key}`。
pub fn routes(
    guard: Guard,
) -> impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone {
    routes_at(guard, "captcha")
}

/// 同 [`routes`]，自定义路径前缀。
pub fn routes_at(
    guard: Guard,
    prefix: &'static str,
) -> impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone {
    let new_route = {
        let guard = guard.clone();
        warp::path(prefix)
            .and(warp::path("new"))
            .and(warp::path::end())
            .and(guard_filter(guard))
            .map(|g: Guard| {
                warp::reply::json(
                    &g.create_json(None)
                        .unwrap_or_else(|_| serde_json::json!({ "error": "internal" })),
                )
            })
    };
    let image_route = warp::path(prefix)
        .and(warp::path::param::<String>())
        .and(warp::path::end())
        .and(guard_filter(guard))
        .map(|key: String, g: Guard| image_reply(&g, &key));
    new_route.or(image_route)
}
