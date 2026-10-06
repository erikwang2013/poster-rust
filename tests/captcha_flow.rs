//! 端到端流程：`CaptchaManager` 生成 → 从存储读回内部答案 → 校验。
//!
//! 覆盖三种类型、容差边界、尝试次数、TTL、跨 key 限流、随机类型与结果 JSON 键名。

use std::sync::Arc;

use poster::PosterConfig;
use poster::captcha::{Answer, CaptchaManager, CaptchaResult};
use poster::storage::{MemoryStorage, Storage};

/// 管理器 + 可读句柄（测试要直接从存储里取回内部答案）。
fn manager_with(config: PosterConfig) -> (CaptchaManager, Arc<MemoryStorage>) {
    let storage = Arc::new(MemoryStorage::new());
    let manager = CaptchaManager::with_config_and_storage(Arc::new(config), storage.clone());
    (manager, storage)
}

fn manager() -> (CaptchaManager, Arc<MemoryStorage>) {
    manager_with(PosterConfig::default())
}

/// 存储里的答案载荷（含 `type` / `attempts` / 各类型答案）。
fn payload(storage: &MemoryStorage, key: &str) -> serde_json::Value {
    storage
        .get(key)
        .expect("存储读取失败")
        .expect("key 不在存储里")
        .json()
        .expect("载荷不是 JSON")
}

/// 按载荷里的目标坐标构造点击答案（顺序即 `targets` 顺序）。
fn click_answer(payload: &serde_json::Value) -> Answer {
    let points = payload["targets"]
        .as_array()
        .expect("载荷缺 targets")
        .iter()
        .map(|t| {
            (
                t["x"].as_f64().expect("缺 x") as f32,
                t["y"].as_f64().expect("缺 y") as f32,
            )
        })
        .collect();
    Answer::Click(points)
}

fn slider_x(storage: &MemoryStorage, key: &str) -> f32 {
    payload(storage, key)["x"].as_f64().expect("缺 x") as f32
}

fn rotate_angle(storage: &MemoryStorage, key: &str) -> f32 {
    payload(storage, key)["angle"].as_f64().expect("缺 angle") as f32
}

/// 测试用临时目录（每个用例一个，避免并行互相踩）。
fn tmp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("poster-captcha-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时目录失败");
    dir
}

#[test]
fn click_generates_and_verifies_with_the_internal_answer() {
    let (manager, storage) = manager();
    let result = manager.create(Some("click")).unwrap().generate().unwrap();
    assert_eq!(result.captcha_type, "click");

    let payload = payload(&storage, &result.key);
    assert_eq!(payload["type"], "click");
    assert_eq!(payload["targets"].as_array().unwrap().len(), 3, "medium = 3 目标");
    assert_eq!(payload["attempts"], 0);

    assert!(manager.verify(&result.key, click_answer(&payload)).unwrap());
    // 一次性：通过后答案与图片都清掉
    assert!(storage.get(&result.key).unwrap().is_none());
    assert!(manager.image_bytes(&result.key).unwrap().is_none());
}

#[test]
fn rotate_generates_and_verifies_with_the_internal_answer() {
    let (manager, storage) = manager();
    let result = manager
        .create(Some("rotate"))
        .unwrap()
        .generate()
        .unwrap();
    assert_eq!(result.captcha_type, "rotate");
    assert_eq!(payload(&storage, &result.key)["type"], "rotate");

    let angle = rotate_angle(&storage, &result.key);
    assert!(manager.verify(&result.key, Answer::Rotate(angle)).unwrap());
}

#[test]
fn slider_generates_and_verifies_with_the_internal_answer() {
    let (manager, storage) = manager();
    let result = manager
        .create(Some("slider"))
        .unwrap()
        .generate()
        .unwrap();
    assert_eq!(result.captcha_type, "slider");

    let payload = payload(&storage, &result.key);
    assert_eq!(payload["type"], "slider");
    assert!(result.extra["puzzle_w"].as_u64().is_some());
    assert!(
        result.extra["puzzle"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,")
    );

    let x = payload["x"].as_f64().unwrap() as f32;
    assert!(manager.verify(&result.key, Answer::Slider(x)).unwrap());
}

#[test]
fn wrong_answer_fails_and_keeps_the_key() {
    let (manager, storage) = manager();
    let result = manager
        .create(Some("slider"))
        .unwrap()
        .generate()
        .unwrap();
    let x = slider_x(&storage, &result.key);

    assert!(!manager.verify(&result.key, Answer::Slider(x + 500.0)).unwrap());
    assert!(
        storage.get(&result.key).unwrap().is_some(),
        "失败后 key 应保留剩余次数"
    );
    assert!(manager.verify(&result.key, Answer::Slider(x)).unwrap());
}

#[test]
fn tolerance_boundaries() {
    let (manager, storage) = manager();

    // 点击 ±18px：距离正好等于容差算通过
    for (offset, expect) in [(18.0f32, true), (18.5, false)] {
        let result = manager.create(Some("click")).unwrap().generate().unwrap();
        let payload = payload(&storage, &result.key);
        let Answer::Click(mut points) = click_answer(&payload) else {
            unreachable!()
        };
        points[0].0 += offset;
        assert_eq!(
            manager.verify(&result.key, Answer::Click(points)).unwrap(),
            expect,
            "点击偏移 {offset}px"
        );
    }

    // 旋转 ±5°
    for (offset, expect) in [(5.0f32, true), (5.5, false)] {
        let result = manager
            .create(Some("rotate"))
            .unwrap()
            .generate()
            .unwrap();
        let angle = rotate_angle(&storage, &result.key);
        assert_eq!(
            manager
                .verify(&result.key, Answer::Rotate(angle + offset))
                .unwrap(),
            expect,
            "旋转偏移 {offset}°"
        );
    }

    // 滑块 ±4px
    for (offset, expect) in [(4.0f32, true), (4.5, false)] {
        let result = manager
            .create(Some("slider"))
            .unwrap()
            .generate()
            .unwrap();
        let x = slider_x(&storage, &result.key);
        assert_eq!(
            manager.verify(&result.key, Answer::Slider(x + offset)).unwrap(),
            expect,
            "滑块偏移 {offset}px"
        );
    }
}

#[test]
fn attempts_over_the_limit_drop_the_key() {
    let (manager, storage) = manager();
    let result = manager
        .create(Some("slider"))
        .unwrap()
        .generate()
        .unwrap();
    let x = slider_x(&storage, &result.key);

    for round in 1..=3 {
        assert!(
            !manager
                .verify(&result.key, Answer::Slider(x + 500.0))
                .unwrap(),
            "第 {round} 次错误答案"
        );
        assert!(
            storage.get(&result.key).unwrap().is_some(),
            "第 {round} 次后 key 仍在（上限 3）"
        );
    }

    // 第 4 次：先判超限，正确答案也拒绝，且 key（含图片）被清掉
    assert!(!manager.verify(&result.key, Answer::Slider(x)).unwrap());
    assert!(storage.get(&result.key).unwrap().is_none());
    assert!(manager.image_bytes(&result.key).unwrap().is_none());
}

#[test]
fn expired_captcha_is_gone() {
    let mut config = PosterConfig::default();
    config.captcha.ttl_secs = 1;
    let (manager, storage) = manager_with(config);

    let result = manager
        .create(Some("slider"))
        .unwrap()
        .generate()
        .unwrap();
    let x = slider_x(&storage, &result.key);
    assert!(manager.image_bytes(&result.key).unwrap().is_some());

    std::thread::sleep(std::time::Duration::from_millis(1200));

    assert!(storage.get(&result.key).unwrap().is_none(), "答案已过期");
    assert!(manager.image_bytes(&result.key).unwrap().is_none());
    assert!(!manager.verify(&result.key, Answer::Slider(x)).unwrap());
}

#[test]
fn rate_limit_is_cross_key_and_per_identity() {
    let mut config = PosterConfig::default();
    config.captcha.rate_limit.max = 3;
    config.captcha.rate_limit.window_secs = 60;
    let (manager, storage) = manager_with(config);

    // 每次换新 key 再猜：单 key 计数拦不住，跨 key 窗口限流才拦得住
    for round in 1..=3 {
        let result = manager
            .create(Some("slider"))
            .unwrap()
            .generate()
            .unwrap();
        let x = slider_x(&storage, &result.key);
        assert!(
            manager
                .verify_as(&result.key, Answer::Slider(x), "1.2.3.4")
                .unwrap(),
            "窗口内第 {round} 次应放行"
        );
    }

    let result = manager
        .create(Some("slider"))
        .unwrap()
        .generate()
        .unwrap();
    let x = slider_x(&storage, &result.key);
    assert!(
        !manager
            .verify_as(&result.key, Answer::Slider(x), "1.2.3.4")
            .unwrap(),
        "窗口内第 4 次应拒绝"
    );
    // 限流按身份隔离；被限流的那次没消耗尝试次数，换个身份仍能过
    assert!(
        manager
            .verify_as(&result.key, Answer::Slider(x), "5.6.7.8")
            .unwrap()
    );
}

#[test]
fn random_type_resolves_before_generating() {
    let (manager, _) = manager();
    let mut seen = std::collections::HashSet::new();
    for _ in 0..40 {
        // create(None) 取配置默认类型（默认就是 random）
        let result = manager.create(None).unwrap().generate().unwrap();
        assert!(
            ["click", "rotate", "slider"].contains(&result.captcha_type.as_str()),
            "实际类型应是具体类型，而不是 {}",
            result.captcha_type
        );
        seen.insert(result.captcha_type);
    }
    // 40 次三选一没抽到某个类型的概率约 1e-7
    assert_eq!(seen.len(), 3, "三种类型都应被抽到：{seen:?}");
}

#[test]
fn result_json_uses_php_key_names() {
    let (manager, _) = manager();
    let result = manager
        .create(Some("rotate"))
        .unwrap()
        .generate()
        .unwrap();

    let value = serde_json::to_value(&result).unwrap();
    assert_eq!(value["type"], "rotate");
    assert!(value["key"].is_string());
    assert!(value["extra"].is_object());
    assert!(value["image"].as_str().unwrap().starts_with("data:image/png;base64,"));
    assert!(value.get("url").is_none(), "无出图路由时 url 不出现");

    let mut routed = result.clone();
    routed.url = Some("/captcha/abc123".into());
    let value = serde_json::to_value(&routed).unwrap();
    assert_eq!(value["url"], "/captcha/abc123");
    let back: CaptchaResult = serde_json::from_value(value).unwrap();
    assert_eq!(back, routed);
}

#[test]
fn answer_serde_shape_matches_the_http_body() {
    // 框架适配器把请求体 `{ "key": …, "answer": … }` 原样反序列化成 Answer
    let cases = [
        (Answer::Slider(173.0), serde_json::json!({"Slider": 173.0})),
        (Answer::Rotate(185.0), serde_json::json!({"Rotate": 185.0})),
        (
            Answer::Click(vec![(120.0, 80.0)]),
            serde_json::json!({"Click": [[120.0, 80.0]]}),
        ),
        (
            Answer::SliderWithTrail {
                x: 173.0,
                trail: vec![(12.0, 3.0, 0.0)],
                duration_ms: 1200,
            },
            serde_json::json!({
                "SliderWithTrail": {"x": 173.0, "trail": [[12.0, 3.0, 0.0]], "duration_ms": 1200}
            }),
        ),
        (
            Answer::RotateWithTrail {
                angle: 90.0,
                trail: vec![(1.0, 2.0, 0.0), (3.0, 4.0, 100.0)],
                duration_ms: 900,
            },
            serde_json::json!({
                "RotateWithTrail": {"angle": 90.0, "trail": [[1.0, 2.0, 0.0], [3.0, 4.0, 100.0]], "duration_ms": 900}
            }),
        ),
    ];
    for (answer, expected) in cases {
        assert_eq!(serde_json::to_value(&answer).unwrap(), expected);
        assert_eq!(
            serde_json::from_value::<Answer>(expected.clone()).unwrap(),
            answer
        );
    }
}

#[test]
fn unknown_type_and_mismatched_answer_variant_are_rejected() {
    let (manager, _) = manager();
    assert!(manager.create(Some("nope")).is_err());
    // setter 参数非法：链式调用照常，错误在 generate() 报出
    assert!(
        manager
            .create(Some("click"))
            .unwrap()
            .set_difficulty("impossible")
            .generate()
            .is_err()
    );
    assert!(
        manager
            .create(Some("slider"))
            .unwrap()
            .set_shape("triangle")
            .generate()
            .is_err()
    );
    assert!(
        manager
            .create(Some("click"))
            .unwrap()
            .set_target_type("emoji")
            .generate()
            .is_err()
    );

    // 答案变体与载荷类型不符 → false（不算错误）
    let result = manager
        .create(Some("slider"))
        .unwrap()
        .generate()
        .unwrap();
    assert!(!manager.verify(&result.key, Answer::Rotate(0.0)).unwrap());
    assert!(
        !manager
            .verify(&result.key, Answer::Click(vec![(1.0, 1.0)]))
            .unwrap()
    );
    // 不存在的 key
    assert!(!manager.verify("no-such-key", Answer::Slider(0.0)).unwrap());
}

#[test]
fn image_bytes_are_persisted_for_the_image_endpoint() {
    use base64::Engine;

    let (manager, _) = manager();
    let result = manager.create(Some("click")).unwrap().generate().unwrap();
    let png = manager
        .image_bytes(&result.key)
        .unwrap()
        .expect("PNG 应已持久化（出图端点要用）");
    assert_eq!(&png[..4], b"\x89PNG");

    let b64 = result
        .image
        .strip_prefix("data:image/png;base64,")
        .expect("应是 data URI");
    assert_eq!(
        base64::engine::general_purpose::STANDARD.decode(b64).unwrap(),
        png,
        "data URI 与存储里应是同一份字节"
    );
}

#[test]
fn too_small_canvas_is_an_error() {
    let (manager, _) = manager();
    let dir = tmp_dir("small-canvas");
    let path = dir.join("small.png");
    let png = poster::ImageDriver::filled(100, 100, "#BBDEFB")
        .unwrap()
        .encode("png", None)
        .unwrap();
    std::fs::write(&path, png).unwrap();

    // 100×100 放不下 3 个点击目标（最小间距 36px + 边距）
    let error = manager
        .create(Some("click"))
        .unwrap()
        .set_background(&path)
        .generate()
        .expect_err("画布过小应报错");
    assert!(
        error.to_string().contains("100x100"),
        "错误信息应带上画布尺寸：{error}"
    );

    // 滑块同理：缺口会被钳到唯一位置
    assert!(
        manager
            .create(Some("slider"))
            .unwrap()
            .set_background(&path)
            .generate()
            .is_err()
    );

    // 显式背景不存在：直接报错，不静默回落
    assert!(
        manager
            .create(Some("click"))
            .unwrap()
            .set_background(dir.join("missing.png"))
            .generate()
            .is_err()
    );
}
