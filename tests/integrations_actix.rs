//! actix-web 集成端到端：生成（含 `?type=`）→ 出图 → 校验（正 / 错答案）→ 404；
//! 以及按请求身份分桶的限流与 `X-Forwarded-For` 开关。
//!
//! 用 `actix_web::test::init_service` + `TestRequest`（无需真起监听端口），覆盖类型检查
//! 看不到的运行时分支：`Guard` 从 `web::Data` 提取、`web::Query` 取 `?type=`、
//! `web::block` 往返、`peer_addr` / XFF 的身份派生。

use std::sync::Arc;

use actix_web::http::{StatusCode, header};
use actix_web::test::{TestRequest, call_service, init_service, read_body, read_body_json};
use actix_web::{App, web};
use serde_json::json;

use poster::captcha::CaptchaManager;
use poster::integrations::actix::configure;
use poster::storage::{MemoryStorage, Storage};
use poster::{Guard, PosterConfig};

/// `rate_limit.max = 1` 的守卫（`trust_forwarded_for` 由调用方决定）；
/// 存储句柄一并返回，供测试读出滑块答案（HTTP 不回传答案，只有正确/错误两种可观察结果）。
fn rate_limited_guard(trust_forwarded_for: bool) -> (Guard, Arc<MemoryStorage>) {
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
    (guard, storage)
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

/// 带（或不带）`X-Forwarded-For` 的校验请求，返回 `pass`（状态码不是 200 直接失败）。
///
/// 写成宏是因为 `init_service` 的返回类型是匿名 `impl Service`：函数签名里没法不引入
/// `actix-http` 直接依赖就把它写出来（本项目不允许为此新增依赖）。
macro_rules! post_verify {
    ($app:expr, $key:expr, $x:expr, $xff:expr) => {{
        let xff: Option<&str> = $xff;
        let mut req = TestRequest::post()
            .uri("/captcha/verify")
            .insert_header((header::CONTENT_TYPE, "application/json"));
        if let Some(xff) = xff {
            req = req.insert_header(("x-forwarded-for", xff));
        }
        let req = req
            .set_payload(json!({ "key": $key, "answer": { "Slider": $x } }).to_string())
            .to_request();
        let resp = call_service(&$app, req).await;
        assert_eq!(resp.status(), StatusCode::OK, "校验端点应返回 200");
        let body: serde_json::Value = read_body_json(resp).await;
        body["pass"].as_bool().expect("响应应有 pass 字段")
    }};
}

#[actix_web::test]
async fn generate_serve_and_verify_roundtrip() {
    let (guard, _storage) = rate_limited_guard(false);
    let app = init_service(
        App::new()
            .app_data(web::Data::new(guard))
            .configure(configure),
    )
    .await;

    // 1. 生成：`?type=` 被采信
    let resp = call_service(
        &app,
        TestRequest::get().uri("/captcha/new?type=slider").to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json: serde_json::Value = read_body_json(resp).await;
    assert_eq!(json["type"], "slider", "载荷类型键与 PHP 对齐为 type");
    let key = json["key"].as_str().expect("生成结果应有 key").to_string();

    // 2. 出图：GET /captcha/{key} → PNG + no-store
    let uri = format!("/captcha/{key}");
    let resp = call_service(&app, TestRequest::get().uri(&uri).to_request()).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers().get(header::CONTENT_TYPE).unwrap(), "image/png");
    assert_eq!(resp.headers().get(header::CACHE_CONTROL).unwrap(), "no-store");
    let bytes = read_body(resp).await;
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "应是 PNG 魔数");

    // 3. 校验：不存在的 key → pass=false
    assert!(!post_verify!(app, "nope", 1.0, None), "未知 key 必须校验失败");

    // 4. 不存在的 key 出图 → 404
    let resp = call_service(&app, TestRequest::get().uri("/captcha/nope").to_request()).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// 限流身份按请求派生（显式开启的 XFF 第一段），而不是所有请求共用常量 `"default"` 桶：
/// 同身份第二个正确校验因超限被拒，换身份后仍放行。
#[actix_web::test]
async fn verify_rate_limit_buckets_by_request_identity() {
    let (guard, storage) = rate_limited_guard(true);
    let app = init_service(
        App::new()
            .app_data(web::Data::new(guard.clone()))
            .configure(configure),
    )
    .await;

    // 身份 A 第一次：答案正确且配额充足 → 放行（顺带用满本窗口的 1 次配额）
    let (key, x) = slider_key_with_answer(&guard, &storage);
    assert!(post_verify!(app, &key, x, Some("203.0.113.7")), "首次校验应通过");

    // 身份 A 第二次：答案仍然正确，但配额已用尽 → 拒绝
    let (key, x) = slider_key_with_answer(&guard, &storage);
    assert!(
        !post_verify!(app, &key, x, Some("203.0.113.7")),
        "同身份超限后即使答案正确也必须拒绝"
    );

    // 身份 B：换一个 XFF → 独立计数桶，仍放行（常量 "default" 的旧实现会在这里误杀）
    let (key, x) = slider_key_with_answer(&guard, &storage);
    assert!(
        post_verify!(app, &key, x, Some("198.51.100.9")),
        "不同身份应各自独立成桶"
    );
}

/// 默认不采信 `X-Forwarded-For`：伪造头换不来新桶，两次请求仍打同一个桶（超限被拒）。
#[actix_web::test]
async fn forged_xff_does_not_change_bucket_by_default() {
    let (guard, storage) = rate_limited_guard(false);
    let app = init_service(
        App::new()
            .app_data(web::Data::new(guard.clone()))
            .configure(configure),
    )
    .await;

    let (key, x) = slider_key_with_answer(&guard, &storage);
    assert!(post_verify!(app, &key, x, Some("203.0.113.7")), "首次校验应通过");

    let (key, x) = slider_key_with_answer(&guard, &storage);
    assert!(
        !post_verify!(app, &key, x, Some("198.51.100.9")),
        "默认不采信 XFF：换个伪造 IP 也应共用同一个桶并超限被拒"
    );
}

/// 答案错误 → `pass=false`。
#[actix_web::test]
async fn wrong_answer_is_rejected() {
    let (guard, storage) = rate_limited_guard(true);
    let app = init_service(
        App::new()
            .app_data(web::Data::new(guard.clone()))
            .configure(configure),
    )
    .await;

    let (key, x) = slider_key_with_answer(&guard, &storage);
    assert!(
        !post_verify!(app, &key, x + 60.0, Some("203.0.113.7")),
        "错误答案必须不通过"
    );
}
