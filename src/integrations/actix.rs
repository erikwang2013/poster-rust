//! actix-web 集成：`Guard` 提取器 + `GET {path}/{key} → image/png` 出图路由 + 校验端点。
//!
//! ```no_run
//! use actix_web::{web, App, HttpServer};
//! use poster::{Guard, captcha::CaptchaManager, integrations::actix::configure};
//!
//! # fn main() -> std::io::Result<()> {
//! let guard = Guard::from_manager(std::sync::Arc::new(CaptchaManager::new().unwrap())).unwrap();
//! HttpServer::new(move || {
//!     App::new()
//!         .app_data(web::Data::new(guard.clone()))
//!         .configure(configure)          // /captcha/new、/captcha/verify、/captcha/{key}
//! })
//! # .bind(("127.0.0.1", 8080))?.run()
//! # }
//! ```

use std::future::{Ready, ready};

use actix_web::dev::Payload;
use actix_web::web::{self, ServiceConfig};
use actix_web::http::header;
use actix_web::{FromRequest, HttpRequest, HttpResponse, Responder};

use crate::captcha::Answer;
use crate::guard::{Guard, client_identity};

/// `Guard` 的 actix 提取器：从 `web::Data<Guard>` 取（未注册时 500）。
impl FromRequest for Guard {
    type Error = actix_web::Error;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        ready(
            req.app_data::<web::Data<Guard>>()
                .map(|data| data.get_ref().clone())
                .ok_or_else(|| {
                    actix_web::error::ErrorInternalServerError(
                        "poster Guard 未注册：请在 App 上 app_data(web::Data::new(guard.clone()))",
                    )
                }),
        )
    }
}

/// `GET {path}/{key}` → PNG（`Cache-Control: no-store`）。
pub async fn captcha_image(path: web::Path<String>, guard: Guard) -> impl Responder {
    match guard.image(&path.into_inner()) {
        Ok(Some(img)) => HttpResponse::Ok()
            .content_type(img.content_type)
            .insert_header((header::CACHE_CONTROL, img.cache_control))
            .body(img.bytes),
        Ok(None) => HttpResponse::NotFound().finish(),
        Err(_) => HttpResponse::InternalServerError().finish(),
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
pub async fn captcha_verify(
    req: HttpRequest,
    body: web::Json<VerifyRequest>,
    guard: Guard,
) -> impl Responder {
    let identity = client_identity(
        req.headers().get("x-forwarded-for").and_then(|value| value.to_str().ok()),
        req.peer_addr().map(|addr| addr.ip()),
    );
    match guard.verify_as(&body.key, body.answer.clone(), &identity) {
        Ok(pass) => HttpResponse::Ok().json(serde_json::json!({ "pass": pass })),
        Err(_) => HttpResponse::InternalServerError().finish(),
    }
}

/// 生成端点：`GET {path}/new?type=click` → `CaptchaResult` JSON。
pub async fn captcha_new(
    query: web::Query<std::collections::HashMap<String, String>>,
    guard: Guard,
) -> impl Responder {
    let kind = query.get("type").map(String::as_str);
    match guard.create_json(kind) {
        Ok(value) => HttpResponse::Ok().json(value),
        Err(_) => HttpResponse::InternalServerError().finish(),
    }
}

/// 注册三条路由（默认前缀 `/captcha`，与 PHP 版 `captcha.route.path` 对应）。
pub fn configure(cfg: &mut ServiceConfig) {
    configure_at(cfg, "/captcha");
}

/// 同上，自定义路径前缀。
pub fn configure_at(cfg: &mut ServiceConfig, path: &str) {
    cfg.route(&format!("{path}/new"), web::get().to(captcha_new));
    cfg.route(&format!("{path}/verify"), web::post().to(captcha_verify));
    cfg.route(&format!("{path}/{{key}}"), web::get().to(captcha_image));
}
