//! 海报全元素示例：一张 750×1334 的海报，把 14 种元素都画一遍。
//!
//! ```bash
//! cargo run --example poster_showcase --offline -j 4
//! ```
//! 输出到 `examples/output/poster_showcase.png`。

use serde_json::json;

use poster::assets;
use poster::drivers::{LineOptions, OverlayOptions, TextOptions, TextAlign};
use poster::poster::elements::{
    artistic_text::ArtisticTextElement, avatar::AvatarElement, calendar::CalendarElement,
    chart::ChartElement, emoji::EmojiElement, emoticon::EmoticonElement, icon::IconElement,
    image::ImageElement, line::LineElement, qrcode::QrcodeElement, shape::ShapeElement,
    table::TableElement, text::TextElement, watermark::WatermarkElement,
};
use poster::poster::{Direction, PosterBuilder};

fn main() -> poster::Result<()> {
    let pet = assets::pet_path().to_string_lossy().into_owned();

    let mut builder = PosterBuilder::new()?;
    builder.width(750).height(1334);

    // 背景：竖直渐变
    builder.background_gradient("#FFF6F0", "#FFE3D5", Direction::Vertical);

    // 顶部装饰条 + 卡片底
    builder
        .add_shape(
            "rect",
            ShapeElement {
                x: 0,
                y: 0,
                width: 750,
                height: 180,
                color: "#FF6B6B".into(),
                filled: true,
                ..Default::default()
            },
        )
        .add_shape(
            "rect",
            ShapeElement {
                x: 40,
                y: 380,
                width: 670,
                height: 380,
                radius: Some(24),
                color: "#FFFFFF".into(),
                filled: true,
                ..Default::default()
            },
        )
        .add_shape(
            "circle",
            ShapeElement {
                x: 660,
                y: 120,
                radius: Some(70),
                color: "#FFEAA7".into(),
                opacity: Some(0.5),
                filled: true,
                ..Default::default()
            },
        );

    // 艺术字标题（霓虹）+ 普通文字
    builder
        .add_artistic_text(
            "海报生成器",
            "stroke",
            ArtisticTextElement {
                x: 60,
                y: 128,          // 字变准后（与 PHP 同口径）标题下移，避免贴顶
                size: 60.0,
                color: "#FFFFFF".into(),
                stroke_color: "#C2402F".into(),
                stroke_width: 2,
                ..Default::default()
            },
        )
        .add_text(
            "纯 Rust · 14 种元素 · 一行链式调用",
            TextElement {
                x: 375,
                y: 238,
                style: TextOptions {
                    size: 26.0,
                    color: "#8A6A5B".into(),
                    align: TextAlign::Center,
                    ..Default::default()
                },
                ..Default::default()
            },
        );

    // 分身像 + 宠物图
    builder
        .add_avatar(
            pet.clone(),
            AvatarElement {
                x: 70,
                y: 300,
                size: 120,
                circle: true,
                border: Some("#FF6B6B".into()),
                border_width: 4,
                style: OverlayOptions::default(),
                ..Default::default()
            },
        )
        .add_image(
            pet.clone(),
            ImageElement {
                x: 560,
                y: 420,
                style: OverlayOptions {
                    width: Some(120),
                    height: Some(120),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .add_text(
            "Posty 出品",
            TextElement {
                x: 130,
                y: 480,
                style: TextOptions {
                    size: 28.0,
                    color: "#333333".into(),
                    align: TextAlign::Center,
                    ..Default::default()
                },
                ..Default::default()
            },
        );

    // 图例文字 + 分隔线
    builder
        .add_line(LineElement {
            x1: 60,
            y1: 520,
            x2: Some(690),
            y2: Some(520),
            x: None,
            y: None,
            style: LineOptions {
                color: "#FFD9C7".into(),
                width: 3,
            },
        })
        .add_text(
            "本月数据",
            TextElement {
                x: 60,
                y: 570,
                style: TextOptions {
                    size: 30.0,
                    color: "#FF6B6B".into(),
                    ..Default::default()
                },
                ..Default::default()
            },
        );

    // 表格
    builder.add_table(TableElement {
        x: 60,
        y: 600,
        header: vec![json!("渠道"), json!("新增"), json!("占比")],
        rows: vec![
            vec![json!("小程序"), json!(1280), json!("42%")],
            vec![json!("App"), json!(960), json!("31%")],
            vec![json!("H5"), json!(820), json!("27%")],
        ],
        width: 630,
        header_bg: "#FF6B6B".into(),
        header_color: "#FFFFFF".into(),
        row_bg: vec!["#FFF8F4".into(), "#FFFFFF".into()],
        alignments: vec!["left".into(), "right".into(), "right".into()],
        ..Default::default()
    });

    // 图表
    let sales = vec![
        json!({"label": "一月", "value": 120}),
        json!({"label": "二月", "value": 200}),
        json!({"label": "三月", "value": 160}),
        json!({"label": "四月", "value": 260}),
    ];
    builder
        .add_chart(
            "bar",
            sales.clone(),
            ChartElement {
                x: 50,
                y: 780,
                width: 650,
                height: 200,
                ..Default::default()
            },
        )
        .add_chart(
            "pie",
            vec![json!({"label": "小程序", "value": 42}), json!({"label": "App", "value": 31}), json!({"label": "H5", "value": 27})],
            ChartElement {
                x: 350,
                y: 1000,
                width: 230,
                height: 200,
                ..Default::default()
            },
        );

    // 日历
    let mut highlights = std::collections::BTreeMap::new();
    highlights.insert("2026-10-01".to_string(), json!({"text": "国庆", "bg": "#FFEAA7"}));
    highlights.insert("2026-10-06".to_string(), json!({"text": "今天", "bg": "#D6F5F0"}));
    builder.add_calendar(CalendarElement {
        year: Some(2026),
        month: Some(10),
        x: 30,
        y: 1000,
        cell_size: 42,
        highlights,
        ..Default::default()
    });

    // 图标 / 颜文字 / Emoji
    builder
        .add_icon(
            "heart",
            IconElement {
                x: 585,
                y: 1262,        // 未提供 FontAwesome 字体时渲染为字面 "[heart]"
                size: 20,
                color: "#FF6B6B".into(),
                ..Default::default()
            },
        )
        .add_emoticon(
            "happy",
            EmoticonElement {
                x: 340,
                y: 1262,
                size: 18,
                color: "#8A6A5B".into(),
                ..Default::default()
            },
        )
        .add_emoji(
            "A",
            EmojiElement {
                x: 520,
                y: 1262,
                size: 24,
                ..Default::default()
            },
        )
        .add_artistic_text(
            "限时活动",
            "neon",
            ArtisticTextElement {
                x: 545,
                y: 1212,         // 二维码文案下方的空带
                size: 22.0,
                glow_color: Some("#FF6B6B".into()),
                ..Default::default()
            },
        );

    // 二维码（带文案）
    builder.add_qrcode(
        "https://erik.xyz/poster-rust",
        QrcodeElement {
            x: 610,
            y: 1010,
            size: 120,
            logo: None,
            label: Some("扫码看源码".into()),
            ..Default::default()
        },
    );

    // 平铺水印
    builder.add_watermark(
        "POSTY",
        WatermarkElement {
            font: None,
            size: 16.0,
            color: "#FF000010".into(),
            angle: -30.0,
            spacing_x: Some(180),
            spacing_y: Some(140),
            ..Default::default()
        },
    );

    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/output");
    std::fs::create_dir_all(&output).map_err(poster::PosterError::Io)?;
    let png = output.join("poster_showcase.png");
    let jpg = output.join("poster_showcase.jpg");
    builder.save(&png, None)?;
    builder.save(&jpg, Some(88))?;

    let canvas = builder.render()?;
    println!(
        "已生成 {}×{} 海报：{}\n  {}\n  {}\n  data URI 长度 {} 字节",
        canvas.width(),
        canvas.height(),
        png.display(),
        jpg.display(),
        output.display(),
        builder.output("png", None)?.len(),
    );
    Ok(())
}
