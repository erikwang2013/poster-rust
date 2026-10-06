//! axum 集成端到端：生成 → 出图 → 校验（错误答案）→ 404。

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use poster::captcha::CaptchaManager;
use poster::integrations::axum::{GuardState, captcha_routes};
use poster::Guard;
use tower::ServiceExt;

#[derive(Clone)]
struct AppState {
    captcha: Guard,
}

impl GuardState for AppState {
    fn guard(&self) -> &Guard {
        &self.captcha
    }
}

fn app() -> Router {
    let guard = Guard::from_manager(Arc::new(CaptchaManager::new().unwrap())).unwrap();
    Router::new()
        .merge(captcha_routes::<AppState>())
        .with_state(AppState { captcha: guard })
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), 4 * 1024 * 1024).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn generate_serve_and_verify_roundtrip() {
    let app = app();

    // 1. 生成
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/captcha/new?type=slider")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    let key = json["key"].as_str().expect("生成结果应有 key").to_string();
    assert_eq!(json["type"], "slider", "载荷类型键与 PHP 对齐为 type");
    assert!(json["image"].as_str().unwrap().starts_with("data:image/png;base64,"));

    // 2. 出图：GET /captcha/{key} → PNG + no-store
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/captcha/{key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()["content-type"], "image/png");
    assert_eq!(resp.headers()["cache-control"], "no-store");
    let bytes = axum::body::to_bytes(resp.into_body(), 4 * 1024 * 1024).await.unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "应是 PNG 魔数");

    // 3. 校验：不存在的 key → pass=false
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/captcha/verify")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "key": "nope", "answer": { "Slider": 1.0 } }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["pass"], false, "未知 key 必须校验失败");

    // 4. 不存在的 key 出图 → 404
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/captcha/nope")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
