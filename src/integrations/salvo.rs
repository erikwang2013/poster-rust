//! Salvo 集成：自带状态的 `Handler`（无需 salvo 的 `affix-state` 等额外 feature），
//! 提供 `GET {path}/{key} → image/png` 与 `GET {path}/new` 两条路由。
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

use salvo::{Depot, FlowCtrl, Request, Response, Router};

use crate::guard::Guard;

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

/// 路由：`/captcha/{key}` 与 `/captcha/new`。
pub fn routes(guard: Guard) -> Router {
    routes_at(guard, "captcha")
}

/// 同 [`routes`]，自定义路径前缀。
pub fn routes_at(guard: Guard, prefix: &str) -> Router {
    Router::with_path(format!("{prefix}/{{key}}"))
        .get(CaptchaImageHandler {
            guard: guard.clone(),
        })
        .push(Router::with_path(format!("{prefix}/new")).get(CaptchaNewHandler { guard }))
}
