//! rocket 集成端到端：`POST /captcha/verify` 的身份分桶限流、错误答案、非法 JSON。
//!
//! 用 rocket 自带的 `local::blocking::Client`（无需额外 feature），覆盖类型检查看不到的运行时分支：
//! 自定义 `ForwardedFor` 请求守卫的取值、内置 `Option<IpAddr>` 守卫（`Request::client_ip`）、
//! `Json` 数据守卫的错误码。

use std::net::SocketAddr;
use std::sync::Arc;

use rocket::http::{ContentType, Header, Status};
use rocket::local::blocking::Client;
use serde_json::json;

use poster::captcha::CaptchaManager;
use poster::integrations::rocket::routes;
use poster::storage::{MemoryStorage, Storage};
use poster::{Guard, PosterConfig};

/// `rate_limit.max = 1` 的应用：守卫与存储句柄一并返回，供测试读出滑块答案
/// （HTTP 不回传答案，只有正确/错误两种可观察结果）。
fn rate_limited_client() -> (Client, Guard, Arc<MemoryStorage>) {
    let storage = Arc::new(MemoryStorage::new());
    let mut config = PosterConfig::default();
    config.captcha.rate_limit.max = 1;
    config.captcha.rate_limit.window_secs = 60;
    let manager = Arc::new(CaptchaManager::with_config_and_storage(
        Arc::new(config),
        storage.clone(),
    ));
    let guard = Guard::from_manager(manager).unwrap();
    let client = Client::untracked(rocket::build().manage(guard.clone()).mount("/", routes())).unwrap();
    (client, guard, storage)
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

fn verify_request<'c>(
    client: &'c Client,
    key: &str,
    x: f32,
    xff: Option<&str>,
) -> rocket::local::blocking::LocalRequest<'c> {
    let body = json!({ "key": key, "answer": { "Slider": x } }).to_string();
    let req = client.post("/captcha/verify").header(ContentType::JSON).body(body);
    match xff {
        Some(xff) => req.header(Header::new("x-forwarded-for", xff.to_owned())),
        None => req,
    }
}

/// 带 `X-Forwarded-For` 的校验请求，返回 `pass`（状态码不是 200 直接失败）。
fn post_verify(client: &Client, key: &str, x: f32, xff: Option<&str>) -> bool {
    let resp = verify_request(client, key, x, xff).dispatch();
    assert_eq!(resp.status(), Status::Ok, "校验端点应返回 200");
    resp.into_json::<serde_json::Value>().unwrap()["pass"]
        .as_bool()
        .unwrap()
}

/// 限流身份按请求派生（XFF 第一段），而不是所有请求共用常量 `"default"` 桶：
/// 同身份第二个正确校验因超限被拒，换身份后仍放行。
#[test]
fn verify_rate_limit_buckets_by_request_identity() {
    let (client, guard, storage) = rate_limited_client();

    // 身份 A 第一次：答案正确且配额充足 → 放行（顺带用满本窗口的 1 次配额）
    let (key, x) = slider_key_with_answer(&guard, &storage);
    assert!(post_verify(&client, &key, x, Some("203.0.113.7")), "首次校验应通过");

    // 身份 A 第二次：答案仍然正确，但配额已用尽 → 拒绝
    let (key, x) = slider_key_with_answer(&guard, &storage);
    assert!(
        !post_verify(&client, &key, x, Some("203.0.113.7")),
        "同身份超限后即使答案正确也必须拒绝"
    );

    // 身份 B：换一个 XFF → 独立计数桶，仍放行（常量 "default" 的旧实现会在这里误杀）
    let (key, x) = slider_key_with_answer(&guard, &storage);
    assert!(
        post_verify(&client, &key, x, Some("198.51.100.9")),
        "不同身份应各自独立成桶"
    );
}

/// 不带 XFF、指定对端地址的校验请求，返回 `pass`（对端 IP 由 `Request::client_ip` 取，
/// 本地客户端默认没有对端，需要显式 `remote()` 伪造）。
fn post_verify_from(client: &Client, key: &str, x: f32, remote: SocketAddr) -> bool {
    let resp = verify_request(client, key, x, None).remote(remote).dispatch();
    assert_eq!(resp.status(), Status::Ok, "校验端点应返回 200");
    resp.into_json::<serde_json::Value>().unwrap()["pass"]
        .as_bool()
        .unwrap()
}

/// 无 `X-Forwarded-For` 时按**对端地址**分桶：同对端第二次超限被拒，换对端仍放行。
/// （poem / salvo 的测试客户端没有设置对端地址的入口，这条分支只有 rocket 能端到端跑到。）
#[test]
fn peer_address_buckets_when_xff_absent() {
    let (client, guard, storage) = rate_limited_client();
    let a = SocketAddr::from(([203, 0, 113, 7], 8080));
    let b = SocketAddr::from(([198, 51, 100, 9], 8080));

    let (key, x) = slider_key_with_answer(&guard, &storage);
    assert!(post_verify_from(&client, &key, x, a), "对端 A 首次校验应通过");

    let (key, x) = slider_key_with_answer(&guard, &storage);
    assert!(!post_verify_from(&client, &key, x, a), "同对端超限后即使答案正确也必须拒绝");

    let (key, x) = slider_key_with_answer(&guard, &storage);
    assert!(post_verify_from(&client, &key, x, b), "不同对端应各自独立成桶");
}

/// 缺 `X-Forwarded-For`：不 panic，退化成同一个兜底桶（`client_identity` 回退到对端 IP）。
#[test]
fn missing_xff_falls_back_to_a_shared_bucket() {
    let (client, guard, storage) = rate_limited_client();

    let (key, x) = slider_key_with_answer(&guard, &storage);
    assert!(post_verify(&client, &key, x, None), "缺 XFF 首次校验应通过");

    let (key, x) = slider_key_with_answer(&guard, &storage);
    assert!(
        !post_verify(&client, &key, x, None),
        "缺 XFF 的请求应共用同一个桶，第二次超限被拒"
    );
}

/// 答案错误 → `pass=false`。
#[test]
fn wrong_answer_is_rejected() {
    let (client, guard, storage) = rate_limited_client();
    let (key, x) = slider_key_with_answer(&guard, &storage);
    assert!(
        !post_verify(&client, &key, x + 60.0, Some("203.0.113.7")),
        "错误答案必须不通过"
    );
}

/// 非法 JSON → 400（`Json` 数据守卫的语法错误码，另有 422 字段不符 / 413 超限）。
#[test]
fn malformed_json_is_rejected_with_400() {
    let (client, _guard, _storage) = rate_limited_client();
    let resp = client
        .post("/captcha/verify")
        .header(ContentType::JSON)
        .body("{ not json")
        .dispatch();
    assert_eq!(resp.status(), Status::BadRequest, "非法 JSON 应返回 400");
}
