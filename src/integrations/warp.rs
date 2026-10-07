//! Warp 集成：把 `Guard` 装进 filter 链 + `GET {path}/{key} → image/png` 路由 + 校验端点。
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
use std::net::SocketAddr;

use warp::Filter;
use warp::http::{StatusCode, header};

use crate::captcha::Answer;
use crate::guard::{Guard, client_identity};

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

/// 校验请求体：`{ "key": "…", "answer": … }`。
#[derive(Debug, serde::Deserialize)]
pub struct VerifyRequest {
    pub key: String,
    pub answer: Answer,
}

/// `POST {path}/verify` 的回复（`{"pass": true|false}`）。
///
/// 限流身份按请求派生（[`client_identity`]：`X-Forwarded-For` 第一段 → 对端 IP → `"unknown"`）。
/// 请求体 JSON 解析失败由 `warp::body::json()` 拒绝 → 400。
fn verify_reply(
    guard: &Guard,
    remote: Option<SocketAddr>,
    xff: Option<String>,
    body: VerifyRequest,
) -> Box<dyn warp::Reply + Send> {
    let identity = client_identity(xff.as_deref(), remote.map(|addr| addr.ip()));
    match guard.verify_as(&body.key, body.answer, &identity) {
        Ok(pass) => Box::new(warp::reply::json(&serde_json::json!({ "pass": pass }))),
        Err(_) => Box::new(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// 路由：`GET /captcha/new`、`POST /captcha/verify`、`GET /captcha/{key}`。
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
    // 出图路由没有方法过滤（`POST /captcha/verify` 也会被 `path::param` 吃掉），必须排在它前面。
    let verify_route = {
        let guard = guard.clone();
        warp::path(prefix)
            .and(warp::path("verify"))
            .and(warp::path::end())
            .and(warp::post())
            .and(warp::addr::remote())
            .and(warp::header::optional::<String>("x-forwarded-for"))
            .and(warp::body::json())
            .and(guard_filter(guard))
            .map(
                |remote: Option<SocketAddr>, xff: Option<String>, body: VerifyRequest, g: Guard| {
                    verify_reply(&g, remote, xff, body)
                },
            )
    };
    // 出图路由限 GET：否则 `POST /captcha/verify` 的非法 JSON 拒绝会「回退」到这里变成 404；
    // 顺带让出图端点与其余框架一致地只认 GET。
    let image_route = warp::path(prefix)
        .and(warp::path::param::<String>())
        .and(warp::path::end())
        .and(warp::get())
        .and(guard_filter(guard))
        .map(|key: String, g: Guard| image_reply(&g, &key));
    new_route.or(verify_route).or(image_route)
}
