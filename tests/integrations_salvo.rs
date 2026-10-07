//! salvo 集成端到端：`POST /captcha/verify` 的身份分桶限流、错误答案、非法 JSON，
//! 外加 `GET /captcha/new` 的路由可达性回归。
//!
//! 用 `salvo::test::TestClient`（需 salvo 的 `test` feature），覆盖类型检查看不到的运行时分支：
//! `parse_json` + `Writer` 的错误渲染路径、`req.header::<String>` 的取值、
//! `remote_addr().ip()` 在非 TCP 连接上的 `None` 回退、以及子路由的**相对**段拼接。

use std::sync::Arc;

use salvo::Router;
use salvo::http::StatusCode;
use salvo::test::{ResponseExt, TestClient};
use serde_json::json;

use poster::captcha::CaptchaManager;
use poster::integrations::salvo::routes;
use poster::storage::{MemoryStorage, Storage};
use poster::{Guard, PosterConfig};

/// `rate_limit.max = 1` 的路由表：守卫与存储句柄一并返回，供测试读出滑块答案
/// （HTTP 不回传答案，只有正确/错误两种可观察结果）。
///
/// `Router` 不是 `Clone`，用 `Arc<Router>` 复用（salvo 为它实现了 `SendTarget`）。
fn rate_limited_router() -> (Arc<Router>, Guard, Arc<MemoryStorage>) {
    let storage = Arc::new(MemoryStorage::new());
    let mut config = PosterConfig::default();
    config.captcha.rate_limit.max = 1;
    config.captcha.rate_limit.window_secs = 60;
    let manager = Arc::new(CaptchaManager::with_config_and_storage(
        Arc::new(config),
        storage.clone(),
    ));
    let guard = Guard::from_manager(manager).unwrap();
    (Arc::new(routes(guard.clone())), guard, storage)
}

/// 生成一张滑块验证码，并从存储里读出目标 x 作为正确答案。
fn slider_key_with_answer(guard: &Guard, storage: &MemoryStorage) -> (String, f32) {
    let result = guard.create(Some("slider")).unwrap().generate().unwrap();
    let payload = storage
        .get(&result.key)
        .unwrap()
        .expect("载荷应已落存储")
        .json()
        .unwrap();
    let x = payload["x"].as_f64().expect("滑块载荷应有 x") as f32;
    (result.key, x)
}

/// 校验请求：断言 200 与 `{"pass": bool}` 的完整形状。
async fn post_verify(router: &Arc<Router>, key: &str, x: f32, xff: Option<&str>, expect_pass: bool) {
    let mut req = TestClient::post("http://127.0.0.1/captcha/verify")
        .json(&json!({ "key": key, "answer": { "Slider": x } }));
    if let Some(xff) = xff {
        req = req.add_header("x-forwarded-for", xff, true);
    }
    let mut resp = req.send(router.clone()).await;
    assert_eq!(resp.status_code, Some(StatusCode::OK), "校验端点应返回 200");
    let body: serde_json::Value = resp.take_json().await.unwrap();
    assert_eq!(body, json!({ "pass": expect_pass }));
}

/// 限流身份按请求派生（XFF 第一段），而不是所有请求共用常量 `"default"` 桶：
/// 同身份第二个正确校验因超限被拒，换身份后仍放行。
#[tokio::test]
async fn verify_rate_limit_buckets_by_request_identity() {
    let (router, guard, storage) = rate_limited_router();

    // 身份 A 第一次：答案正确且配额充足 → 放行（顺带用满本窗口的 1 次配额）
    let (key, x) = slider_key_with_answer(&guard, &storage);
    post_verify(&router, &key, x, Some("203.0.113.7"), true).await;

    // 身份 A 第二次：答案仍然正确，但配额已用尽 → 拒绝
    let (key, x) = slider_key_with_answer(&guard, &storage);
    post_verify(&router, &key, x, Some("203.0.113.7"), false).await;

    // 身份 B：换一个 XFF → 独立计数桶，仍放行（常量 "default" 的旧实现会在这里误杀）
    let (key, x) = slider_key_with_answer(&guard, &storage);
    post_verify(&router, &key, x, Some("198.51.100.9"), true).await;
}

/// 缺 `X-Forwarded-For`：不 panic，退化成同一个兜底桶
/// （测试客户端的 `remote_addr` 是 `SocketAddr::Unknown`，取不到对端 IP → `"unknown"`）。
#[tokio::test]
async fn missing_xff_falls_back_to_a_shared_bucket() {
    let (router, guard, storage) = rate_limited_router();

    let (key, x) = slider_key_with_answer(&guard, &storage);
    post_verify(&router, &key, x, None, true).await;

    let (key, x) = slider_key_with_answer(&guard, &storage);
    post_verify(&router, &key, x, None, false).await;
}

/// 答案错误 → `pass=false`。
#[tokio::test]
async fn wrong_answer_is_rejected() {
    let (router, guard, storage) = rate_limited_router();
    let (key, x) = slider_key_with_answer(&guard, &storage);
    post_verify(&router, &key, x + 60.0, Some("203.0.113.7"), false).await;
}

/// 非法 JSON → 400（`parse_json` 失败交给 salvo 的 `Writer` 渲染，不是 500）。
#[tokio::test]
async fn malformed_json_is_rejected_with_400() {
    let (router, _guard, _storage) = rate_limited_router();
    let resp = TestClient::post("http://127.0.0.1/captcha/verify")
        .raw_json("{ not json")
        .send(router.clone())
        .await;
    assert_eq!(resp.status_code, Some(StatusCode::BAD_REQUEST), "非法 JSON 应返回 400");
}

/// 路由回归：`GET /captcha/new` 直接可达——salvo 的子路由按相对段拼接，
/// 把 `{prefix}/new` 嵌在 `{prefix}/{key}` 之下会拼成 `/{prefix}/{key}/{prefix}/new`（永不匹配）。
#[tokio::test]
async fn new_endpoint_is_routable_at_flat_path() {
    let (router, _guard, _storage) = rate_limited_router();
    let mut resp = TestClient::get("http://127.0.0.1/captcha/new")
        .send(router.clone())
        .await;
    assert_eq!(resp.status_code, Some(StatusCode::OK), "GET /captcha/new 应可达");
    let body: serde_json::Value = resp.take_json().await.unwrap();
    assert!(body["key"].is_string(), "生成端点应返回 key");
}
