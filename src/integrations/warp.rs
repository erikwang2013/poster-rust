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

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;

use warp::Filter;
use warp::http::{StatusCode, header};

use crate::captcha::Answer;
use crate::guard::{Guard, client_identity};

use super::MAX_VERIFY_BODY;

/// 把 `Guard` 注入 filter 链：`warp::path("api").and(guard_filter(g)).and_then(handler)`
pub fn guard_filter(guard: Guard) -> impl Filter<Extract = (Guard,), Error = Infallible> + Clone {
    warp::any().map(move || guard.clone())
}

/// PNG 出图回复（`Content-Type: image/png` + `Cache-Control: no-store`，对齐其余框架）。
fn image_reply(guard: &Guard, key: &str) -> Box<dyn warp::Reply + Send> {
    match guard.image(key) {
        Ok(Some(img)) => Box::new(warp::reply::with_header(
            warp::reply::with_header(
                warp::reply::with_status(img.bytes, StatusCode::OK),
                header::CONTENT_TYPE,
                img.content_type,
            ),
            header::CACHE_CONTROL,
            img.cache_control,
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
/// 限流身份按请求派生（[`client_identity`]：对端 IP → `"unknown"`）。
/// `X-Forwarded-For` 默认**不**采信（可伪造），仅在 [`Guard::with_trust_forwarded_for`] 显式
/// 开启后才作为身份参与分桶。
/// 请求体 JSON 解析失败由 `warp::body::json()` 拒绝 → 400；超过
/// [`MAX_VERIFY_BODY`](super::MAX_VERIFY_BODY) 由 `content_length_limit` 拒绝 → 413。
fn verify_reply(
    guard: &Guard,
    remote: Option<SocketAddr>,
    xff: Option<String>,
    body: VerifyRequest,
) -> Box<dyn warp::Reply + Send> {
    let xff = if guard.trust_forwarded_for() {
        xff.as_deref()
    } else {
        None
    };
    let identity = client_identity(xff, remote.map(|addr| addr.ip()));
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
    // 生成失败回 500（对齐其余框架；旧实现把错误混在 200 的 `{"error":"internal"}` 里）。
    let new_route = {
        let guard = guard.clone();
        warp::path(prefix)
            .and(warp::path("new"))
            .and(warp::path::end())
            .and(warp::get())
            .and(warp::query::<HashMap<String, String>>())
            .and(guard_filter(guard))
            .and_then(|query: HashMap<String, String>, g: Guard| async move {
                let kind = query.get("type").cloned();
                let created = super::run_blocking(move || g.create_json(kind.as_deref())).await;
                let reply: Box<dyn warp::Reply + Send> = match created {
                    Ok(value) => Box::new(warp::reply::json(&value)),
                    Err(_) => Box::new(StatusCode::INTERNAL_SERVER_ERROR),
                };
                Ok::<_, warp::Rejection>(reply)
            })
    };
    // 出图路由没有方法过滤（`POST /captcha/verify` 也会被 `path::param` 吃掉），必须排在它前面。
    let verify_route = {
        let guard = guard.clone();
        warp::path(prefix)
            .and(warp::path("verify"))
            .and(warp::path::end())
            .and(warp::post())
            // warp 的 `body::json()` 默认无大小上限：先按 Content-Length 卡 413。
            .and(warp::body::content_length_limit(MAX_VERIFY_BODY as u64))
            .and(warp::addr::remote())
            .and(warp::header::optional::<String>("x-forwarded-for"))
            .and(warp::body::json())
            .and(guard_filter(guard))
            .and_then(
                |remote: Option<SocketAddr>, xff: Option<String>, body: VerifyRequest, g: Guard| async move {
                    // `verify_reply` 本身不会失败：包成 `Ok` 只是为了复用 `run_blocking` 的 JoinError 兜底。
                    let reply = super::run_blocking(move || Ok(verify_reply(&g, remote, xff, body))).await;
                    let reply: Box<dyn warp::Reply + Send> = match reply {
                        Ok(reply) => reply,
                        Err(_) => Box::new(StatusCode::INTERNAL_SERVER_ERROR),
                    };
                    Ok::<_, warp::Rejection>(reply)
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
        .and_then(|key: String, g: Guard| async move {
            let reply = super::run_blocking(move || Ok(image_reply(&g, &key))).await;
            let reply: Box<dyn warp::Reply + Send> = match reply {
                Ok(reply) => reply,
                Err(_) => Box::new(StatusCode::INTERNAL_SERVER_ERROR),
            };
            Ok::<_, warp::Rejection>(reply)
        });
    new_route.or(verify_route).or(image_route)
}
