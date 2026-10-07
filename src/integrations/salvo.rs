//! Salvo 集成：自带状态的 `Handler`（无需 salvo 的 `affix-state` 等额外 feature），
//! 提供 `GET {path}/{key} → image/png`、`GET {path}/new` 与 `POST {path}/verify` 三条路由。
//!
//! ```no_run
//! # use std::sync::Arc;
//! use poster::{Guard, captcha::CaptchaManager};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let guard = Guard::from_manager(Arc::new(CaptchaManager::new()?))?;
//! let router = poster::integrations::salvo::routes(guard);   // 也可 Router::new().push(…)
//! # Ok(())
//! # }
//! ```

use salvo::{Depot, FlowCtrl, Request, Response, Router, Writer};

use crate::captcha::Answer;
use crate::guard::{Guard, client_identity};

/// `GET {path}/{key}` → PNG。
pub struct CaptchaImageHandler {
    guard: Guard,
}

#[salvo::handler]
impl CaptchaImageHandler {
    async fn handle(
        &self,
        req: &mut Request,
        _depot: &mut Depot,
        res: &mut Response,
        _ctrl: &mut FlowCtrl,
    ) {
        let key: String = req.param("key").unwrap_or_default();
        match self.guard.image(&key) {
            Ok(Some(img)) => {
                res.headers_mut().insert(
                    "content-type",
                    salvo::http::HeaderValue::from_static("image/png"),
                );
                res.headers_mut().insert(
                    "cache-control",
                    salvo::http::HeaderValue::from_static("no-store"),
                );
                res.write_body(img.bytes).ok();
            }
            Ok(None) => {
                res.status_code(salvo::http::StatusCode::NOT_FOUND);
            }
            Err(_) => {
                res.status_code(salvo::http::StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    }
}

/// `GET {path}/new?type=click` → `CaptchaResult` JSON。
pub struct CaptchaNewHandler {
    guard: Guard,
}

#[salvo::handler]
impl CaptchaNewHandler {
    async fn handle(
        &self,
        req: &mut Request,
        _depot: &mut Depot,
        res: &mut Response,
        _ctrl: &mut FlowCtrl,
    ) {
        let kind: Option<String> = req.query("type");
        match self.guard.create_json(kind.as_deref()) {
            Ok(value) => {
                res.render(salvo::prelude::Json(value));
            }
            Err(_) => {
                res.status_code(salvo::http::StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
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
/// 请求体 JSON 解析失败时交给 salvo 自己的 [`Writer`]（400；超出大小上限 413）。
pub struct CaptchaVerifyHandler {
    guard: Guard,
}

#[salvo::handler]
impl CaptchaVerifyHandler {
    async fn handle(
        &self,
        req: &mut Request,
        depot: &mut Depot,
        res: &mut Response,
        _ctrl: &mut FlowCtrl,
    ) {
        let body = match req.parse_json::<VerifyRequest>().await {
            Ok(body) => body,
            Err(err) => {
                err.write(req, depot, res).await;
                return;
            }
        };
        let identity = client_identity(
            req.header::<String>("x-forwarded-for").as_deref(),
            req.remote_addr().ip(),
        );
        match self.guard.verify_as(&body.key, body.answer, &identity) {
            Ok(pass) => {
                res.render(salvo::prelude::Json(serde_json::json!({ "pass": pass })));
            }
            Err(_) => {
                res.status_code(salvo::http::StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    }
}

/// 路由：`/captcha/new`、`/captcha/verify` 与 `/captcha/{key}`。
pub fn routes(guard: Guard) -> Router {
    routes_at(guard, "captcha")
}

/// 同 [`routes`]，自定义路径前缀。
///
/// 三条路由是平级兄弟：salvo 的子路由路径按**相对**段拼接，
/// 把 `{prefix}/new` 挂在 `{prefix}/{key}` 之下会拼成 `/{prefix}/{key}/{prefix}/new`（永不匹配）。
/// 参数段放最后，让 `new` / `verify` 这两个具名段先匹配。
pub fn routes_at(guard: Guard, prefix: &str) -> Router {
    Router::new()
        .push(
            Router::with_path(format!("{prefix}/new"))
                .get(CaptchaNewHandler { guard: guard.clone() }),
        )
        .push(
            Router::with_path(format!("{prefix}/verify"))
                .post(CaptchaVerifyHandler { guard: guard.clone() }),
        )
        .push(
            Router::with_path(format!("{prefix}/{{key}}"))
                .get(CaptchaImageHandler { guard }),
        )
}
