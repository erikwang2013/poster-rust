//! warp 集成端到端：`POST /captcha/verify` 按请求身份分桶的限流、错误答案拒绝、非法 JSON。

use std::sync::Arc;

use serde_json::json;
use warp::Filter;

use poster::captcha::CaptchaManager;
use poster::integrations::warp::routes;
use poster::storage::{MemoryStorage, Storage};
use poster::{Guard, PosterConfig};

/// `rate_limit.max = 1` 的守卫：存储句柄一并返回，供测试读出滑块答案
/// （HTTP 不回传答案，只有正确/错误两种可观察结果）。
fn rate_limited_guard() -> (Guard, Arc<MemoryStorage>) {
    let storage = Arc::new(MemoryStorage::new());
    let mut config = PosterConfig::default();
    config.captcha.rate_limit.max = 1;
    config.captcha.rate_limit.window_secs = 60;
    let manager = Arc::new(CaptchaManager::with_config_and_storage(
        Arc::new(config),
        storage.clone(),
    ));
    (Guard::from_manager(manager).unwrap(), storage)
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

/// 带 `X-Forwarded-For` 的校验请求，返回 `pass`（状态码不是 200 直接失败）。
async fn post_verify<F>(filter: &F, key: &str, x: f32, xff: &str) -> bool
where
    F: Filter + Sync + 'static,
    F::Extract: warp::Reply,
{
    let resp = warp::test::request()
        .method("POST")
        .path("/captcha/verify")
        .header("x-forwarded-for", xff)
        .json(&json!({ "key": key, "answer": { "Slider": x } }))
        .reply(filter)
        .await;
    assert_eq!(resp.status().as_u16(), 200, "校验端点应返回 200");
    serde_json::from_slice::<serde_json::Value>(resp.body()).unwrap()["pass"]
        .as_bool()
        .unwrap()
}

/// 限流身份按请求派生（XFF 第一段），而不是所有请求共用常量 `"default"` 桶：
/// 同身份第二个正确校验因超限被拒，换身份后仍放行。
#[tokio::test]
async fn verify_rate_limit_buckets_by_request_identity() {
    let (guard, storage) = rate_limited_guard();
    let filter = routes(guard.clone());

    // 身份 A 第一次：答案正确且配额充足 → 放行（顺带用满本窗口的 1 次配额）
    let (key, x) = slider_key_with_answer(&guard, &storage);
    assert!(post_verify(&filter, &key, x, "203.0.113.7").await, "首次校验应通过");

    // 身份 A 第二次：答案仍然正确，但配额已用尽 → 拒绝
    let (key, x) = slider_key_with_answer(&guard, &storage);
    assert!(
        !post_verify(&filter, &key, x, "203.0.113.7").await,
        "同身份超限后即使答案正确也必须拒绝"
    );

    // 身份 B：换一个 XFF → 独立计数桶，仍放行（常量 "default" 的旧实现会在这里误杀）
    let (key, x) = slider_key_with_answer(&guard, &storage);
    assert!(
        post_verify(&filter, &key, x, "198.51.100.9").await,
        "不同身份应各自独立成桶"
    );
}

/// 答案错误 → `pass=false`。
#[tokio::test]
async fn wrong_answer_is_rejected() {
    let (guard, storage) = rate_limited_guard();
    let filter = routes(guard.clone());
    let (key, x) = slider_key_with_answer(&guard, &storage);
    assert!(!post_verify(&filter, &key, x + 60.0, "203.0.113.7").await, "错误答案必须不通过");
}

/// 非法 JSON → 400（warp 的 `body::json` 拒绝）。
#[tokio::test]
async fn malformed_json_is_rejected_with_400() {
    let (guard, _storage) = rate_limited_guard();
    let filter = routes(guard);
    let resp = warp::test::request()
        .method("POST")
        .path("/captcha/verify")
        .header("content-type", "application/json")
        .body("{ not json")
        .reply(&filter)
        .await;
    assert_eq!(resp.status().as_u16(), 400, "非法 JSON 应返回 400");
}
