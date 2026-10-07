//! Builder 测试：链式调用、三种背景、缺图占位、导出与输出。
//!
//! 全局配置（`set_global` 一个进程只能成功一次）在本文件里统一由 [`init_config`] 设置，
//! 每个测试都先调它，避免某个渲染测试先用默认值占住全局配置。

use image::Rgba;
use serde_json::json;

use poster::PosterConfig;
use poster::config::Placeholder;
use poster::drivers::{ImageDriver, TextOptions, color};
use poster::poster::elements::{
    Element, image::ImageElement, shape::ShapeElement, text::TextElement,
};
use poster::poster::{Direction, PosterBuilder};

/// 存在接近某色的像素（容差 40）。
fn has_color(hex: &str) -> impl Fn(&Rgba<u8>) -> bool {
    let target = color::parse(hex).expect("颜色");
    move |pixel: &Rgba<u8>| (0..3).all(|i| (pixel.0[i] as i32 - target.0[i] as i32).abs() <= 40)
}

fn painted(canvas: &ImageDriver) -> bool {
    canvas
        .image()
        .pixels()
        .any(|pixel| !pixel.0[..3].iter().all(|channel| *channel > 245))
}

/// 临时文件路径（测试自己造图用）。
fn temp_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("poster-rust-{name}"))
}

/// 本文件所有测试共用的全局配置：缺图占位画宠物。
fn init_config() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let mut config = PosterConfig::default();
        config.poster.placeholder = Some(Placeholder::Pet);
        poster::config::set_global(config).expect("全局配置只设置一次");
    });
}

#[test]
fn chaining_builds_and_renders() {
    init_config();
    let mut builder = PosterBuilder::new().unwrap();
    builder
        .width(300)
        .height(200)
        .background("#EEEEEE")
        .add_text(
            "链式调用",
            TextElement {
                x: 20,
                y: 80,
                style: TextOptions {
                    size: 32.0,
                    color: "#FF0000".into(),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .add_shape(
            "circle",
            ShapeElement {
                x: 200,
                y: 100,
                radius: Some(40),
                color: "#3366FF".into(),
                filled: true,
                ..Default::default()
            },
        );

    let canvas = builder.render().unwrap();
    assert_eq!(canvas.size(), (300, 200));
    assert!(canvas.image().pixels().any(has_color("#FF0000")), "应有文字");
    assert!(canvas.image().pixels().any(has_color("#3366FF")), "应有圆形");

    // render() 可重复调用，结果一致
    let again = builder.render().unwrap();
    assert_eq!(again.size(), canvas.size());
}

#[test]
fn default_size_comes_from_config() {
    init_config();
    let canvas = PosterBuilder::new().unwrap().render().unwrap();
    assert_eq!(canvas.size(), (750, 1334), "默认尺寸来自 poster.default_width/height");
}

#[test]
fn background_kinds_color_gradient_image() {
    init_config();
    // 纯色
    let mut color_bg = PosterBuilder::new().unwrap();
    color_bg.width(100).height(50).background("#123456");
    let canvas = color_bg.render().unwrap();
    assert!(canvas.image().pixels().all(has_color("#123456")), "应整幅纯色");

    // 渐变：竖直方向上下两端分别是起止色
    let mut gradient = PosterBuilder::new().unwrap();
    gradient
        .width(60)
        .height(200)
        .background_gradient("#FF0000", "#0000FF", Direction::Vertical);
    let canvas = gradient.render().unwrap();
    assert!(has_color("#FF0000")(canvas.image().get_pixel(30, 0)), "顶端应接近起始色");
    assert!(has_color("#0000FF")(canvas.image().get_pixel(30, 199)), "底端应接近结束色");

    // 图片背景：40×40 的红色方块铺满 200×100 画布（cover：放大后居中裁剪）
    let source = temp_path("builder-bg.png");
    ImageDriver::filled(40, 40, "#FF0000").unwrap().save(&source, None, None).unwrap();
    let mut image_bg = PosterBuilder::new().unwrap();
    image_bg.width(200).height(100).background(&source.to_string_lossy());
    let canvas = image_bg.render().unwrap();
    assert_eq!(canvas.size(), (200, 100));
    assert!(has_color("#FF0000")(canvas.image().get_pixel(0, 0)), "cover 应铺满四角");
    assert!(has_color("#FF0000")(canvas.image().get_pixel(199, 99)), "cover 应铺满四角");
    let _ = std::fs::remove_file(&source);
}

#[test]
fn add_pet_and_add_by_type_name() {
    init_config();
    let mut builder = PosterBuilder::new().unwrap();
    builder.width(200).height(200).add_pet(ImageElement {
        x: 10,
        y: 10,
        ..Default::default()
    });
    assert!(painted(&builder.render().unwrap()), "add_pet 应画出宠物图");

    // 按类型名追加
    let mut builder = PosterBuilder::new().unwrap();
    builder
        .width(200)
        .height(200)
        .add("shape", json!({"shape": "rect", "x": 10, "y": 10, "width": 80, "height": 40, "color": "#00AA00", "filled": true}))
        .unwrap();
    let canvas = builder.render().unwrap();
    assert!(canvas.image().pixels().any(has_color("#00AA00")), "add() 应生效");

    // 未知类型报错
    let mut builder = PosterBuilder::new().unwrap();
    let err = builder.add("hologram", json!({})).err().expect("未知类型应报错");
    assert!(err.to_string().contains("hologram"), "未知类型应报错: {err}");
}

#[test]
fn save_and_output_write_real_bytes() {
    init_config();
    let mut builder = PosterBuilder::new().unwrap();
    builder.width(120).height(80).background("#224466");

    let path = temp_path("builder-out.png");
    builder.save(&path, None).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G']), "png 魔数");
    let reopened = ImageDriver::load(&path).unwrap();
    assert_eq!(reopened.size(), (120, 80));
    let _ = std::fs::remove_file(&path);

    let data_uri = builder.output("png", None).unwrap();
    assert!(data_uri.starts_with("data:image/png;base64,"), "应输出 data URI");
    assert!(data_uri.len() > 100, "data URI 不应为空");
}

#[test]
fn to_array_shape_matches_template_contract() {
    init_config();
    let mut builder = PosterBuilder::new().unwrap();
    builder.width(320).height(480).add_text(
        "导出",
        TextElement {
            x: 10,
            y: 40,
            ..Default::default()
        },
    );
    let value = builder.to_array();
    assert_eq!(value["width"], json!(320));
    assert_eq!(value["height"], json!(480));
    let elements = value["elements"].as_array().expect("elements 数组");
    assert_eq!(elements.len(), 1);
    assert_eq!(elements[0]["type"], json!("text"));
    assert_eq!(elements[0]["text"], json!("导出"));

    // 导出 → 导入 → 导出，结果一致
    let template = poster::poster::PosterTemplate::from_config(value).unwrap();
    assert_eq!(template.width(), 320);
    assert_eq!(template.height(), 480);
    assert_eq!(template.to_array(), builder.to_array());
    assert_eq!(
        template.elements()[0],
        Element::Text(TextElement {
            text: "导出".into(),
            x: 10,
            y: 40,
            ..Default::default()
        })
    );
}

// ── 渲染缓存（对齐 PHP 的 $rendered 标志） ─────────────

#[test]
fn render_is_cached_until_invalidated() {
    init_config();
    let mut builder = PosterBuilder::new().unwrap();
    builder.width(150).height(100).background("#FFFFFF").add_shape(
        "rect",
        ShapeElement {
            x: 10,
            y: 10,
            width: 40,
            height: 20,
            color: "#FF0000".into(),
            filled: true,
            ..Default::default()
        },
    );

    // 无变更：重复 render / output 字节一致（第二次起命中缓存）
    let first = builder.render().unwrap();
    let second = builder.render().unwrap();
    assert_eq!(first.image().as_raw(), second.image().as_raw(), "无变更时重复渲染应一致");

    let uri_before = builder.output("png", None).unwrap();
    assert_eq!(uri_before, builder.output("png", None).unwrap(), "无变更时重复 output 应一致");

    // render() 返回的是副本：改动画布不应污染缓存
    let mut detached = builder.render().unwrap();
    detached.image_mut().put_pixel(0, 0, Rgba([0, 255, 0, 255]));
    let untouched = builder.render().unwrap();
    assert!(!has_color("#00FF00")(untouched.image().get_pixel(0, 0)), "render() 应返回副本");

    // 改元素 → 缓存失效，输出随之变化
    builder.add_shape(
        "circle",
        ShapeElement {
            x: 110,
            y: 60,
            radius: Some(25),
            color: "#0000FF".into(),
            filled: true,
            ..Default::default()
        },
    );
    let uri_after = builder.output("png", None).unwrap();
    assert_ne!(uri_before, uri_after, "改元素后应重新渲染");
    assert!(
        builder.render().unwrap().image().pixels().any(has_color("#0000FF")),
        "新元素应出现在画布上"
    );

    // 背景变更同样失效
    let uri_round2 = builder.output("png", None).unwrap();
    builder.background("#123456");
    assert_ne!(builder.output("png", None).unwrap(), uri_round2, "改背景后应重新渲染");
}

#[test]
fn output_bytes_matches_output_and_save() {
    init_config();
    let mut builder = PosterBuilder::new().unwrap();
    builder.width(90).height(60).background("#336699");

    let bytes = builder.output_bytes("png", None).unwrap();
    assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G']), "png 魔数");

    // 与 output() 的 data URI 载荷一致
    use base64::Engine;
    let uri = builder.output("png", None).unwrap();
    let payload = uri.strip_prefix("data:image/png;base64,").expect("data URI 前缀");
    assert_eq!(
        base64::engine::general_purpose::STANDARD.decode(payload).unwrap(),
        bytes,
        "output_bytes 与 output 载荷应一致"
    );

    // 缓存不改变保存结果：两次 save 到不同路径字节一致
    let path_a = temp_path("builder-cache-a.png");
    let path_b = temp_path("builder-cache-b.png");
    builder.save(&path_a, None).unwrap();
    builder.save(&path_b, None).unwrap();
    let file_a = std::fs::read(&path_a).unwrap();
    assert_eq!(file_a, std::fs::read(&path_b).unwrap(), "两次 save 字节一致");
    assert_eq!(file_a, bytes, "save 与 output_bytes 字节一致");
    let _ = std::fs::remove_file(&path_a);
    let _ = std::fs::remove_file(&path_b);
}

#[test]
fn missing_image_falls_back_to_pet_placeholder() {
    init_config();

    // 文件不存在 → 画宠物
    let mut builder = PosterBuilder::new().unwrap();
    builder.width(240).height(240).add_image(
        "definitely-missing.png",
        ImageElement {
            x: 20,
            y: 20,
            ..Default::default()
        },
    );
    assert!(painted(&builder.render().unwrap()), "缺图时应画 Posty 占位图");

    // 图片存在 → 画图片本身（占位只对缺失文件生效）
    let source = temp_path("builder-placeholder-source.png");
    ImageDriver::filled(30, 30, "#00FF00").unwrap().save(&source, None, None).unwrap();
    let mut builder = PosterBuilder::new().unwrap();
    builder.width(120).height(120).add_image(
        source.to_string_lossy(),
        ImageElement {
            x: 0,
            y: 0,
            ..Default::default()
        },
    );
    let canvas = builder.render().unwrap();
    assert!(canvas.image().pixels().any(has_color("#00FF00")), "存在的图片应正常绘制");
    let _ = std::fs::remove_file(&source);
}
