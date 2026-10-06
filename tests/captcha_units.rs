//! 细粒度校验：行为轨迹判定、jigsaw 轮廓像素采样、builder 钳制、icon 缩略图。
//!
//! 像素断言都建在**纯色背景**上，避免程序化/内置背景的噪点干扰。

use std::sync::Arc;

use poster::PosterConfig;
use poster::ImageDriver;
use poster::captcha::{Answer, CaptchaManager};
use poster::storage::{MemoryStorage, Storage};

fn manager_with(config: PosterConfig) -> (CaptchaManager, Arc<MemoryStorage>) {
    let storage = Arc::new(MemoryStorage::new());
    let manager = CaptchaManager::with_config_and_storage(Arc::new(config), storage.clone());
    (manager, storage)
}

fn manager() -> (CaptchaManager, Arc<MemoryStorage>) {
    manager_with(PosterConfig::default())
}

fn payload(storage: &MemoryStorage, key: &str) -> serde_json::Value {
    storage
        .get(key)
        .expect("存储读取失败")
        .expect("key 不在存储里")
        .json()
        .expect("载荷不是 JSON")
}

fn tmp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("poster-captcha-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时目录失败");
    dir
}

/// data URI → 解码后的图片。
fn decode_uri(uri: &str) -> ImageDriver {
    use base64::Engine;
    let b64 = uri
        .strip_prefix("data:image/png;base64,")
        .expect("应是 PNG data URI");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .expect("base64 解码失败");
    ImageDriver::from_bytes(&bytes).expect("PNG 解码失败")
}

/// 写一张纯色背景并返回路径（画布尺寸 = 图片尺寸）。
fn solid_background(tag: &str, width: u32, height: u32) -> std::path::PathBuf {
    let path = tmp_dir(tag).join("bg.png");
    let png = ImageDriver::filled(width, height, "#FFFFFF")
        .unwrap()
        .encode("png", None)
        .unwrap();
    std::fs::write(&path, png).unwrap();
    path
}

/// 某像素是否为「被缺口/噪点涂暗」的像素（白底上的 #00000040 ≈ 191）。
fn is_dark(driver: &ImageDriver, x: u32, y: u32) -> bool {
    driver.image().get_pixel(x, y).0[0] < 200
}

/// 矩形区域内暗像素占比。
fn dark_ratio(driver: &ImageDriver, x: i32, y: i32, w: u32, h: u32) -> f32 {
    let mut dark = 0u32;
    for oy in 0..h {
        for ox in 0..w {
            if is_dark(driver, (x + ox as i32) as u32, (y + oy as i32) as u32) {
                dark += 1;
            }
        }
    }
    dark as f32 / (w * h) as f32
}

// ── 行为轨迹 ────────────────────────────────────────────────

/// 像人的轨迹：先停一下再冲过去（线性度 ≈ 0.77）。
const HUMAN_TRAIL: [(f64, f64, f64); 4] = [
    (0.0, 0.0, 0.0),
    (5.0, 0.0, 150.0),
    (6.0, 1.0, 300.0),
    (30.0, 0.0, 450.0),
];

/// 匀速直线：线性度 ≈ 1.0。
const ROBOT_TRAIL: [(f64, f64, f64); 4] = [
    (0.0, 0.0, 0.0),
    (10.0, 0.0, 100.0),
    (20.0, 0.0, 200.0),
    (30.0, 0.0, 300.0),
];

fn trajectory_config() -> PosterConfig {
    let mut config = PosterConfig::default();
    config.captcha.trajectory.enabled = true;
    config
}

/// 生成一张滑块，返回 (key, 正确的 x)。
fn slider(manager: &CaptchaManager, storage: &MemoryStorage) -> (String, f32) {
    let result = manager
        .create(Some("slider"))
        .unwrap()
        .generate()
        .unwrap();
    let x = payload(storage, &result.key)["x"].as_f64().unwrap() as f32;
    (result.key, x)
}

#[test]
fn slider_trajectory_verdicts() {
    let (manager, storage) = manager_with(trajectory_config());

    // 像人：通过
    let (key, x) = slider(&manager, &storage);
    assert!(
        manager
            .verify(
                &key,
                Answer::SliderWithTrail {
                    x,
                    trail: HUMAN_TRAIL.to_vec(),
                    duration_ms: 500,
                }
            )
            .unwrap()
    );

    // 匀速直线（机器人）：线性度超上限，拒绝
    let (key, x) = slider(&manager, &storage);
    assert!(
        !manager
            .verify(
                &key,
                Answer::SliderWithTrail {
                    x,
                    trail: ROBOT_TRAIL.to_vec(),
                    duration_ms: 300,
                }
            )
            .unwrap()
    );

    // 采样点不足（< 4）
    let (key, x) = slider(&manager, &storage);
    assert!(
        !manager
            .verify(
                &key,
                Answer::SliderWithTrail {
                    x,
                    trail: HUMAN_TRAIL[..3].to_vec(),
                    duration_ms: 500,
                }
            )
            .unwrap()
    );

    // 耗时越界：太快 / 太慢
    for duration in [100u64, 6000] {
        let (key, x) = slider(&manager, &storage);
        assert!(
            !manager
                .verify(
                    &key,
                    Answer::SliderWithTrail {
                        x,
                        trail: HUMAN_TRAIL.to_vec(),
                        duration_ms: duration,
                    }
                )
                .unwrap(),
            "耗时 {duration}ms 应越界"
        );
    }

    // 开启轨迹后，不带轨迹的裸数值一律拒绝（旧前端无法自证）
    let (key, x) = slider(&manager, &storage);
    assert!(!manager.verify(&key, Answer::Slider(x)).unwrap());
}

#[test]
fn rotate_trajectory_verdicts() {
    let (manager, storage) = manager_with(trajectory_config());

    let result = manager
        .create(Some("rotate"))
        .unwrap()
        .generate()
        .unwrap();
    let angle = payload(&storage, &result.key)["angle"].as_f64().unwrap() as f32;

    // 角度对 + 轨迹像人：通过
    assert!(
        manager
            .verify(
                &result.key,
                Answer::RotateWithTrail {
                    angle,
                    trail: HUMAN_TRAIL.to_vec(),
                    duration_ms: 500,
                }
            )
            .unwrap()
    );

    // 角度对但轨迹是直线：拒绝
    let result = manager
        .create(Some("rotate"))
        .unwrap()
        .generate()
        .unwrap();
    let angle = payload(&storage, &result.key)["angle"].as_f64().unwrap() as f32;
    assert!(
        !manager
            .verify(
                &result.key,
                Answer::RotateWithTrail {
                    angle,
                    trail: ROBOT_TRAIL.to_vec(),
                    duration_ms: 300,
                }
            )
            .unwrap()
    );

    // 裸角度：拒绝
    let result = manager
        .create(Some("rotate"))
        .unwrap()
        .generate()
        .unwrap();
    let angle = payload(&storage, &result.key)["angle"].as_f64().unwrap() as f32;
    assert!(!manager.verify(&result.key, Answer::Rotate(angle)).unwrap());
}

// ── 滑块像素采样 ────────────────────────────────────────────

#[test]
fn jigsaw_piece_is_masked_by_the_outline() {
    let (manager, storage) = manager();
    let background = solid_background("jigsaw", 400, 300);

    let result = manager
        .create(Some("slider"))
        .unwrap()
        .set_shape("jigsaw")
        .set_background(&background)
        .generate()
        .unwrap();

    let puzzle_w = result.extra["puzzle_w"].as_u64().unwrap() as i32;
    let puzzle_h = result.extra["puzzle_h"].as_u64().unwrap() as i32;
    // 凸出半径 = 短边/5（非 hard）
    let knob = puzzle_w.min(puzzle_h) / 5;

    let piece = decode_uri(result.extra["puzzle"].as_str().unwrap());
    assert_eq!(
        piece.size(),
        ((puzzle_w + 2 * knob) as u32, (puzzle_h + 2 * knob) as u32),
        "拼图块 PNG 是外扩后的外接矩形"
    );
    assert_eq!(piece.size(), (70, 70));

    let alpha = |x: i32, y: i32| piece.image().get_pixel(x as u32, y as u32).0[3];
    // 轮廓只在四条边的中点凸出/凹陷，外接矩形的角永远在轮廓外 → 透明
    assert_eq!(alpha(1, 1), 0, "外接矩形左上角应在轮廓外");
    assert_eq!(alpha(piece.width() as i32 - 2, piece.height() as i32 - 2), 0);
    // 本体内部（左上角内侧、正中心）永远在轮廓内 → 不透明
    assert_eq!(alpha(knob + 2, knob + 2), 255);
    assert_eq!(alpha(knob + puzzle_w / 2, knob + puzzle_h / 2), 255);
    // 上边凸起只占中点 ±knob 的一段，往左让开就是轮廓外
    assert_eq!(alpha(knob + 2, 2), 0, "上边凸起以外应透明");

    // 缺口与拼图块共用轮廓：背景上被涂暗的区域应覆盖大半个外接矩形。
    // 噪点用的是同一种颜色，无法逐像素区分，只能看覆盖率——最坏情况（四边全凹，
    // 凹陷部分不进缺口）本体仍有约 3/4 是暗的，而无缺口时噪点远达不到这个比例。
    let background_img = decode_uri(&result.image);
    let answer_x = payload(&storage, &result.key)["x"].as_f64().unwrap() as i32;
    let answer_y = payload(&storage, &result.key)["y"].as_f64().unwrap() as i32;
    let ratio = dark_ratio(
        &background_img,
        answer_x + knob,
        answer_y + knob,
        puzzle_w as u32,
        puzzle_h as u32,
    );
    assert!(ratio >= 0.7, "缺口应覆盖本体区域，实测 {ratio}");

    // 存的是拼图块 PNG 的左上角：外扩矩形在画布内
    assert!(answer_x >= 0 && answer_y >= 0);
    assert!((answer_x + puzzle_w + 2 * knob) <= background_img.width() as i32);
    assert!((answer_y + puzzle_h + 2 * knob) <= background_img.height() as i32);
}

#[test]
fn square_piece_is_a_plain_rectangle() {
    let (manager, storage) = manager();
    let background = solid_background("square", 400, 300);

    let result = manager
        .create(Some("slider"))
        .unwrap()
        .set_shape("square")
        .set_background(&background)
        .generate()
        .unwrap();

    let puzzle_w = result.extra["puzzle_w"].as_u64().unwrap() as i32;
    let puzzle_h = result.extra["puzzle_h"].as_u64().unwrap() as i32;

    let piece = decode_uri(result.extra["puzzle"].as_str().unwrap());
    assert_eq!(piece.size(), (puzzle_w as u32, puzzle_h as u32), "矩形无外扩");
    // 整块都是不透明的（背景为纯白，切下来的像素也是白的）
    for (x, y) in [(0, 0), (puzzle_w - 1, 0), (0, puzzle_h - 1), (puzzle_w / 2, puzzle_h / 2)] {
        let pixel = piece.image().get_pixel(x as u32, y as u32).0;
        assert_eq!(pixel[3], 255, "({x},{y}) 应不透明");
        assert_eq!(pixel[0], 255, "({x},{y}) 取自纯白背景");
    }

    // 缺口是整块矩形
    let x = payload(&storage, &result.key)["x"].as_f64().unwrap() as i32;
    let y = payload(&storage, &result.key)["y"].as_f64().unwrap() as i32;
    let ratio = dark_ratio(&decode_uri(&result.image), x, y, puzzle_w as u32, puzzle_h as u32);
    assert!(ratio >= 0.95, "矩形缺口应铺满整块，实测 {ratio}");
}

// ── Builder 行为 ────────────────────────────────────────────

#[test]
fn builders_clamp_like_php() {
    let (manager, storage) = manager();

    // set_size 钳到 60-400：500 → 400
    let result = manager
        .create(Some("rotate"))
        .unwrap()
        .set_size(500)
        .generate()
        .unwrap();
    assert_eq!(decode_uri(&result.image).size(), (400, 400));

    // set_angle_range 钳到 1-359，且受难度范围约束（medium = 30-200）
    let result = manager
        .create(Some("rotate"))
        .unwrap()
        .set_angle_range(0.0, 400.0)
        .generate()
        .unwrap();
    let angle = payload(&storage, &result.key)["angle"].as_f64().unwrap();
    assert!((30.0..=200.0).contains(&angle), "medium 应落在 30-200，实测 {angle}");

    // 难度决定点击目标个数：easy 2 / hard 4
    for (difficulty, expected) in [("easy", 2usize), ("hard", 4)] {
        let result = manager
            .create(Some("click"))
            .unwrap()
            .set_difficulty(difficulty)
            .generate()
            .unwrap();
        let targets = payload(&storage, &result.key)["targets"]
            .as_array()
            .unwrap()
            .len();
        assert_eq!(targets, expected, "{difficulty} 目标数");
    }

    // hard 的滑块拼图更小（40×40），且缺口仍然是正方形的
    let result = manager
        .create(Some("slider"))
        .unwrap()
        .set_difficulty("hard")
        .generate()
        .unwrap();
    assert_eq!(result.extra["puzzle_w"], 40);
    assert_eq!(result.extra["puzzle_h"], 40);
}

#[test]
fn click_targets_are_ordered_and_icon_mode_carries_thumbnails() {
    let (manager, _) = manager();

    // 文字模式：提示带 order，不含缩略图
    let result = manager
        .create(Some("click"))
        .unwrap()
        .set_difficulty("easy")
        .generate()
        .unwrap();
    let texts = result.extra["texts"].as_array().unwrap();
    assert_eq!(texts.len(), 2);
    assert_eq!(texts[0]["order"], 1);
    assert_eq!(texts[1]["order"], 2);
    assert!(texts[0]["text"].is_string());
    assert!(texts[0].get("thumb").is_none(), "文字模式不该有缩略图");

    // icon 模式：每项附矢量图形缩略图
    let result = manager
        .create(Some("click"))
        .unwrap()
        .set_difficulty("easy")
        .set_target_type("icon")
        .generate()
        .unwrap();
    for (index, item) in result.extra["texts"].as_array().unwrap().iter().enumerate() {
        assert_eq!(item["order"].as_u64().unwrap(), (index + 1) as u64);
        let thumb = decode_uri(item["thumb"].as_str().expect("icon 模式应有 thumb"));
        assert!(thumb.width() > 0 && thumb.height() > 0);
    }

    // 自定义词表：不足时循环取用
    let result = manager
        .create(Some("click"))
        .unwrap()
        .set_difficulty("hard")
        .set_words(vec!["甲".into(), "乙".into()])
        .generate()
        .unwrap();
    let texts: Vec<&str> = result.extra["texts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["text"].as_str().unwrap())
        .collect();
    assert_eq!(texts.len(), 4);
    assert!(texts.iter().all(|text| *text == "甲" || *text == "乙"));
}
