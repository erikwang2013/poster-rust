//! poem 集成端到端：`POST /captcha/verify` 的身份分桶限流、错误答案、非法 JSON。
//!
//! 用 `poem::test::TestClient`（需 poem 的 `test` feature），覆盖类型检查看不到的运行时分支：
//! `Guard` 提取器从 `Route::data` 取值、手写 body 读取（含 2 MiB 上限）与 `&Request` 的取参顺序、
//! `RemoteAddr` 不是 TCP 连接时的回退路径。

use std::sync::Arc;

use poem::http::StatusCode;
use poem::test::TestClient;
use poem::{Endpoint, EndpointExt, Route};
use serde_json::json;

use poster::captcha::CaptchaManager;
use poster::integrations::poem::routes;
use poster::storage::{MemoryStorage, Storage};
use poster::{Guard, PosterConfig};

/// `rate_limit.max = 1` 的应用：守卫与存储句柄一并返回，供测试读出滑块答案
/// （HTTP 不回传答案，只有正确/错误两种可观察结果）。
///
/// `trust_forwarded_for` 决定是否采信 `X-Forwarded-For` 作为限流身份：
/// 默认 `false`，测试里用 [`rate_limited_app_trusting_xff`] 显式开启。
fn rate_limited_app(trust_forwarded_for: bool) -> (impl Endpoint, Guard, Arc<MemoryStorage>) {
    let storage = Arc::new(MemoryStorage::new());
    let mut config = PosterConfig::default();
    config.captcha.rate_limit.max = 1;
    config.captcha.rate_limit.window_secs = 60;
    let manager = Arc::new(CaptchaManager::with_config_and_storage(
        Arc::new(config),
        storage.clone(),
    ));
    let guard = Guard::from_manager(manager)
        .unwrap()
        .with_trust_forwarded_for(trust_forwarded_for);
    (
        Route::new().nest("/", routes()).data(guard.clone()),
        guard,
        storage,
    )
}

/// 开启 XFF 信任的应用（模拟「可信代理已覆写 XFF」的部署）。
fn rate_limited_app_trusting_xff() -> (impl Endpoint, Guard, Arc<MemoryStorage>) {
    rate_limited_app(true)
}

/// 生成一张滑块验证码，并从存储里读出目标 x 作为正确答案。
fn slider_key_with_answer(guard: &Guard, storage: &MemoryStorage) -> (String, f32) {
    let result = guard.create(Some("slider")).unwrap().generate().unwrap();
    let payload = storage
        .get(&format!("captcha:{}", result.key))
        .unwrap()
        .expect("载荷应已落存储")
        .json()
        .unwrap();
    let x = payload["x"].as_f64().expect("滑块载荷应有 x") as f32;
    (result.key, x)
}

/// 校验请求：断言 200 与 `{"pass": bool}` 的完整形状（`assert_json` 是整体相等）。
async fn post_verify<E: Endpoint>(
    cli: &TestClient<E>,
    key: &str,
    x: f32,
    xff: Option<&str>,
    expect_pass: bool,
) {
    let mut req = cli.post("/captcha/verify");
    if let Some(xff) = xff {
        req = req.header("x-forwarded-for", xff);
    }
    let resp = req
        .body_json(&json!({ "key": key, "answer": { "Slider": x } }))
        .send()
        .await;
    resp.assert_status(StatusCode::OK);
    resp.assert_json(json!({ "pass": expect_pass })).await;
}

/// 限流身份按请求派生（显式开启信任后的 XFF 第一段），而不是所有请求共用常量 `"default"` 桶：
/// 同身份第二个正确校验因超限被拒，换身份后仍放行。
#[tokio::test]
async fn verify_rate_limit_buckets_by_request_identity() {
    let (app, guard, storage) = rate_limited_app_trusting_xff();
    let cli = TestClient::new(app);

    // 身份 A 第一次：答案正确且配额充足 → 放行（顺带用满本窗口的 1 次配额）
    let (key, x) = slider_key_with_answer(&guard, &storage);
    post_verify(&cli, &key, x, Some("203.0.113.7"), true).await;

    // 身份 A 第二次：答案仍然正确，但配额已用尽 → 拒绝
    let (key, x) = slider_key_with_answer(&guard, &storage);
    post_verify(&cli, &key, x, Some("203.0.113.7"), false).await;

    // 身份 B：换一个 XFF → 独立计数桶，仍放行（常量 "default" 的旧实现会在这里误杀）
    let (key, x) = slider_key_with_answer(&guard, &storage);
    post_verify(&cli, &key, x, Some("198.51.100.9"), true).await;
}

/// 缺 `X-Forwarded-For`：不 panic，退化成同一个兜底桶
/// （测试客户端的 `RemoteAddr` 是 `Addr::custom`，取不到对端 IP → `"unknown"`）。
#[tokio::test]
async fn missing_xff_falls_back_to_a_shared_bucket() {
    let (app, guard, storage) = rate_limited_app(false);
    let cli = TestClient::new(app);

    let (key, x) = slider_key_with_answer(&guard, &storage);
    post_verify(&cli, &key, x, None, true).await;

    let (key, x) = slider_key_with_answer(&guard, &storage);
    post_verify(&cli, &key, x, None, false).await;
}

/// 答案错误 → `pass=false`。
#[tokio::test]
async fn wrong_answer_is_rejected() {
    let (app, guard, storage) = rate_limited_app_trusting_xff();
    let cli = TestClient::new(app);
    let (key, x) = slider_key_with_answer(&guard, &storage);
    post_verify(&cli, &key, x + 60.0, Some("203.0.113.7"), false).await;
}

/// 默认（未开启信任）不采信 `X-Forwarded-For`：伪造头换不来新桶。
/// 测试客户端的 `RemoteAddr` 取不到对端 IP → 身份恒为 `"unknown"`，
/// 两个不同伪造 IP 仍打同一个桶，第二个超限被拒。
#[tokio::test]
async fn forged_xff_does_not_change_bucket_by_default() {
    let (app, guard, storage) = rate_limited_app(false);
    let cli = TestClient::new(app);

    let (key, x) = slider_key_with_answer(&guard, &storage);
    post_verify(&cli, &key, x, Some("203.0.113.7"), true).await;

    let (key, x) = slider_key_with_answer(&guard, &storage);
    post_verify(&cli, &key, x, Some("198.51.100.9"), false).await;
}

/// 超大请求体 → 413（不再用 `Json` 提取器全量读进内存，改为按上限读）。
#[tokio::test]
async fn oversized_verify_body_is_rejected_with_413() {
    let (app, _guard, _storage) = rate_limited_app(false);
    let cli = TestClient::new(app);
    let resp = cli
        .post("/captcha/verify")
        .content_type("application/json")
        .body("x".repeat(3 * 1024 * 1024))
        .send()
        .await;
    resp.assert_status(StatusCode::PAYLOAD_TOO_LARGE);
}

/// 非法 JSON → 400（handler 自己 `serde_json::from_slice` 的失败路径）。
#[tokio::test]
async fn malformed_json_is_rejected_with_400() {
    let (app, _guard, _storage) = rate_limited_app(false);
    let cli = TestClient::new(app);
    let resp = cli
        .post("/captcha/verify")
        .content_type("application/json")
        .body("{ not json")
        .send()
        .await;
    resp.assert_status(StatusCode::BAD_REQUEST);
}
