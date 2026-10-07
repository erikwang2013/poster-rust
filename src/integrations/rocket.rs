//! Rocket 集成：`Guard` 请求守卫 + `GET {path}/{key} → image/png` 路由 + 校验端点。
//!
//! ```no_run
//! # use std::sync::Arc;
//! use poster::{Guard, captcha::CaptchaManager};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let guard = Guard::from_manager(Arc::new(CaptchaManager::new()?))?;
//! let rocket = rocket::build()
//!     .manage(guard)
//!     .mount("/", poster::integrations::rocket::routes());
//! # Ok(())
//! # }
//! ```

use std::convert::Infallible;
use std::io::Cursor;
use std::net::IpAddr;

use rocket::http::{ContentType, Status};
use rocket::request::{FromRequest, Outcome};
use rocket::response::{Responder, Response};
use rocket::serde::json::Json;
use rocket::{Request, Route};

use crate::captcha::Answer;
use crate::guard::{Guard, client_identity};

/// PNG 出图响应体。
pub struct CaptchaImageResponse(pub Vec<u8>);

impl<'r> Responder<'r, 'static> for CaptchaImageResponse {
    fn respond_to(self, _req: &'r Request<'_>) -> rocket::response::Result<'static> {
        Response::build()
            .header(ContentType::PNG)
            .raw_header("Cache-Control", "no-store")
            .sized_body(self.0.len(), Cursor::new(self.0))
            .ok()
    }
}

/// `Guard` 的 Rocket 请求守卫：从托管状态取（`rocket.manage(guard)`）。
///
/// 未托管时 `Forward` 给后续路由（`Infallible` 是不可构造类型，不能作为 Error 值）。
#[rocket::async_trait]
impl<'r> FromRequest<'r> for Guard {
    type Error = Infallible;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match req.rocket().state::<Guard>() {
            Some(guard) => Outcome::Success(guard.clone()),
            None => Outcome::Forward(Status::InternalServerError),
        }
    }
}

/// `GET {path}/{key}` → PNG。
#[rocket::get("/captcha/<key>")]
pub fn captcha_image(guard: Guard, key: String) -> Result<CaptchaImageResponse, Status> {
    match guard.image(&key) {
        Ok(Some(img)) => Ok(CaptchaImageResponse(img.bytes)),
        Ok(None) => Err(Status::NotFound),
        Err(_) => Err(Status::InternalServerError),
    }
}

/// 生成端点：`GET {path}/new?type=click` → JSON。
#[rocket::get("/captcha/new?<kind>")]
pub fn captcha_new(guard: Guard, kind: Option<String>) -> Result<Json<serde_json::Value>, Status> {
    match guard.create_json(kind.as_deref()) {
        Ok(value) => Ok(Json(value)),
        Err(_) => Err(Status::InternalServerError),
    }
}

/// 校验请求体：`{ "key": "…", "answer": … }`。
#[derive(Debug, serde::Deserialize)]
pub struct VerifyRequest {
    pub key: String,
    pub answer: Answer,
}

/// `X-Forwarded-For` 请求守卫（缺失也算成功，由 [`client_identity`] 回退到对端 IP）。
///
/// Rocket 0.5 不允许把 `&Request` 直接当处理器参数（`&'r Request<'r>` 没有 `FromRequest` 实现），
/// 头信息只能通过自定义请求守卫取。
pub struct ForwardedFor(pub Option<String>);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for ForwardedFor {
    type Error = Infallible;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        Outcome::Success(Self(req.headers().get_one("x-forwarded-for").map(str::to_owned)))
    }
}

/// `POST {path}/verify` → `{"pass": true|false}`。
///
/// 限流身份按请求派生（[`client_identity`]：`X-Forwarded-For` 第一段 → 对端 IP → `"unknown"`）。
/// 对端 IP 用内置的 `Option<IpAddr>` 守卫（即 `Request::client_ip`：配了 `real_ip_header`
/// 时读该头，否则取连接地址；取不到则 `None`）。
/// 请求体 JSON 解析失败时由 [`Json`] 数据守卫返回 4xx（400 语法错 / 422 字段不符 / 413 超限）。
#[rocket::post("/captcha/verify", data = "<body>")]
pub fn captcha_verify(
    guard: Guard,
    xff: ForwardedFor,
    client_ip: Option<IpAddr>,
    body: Json<VerifyRequest>,
) -> Result<Json<serde_json::Value>, Status> {
    let identity = client_identity(xff.0.as_deref(), client_ip);
    let VerifyRequest { key, answer } = body.into_inner();
    match guard.verify_as(&key, answer, &identity) {
        Ok(pass) => Ok(Json(serde_json::json!({ "pass": pass }))),
        Err(_) => Err(Status::InternalServerError),
    }
}

/// 注册三条路由：`GET /captcha/new`、`POST /captcha/verify`、`GET /captcha/<key>`
/// （静态段优先于动态段）。
pub fn routes() -> Vec<Route> {
    rocket::routes![captcha_new, captcha_verify, captcha_image]
}
