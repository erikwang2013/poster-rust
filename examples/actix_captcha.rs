//! actix-web 集成示例：起一个带验证码路由的小服务。
//!
//! ```bash
//! cargo run --example actix_captcha --features actix
//! ```

use std::sync::Arc;

use actix_web::{App, HttpServer, web};
use poster::captcha::CaptchaManager;
use poster::integrations::actix::configure;
use poster::Guard;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let guard = Guard::from_manager(Arc::new(CaptchaManager::new().expect("captcha 初始化")))
        .expect("captcha 接线");

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(guard.clone()))
            .configure(configure) // /captcha/new、/captcha/verify、/captcha/{key}
    })
    .bind(("127.0.0.1", 8091))?
    .run()
    .await
}
