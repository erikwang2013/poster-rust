//! axum 集成示例：起一个带验证码路由的小服务。
//!
//! ```bash
//! cargo run --example axum_captcha --features axum
//! # GET http://127.0.0.1:8090/captcha/new?type=slider   生成
//! # GET http://127.0.0.1:8090/captcha/{key}             取图（PNG）
//! # POST http://127.0.0.1:8090/captcha/verify           校验 {"key":…, "answer":…}
//! ```

use std::sync::Arc;

use axum::Router;
use axum::routing::get;
use poster::captcha::CaptchaManager;
use poster::integrations::axum::{GuardState, captcha_routes};
use poster::Guard;

#[derive(Clone)]
struct AppState {
    captcha: Guard,
}

impl GuardState for AppState {
    fn guard(&self) -> &Guard {
        &self.captcha
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 接线期构造（存储探针，失败即退出）
    let guard = Guard::from_manager(Arc::new(CaptchaManager::new()?))?;

    let app = Router::new()
        .merge(captcha_routes::<AppState>())
        .route("/", get(|| async { "poster-rust 验证码示例: GET /captcha/new?type=slider" }))
        .with_state(AppState { captcha: guard });

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8090").await?;
    println!("listening on http://127.0.0.1:8090/captcha/new");
    axum::serve(listener, app).await?;
    Ok(())
}
