//! Rocket 集成：`Guard` 请求守卫 + `GET {path}/{key} → image/png` 路由。
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

use rocket::http::{ContentType, Status};
use rocket::request::{FromRequest, Outcome};
use rocket::response::{Responder, Response};
use rocket::serde::json::Json;
use rocket::{Request, Route};

use crate::guard::Guard;

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

/// 注册两条路由：`GET /captcha/new`、`GET /captcha/<key>`（静态段优先于动态段）。
pub fn routes() -> Vec<Route> {
    rocket::routes![captcha_new, captcha_image]
}
