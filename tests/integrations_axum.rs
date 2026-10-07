//! axum 集成端到端：生成 → 出图 → 校验（错误答案）→ 404；以及按请求身份分桶的限流。

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use poster::captcha::CaptchaManager;
use poster::integrations::axum::{GuardState, captcha_routes};
use poster::storage::{MemoryStorage, Storage};
use poster::{Guard, PosterConfig};
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

// ── 限流身份 ────────────────────────────────────────────────────────────────

/// `rate_limit.max = 1` 的应用：存储句柄一并返回，供测试读出滑块答案
/// （HTTP 不回传答案，只有正确/错误两种可观察结果）。
fn rate_limited_app() -> (Router, Arc<MemoryStorage>) {
    let storage = Arc::new(MemoryStorage::new());
    let mut config = PosterConfig::default();
    config.captcha.rate_limit.max = 1;
    config.captcha.rate_limit.window_secs = 60;
    let manager = Arc::new(CaptchaManager::with_config_and_storage(
        Arc::new(config),
        storage.clone(),
    ));
    let app = Router::new()
        .merge(captcha_routes::<AppState>())
        .with_state(AppState {
            captcha: Guard::from_manager(manager).unwrap(),
        });
    (app, storage)
}

/// 生成一张滑块验证码，并从存储里读出目标 x 作为正确答案。
async fn slider_key_with_answer(app: &Router, storage: &MemoryStorage) -> (String, f32) {
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
    let json = body_json(resp).await;
    let key = json["key"].as_str().expect("生成结果应有 key").to_string();
    let payload = storage
        .get(&key)
        .unwrap()
        .expect("载荷应已落存储")
        .json()
        .unwrap();
    let x = payload["x"].as_f64().expect("滑块载荷应有 x") as f32;
    (key, x)
}

/// 带 `X-Forwarded-For` 的校验请求，返回 `pass`。
async fn post_verify(app: &Router, key: &str, x: f32, xff: &str) -> bool {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/captcha/verify")
                .header("content-type", "application/json")
                .header("x-forwarded-for", xff)
                .body(Body::from(
                    serde_json::json!({ "key": key, "answer": { "Slider": x } }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    body_json(resp).await["pass"].as_bool().unwrap()
}

/// 限流身份按请求派生（XFF 第一段），而不是所有请求共用常量 `"default"` 桶：
/// 同身份第二个正确校验因超限被拒，换身份后仍放行。
#[tokio::test]
async fn verify_rate_limit_buckets_by_request_identity() {
    let (app, storage) = rate_limited_app();

    // 身份 A 第一次：答案正确且配额充足 → 放行（顺带用满本窗口的 1 次配额）
    let (key, x) = slider_key_with_answer(&app, &storage).await;
    assert!(post_verify(&app, &key, x, "203.0.113.7").await, "首次校验应通过");

    // 身份 A 第二次：答案仍然正确，但配额已用尽 → 拒绝
    let (key, x) = slider_key_with_answer(&app, &storage).await;
    assert!(
        !post_verify(&app, &key, x, "203.0.113.7").await,
        "同身份超限后即使答案正确也必须拒绝"
    );

    // 身份 B：换一个 XFF → 独立计数桶，仍放行（常量 "default" 的旧实现会在这里误杀）
    let (key, x) = slider_key_with_answer(&app, &storage).await;
    assert!(
        post_verify(&app, &key, x, "198.51.100.9").await,
        "不同身份应各自独立成桶"
    );
}
