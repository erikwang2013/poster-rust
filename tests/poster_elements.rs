//! 14 种元素的渲染测试：画到白底画布上，回读尺寸与像素断言（不做逐字节比对）。

use image::Rgba;
use serde_json::json;

use poster::config;
use poster::drivers::{ImageDriver, LineOptions, OverlayOptions, TextOptions, color};
use poster::poster::elements::{
    Element, ElementRender, RenderCtx, TYPES, artistic_text::ArtisticTextElement,
    avatar::AvatarElement, calendar::CalendarElement, chart::ChartElement, emoji::EmojiElement,
    emoticon::EmoticonElement, icon::IconElement, image::ImageElement, line::LineElement,
    qrcode::QrcodeElement, shape::ShapeElement, table::TableElement, text::TextElement,
    watermark::WatermarkElement,
};

const WIDTH: u32 = 400;
const HEIGHT: u32 = 300;
const WHITE: &str = "#FFFFFF";

/// 把元素画到白底画布上，返回画布。
fn paint(element: &Element) -> ImageDriver {
    paint_on(element, WIDTH, HEIGHT)
}

fn paint_on(element: &Element, width: u32, height: u32) -> ImageDriver {
    let mut canvas = ImageDriver::filled(width, height, WHITE).expect("白底画布");
    element
        .render(
            &mut canvas,
            &RenderCtx {
                config: config::global(),
            },
        )
        .expect("元素渲染");
    canvas
}

/// 画布上是否存在满足条件的像素。
fn any_pixel(canvas: &ImageDriver, matches: impl Fn(&Rgba<u8>) -> bool) -> bool {
    canvas.image().pixels().any(matches)
}

/// 是否是接近白底的像素。
fn is_white(pixel: &Rgba<u8>) -> bool {
    pixel.0[..3].iter().all(|channel| *channel > 245)
}

/// 画上了东西（至少一个非白像素）。
fn painted(canvas: &ImageDriver) -> bool {
    any_pixel(canvas, |pixel| !is_white(pixel))
}

/// 存在某个颜色的像素（容差 40，容忍抗锯齿边缘）。
fn has_color(hex: &str) -> impl Fn(&Rgba<u8>) -> bool {
    let target = color::parse(hex).expect("颜色");
    move |pixel: &Rgba<u8>| {
        (0..3).all(|i| (pixel.0[i] as i32 - target.0[i] as i32).abs() <= 40)
    }
}

/// 宠物图路径（`add_pet` / 图片元素测试用）。
fn pet() -> String {
    poster::assets::pet_path().to_string_lossy().into_owned()
}

#[test]
fn text_renders_glyphs_in_color() {
    let element = Element::Text(TextElement {
        text: "你好，海报".into(),
        x: 20,
        y: 100,
        style: TextOptions {
            size: 40.0,
            color: "#FF0000".into(),
            ..Default::default()
        },
    });
    let canvas = paint(&element);
    assert_eq!(canvas.size(), (WIDTH, HEIGHT), "画布尺寸不应被元素改变");
    assert!(any_pixel(&canvas, has_color("#FF0000")), "应画出红色文字");
}

#[test]
fn image_renders_at_target_size() {
    let element = Element::Image(ImageElement {
        src: pet(),
        x: 10,
        y: 10,
        style: OverlayOptions {
            width: Some(120),
            height: Some(120),
            ..Default::default()
        },
    });
    let canvas = paint(&element);
    assert!(painted(&canvas), "图片应有非白像素");
    // 目标框外仍是白底：远处不该被覆盖
    assert!(is_white(canvas.image().get_pixel(200, 200)), "图片不该越界");
}

#[test]
fn avatar_renders_pet_with_border() {
    let element = Element::Avatar(AvatarElement {
        src: pet(),
        x: 20,
        y: 20,
        size: 100,
        circle: true,
        border: Some("#00FF00".into()),
        border_width: 4,
        style: OverlayOptions::default(),
    });
    let canvas = paint(&element);
    assert!(painted(&canvas), "头像应有非白像素");
    assert!(any_pixel(&canvas, has_color("#00FF00")), "应画出边框");
    // 圆形裁剪：正方形外角必须仍是白底
    assert!(is_white(canvas.image().get_pixel(22, 22)), "圆形裁剪的角应被切掉");
}

#[test]
fn qrcode_renders_black_modules() {
    let element = Element::Qrcode(QrcodeElement {
        content: "https://erik.xyz".into(),
        x: 20,
        y: 20,
        size: 180,
        ..Default::default()
    });
    let canvas = paint(&element);
    assert!(
        any_pixel(&canvas, |pixel| pixel.0[..3].iter().all(|c| *c < 60)),
        "二维码应有黑色模块"
    );
}

#[test]
fn shape_renders_rect_and_circle() {
    let rect = Element::Shape(ShapeElement {
        shape: "rect".into(),
        x: 20,
        y: 20,
        width: 120,
        height: 60,
        radius: Some(8),
        color: "#3366FF".into(),
        filled: true,
        ..Default::default()
    });
    let canvas = paint(&rect);
    assert!(any_pixel(&canvas, has_color("#3366FF")), "应画出填充矩形");

    let circle = Element::Shape(ShapeElement {
        shape: "circle".into(),
        x: 150,
        y: 150,
        radius: Some(40),
        color: "#FF00FF".into(),
        filled: true,
        ..Default::default()
    });
    let canvas = paint(&circle);
    assert!(any_pixel(&canvas, has_color("#FF00FF")), "应画出实心圆");
    // `x` / `y` 是圆心，圆外的角落仍是白底
    assert!(is_white(canvas.image().get_pixel(110, 110)), "圆外应保持白底");
}

#[test]
fn line_renders_with_width() {
    let element = Element::Line(LineElement {
        x1: 10,
        y1: 10,
        x2: Some(300),
        y2: Some(200),
        x: None,
        y: None,
        style: LineOptions {
            color: "#0000FF".into(),
            width: 4,
        },
    });
    let canvas = paint(&element);
    assert!(any_pixel(&canvas, has_color("#0000FF")), "应画出蓝色直线");
}

#[test]
fn watermark_tiles_repeatedly() {
    let element = Element::Watermark(WatermarkElement {
        text: "内部资料".into(),
        size: 18.0,
        color: "#CCCCCC".into(),
        spacing_x: Some(150),
        spacing_y: Some(100),
        ..Default::default()
    });
    let canvas = paint(&element);
    assert!(any_pixel(&canvas, has_color("#CCCCCC")), "应画出平铺水印");
}

#[test]
fn table_renders_header_and_rows() {
    let element = Element::Table(TableElement {
        x: 20,
        y: 20,
        header: vec![json!("姓名"), json!("分数")],
        rows: vec![vec![json!("张三"), json!(95)], vec![json!("李四"), json!(88)]],
        width: 300,
        header_bg: "#2C3E50".into(),
        row_bg: vec!["#FAFAFA".into(), "#FFFFFF".into()],
        alignments: vec!["left".into(), "right".into()],
        ..Default::default()
    });
    let canvas = paint(&element);
    assert!(any_pixel(&canvas, has_color("#2C3E50")), "应画出表头背景");
    assert!(any_pixel(&canvas, has_color("#FAFAFA")), "应画出偶数行背景");
    assert!(painted(&canvas), "应画出文字");
}

#[test]
fn chart_renders_all_three_types() {
    let data = vec![json!({"label": "一月", "value": 10}), json!({"label": "二月", "value": 30})];

    for chart_type in ["bar", "pie", "line"] {
        let element = Element::Chart(ChartElement {
            chart: chart_type.into(),
            data: data.clone(),
            x: 10,
            y: 10,
            width: 300,
            height: 200,
            ..Default::default()
        });
        let canvas = paint(&element);
        assert!(
            any_pixel(&canvas, has_color("#FF6B6B")),
            "{chart_type} 应使用默认调色板首色"
        );
        assert!(painted(&canvas), "{chart_type} 应画出内容");
    }
}

#[test]
fn chart_rejects_unknown_type() {
    let element = Element::Chart(ChartElement {
        chart: "radar".into(),
        data: vec![json!(1)],
        ..Default::default()
    });
    let mut canvas = ImageDriver::filled(100, 100, WHITE).unwrap();
    let err = element
        .render(&mut canvas, &RenderCtx { config: config::global() })
        .unwrap_err();
    assert!(err.to_string().contains("radar"), "应报出未知图表类型: {err}");
}

#[test]
fn calendar_renders_grid_and_highlight() {
    let mut highlights = std::collections::BTreeMap::new();
    highlights.insert("2026-10-01".to_string(), json!({"text": "国庆", "bg": "#FFEAA7"}));
    let element = Element::Calendar(CalendarElement {
        year: Some(2026),
        month: Some(10),
        x: 10,
        y: 10,
        cell_size: 60,
        highlights,
        header_bg: "#2C3E50".into(),
        ..Default::default()
    });
    let canvas = paint_on(&element, 460, 460);
    assert!(any_pixel(&canvas, has_color("#2C3E50")), "应画出标题栏");
    assert!(any_pixel(&canvas, has_color("#FFEAA7")), "应画出高亮日期背景");
    assert!(painted(&canvas), "应画出日期文字");
}

#[test]
fn calendar_rejects_out_of_range_month() {
    let element = Element::Calendar(CalendarElement {
        year: Some(2026),
        month: Some(13),
        ..Default::default()
    });
    let mut canvas = ImageDriver::filled(100, 100, WHITE).unwrap();
    let err = element
        .render(&mut canvas, &RenderCtx { config: config::global() })
        .unwrap_err();
    assert!(err.to_string().contains("13"), "应报出非法月份: {err}");
}

#[test]
fn artistic_text_renders_all_four_styles() {
    for style in ["stroke", "shadow", "gradient", "neon"] {
        let element = Element::ArtisticText(ArtisticTextElement {
            text: "艺术字".into(),
            x: 20,
            y: 120,
            size: 56.0,
            style: style.into(),
            ..Default::default()
        });
        let canvas = paint(&element);
        assert!(painted(&canvas), "{style} 样式应画出文字");
    }
}

#[test]
fn emoji_renders_text_and_codepoint() {
    let emoji = Element::Emoji(EmojiElement {
        emoji: "A".into(),
        x: 20,
        y: 80,
        size: 48,
        ..Default::default()
    });
    assert!(painted(&paint(&emoji)), "emoji 元素应能画出字符");

    // 码点写法：没有 emoji 字体的机器上退回普通文字，也不该报错
    let by_codepoint = Element::Emoji(EmojiElement {
        codepoint: Some(json!("U+1F600")),
        x: 20,
        y: 80,
        size: 48,
        ..Default::default()
    });
    paint(&by_codepoint);
}

#[test]
fn icon_falls_back_to_placeholder_text() {
    let element = Element::Icon(IconElement {
        icon: "heart".into(),
        x: 20,
        y: 60,
        size: 32,
        ..Default::default()
    });
    // 没有 FontAwesome 字体时画 `[heart]` 占位（同 PHP）
    assert!(painted(&paint(&element)), "无图标字体时应画占位文字");
    assert_eq!(poster::poster::elements::icon::icon_char("heart"), Some('\u{F004}'));
    assert_eq!(poster::poster::elements::icon::icon_char("nope"), None);
}

#[test]
fn emoticon_renders_kaomoji() {
    let element = Element::Emoticon(EmoticonElement {
        text: "(^_^)".into(),
        x: 20,
        y: 80,
        size: 32,
        ..Default::default()
    });
    assert!(painted(&paint(&element)), "颜文字应画出字符");

    let preset = Element::Emoticon(EmoticonElement {
        expression: "happy".into(),
        x: 20,
        y: 80,
        ..Default::default()
    });
    paint(&preset);
}

#[test]
fn registry_types_resolve_to_every_variant() {
    let defs = [
        ("text", json!({})),
        ("image", json!({"src": "x.png"})),
        ("qrcode", json!({"content": "x"})),
        ("avatar", json!({"src": "x.png"})),
        ("shape", json!({"shape": "rect"})),
        ("line", json!({})),
        ("watermark", json!({"text": "x"})),
        ("table", json!({})),
        ("chart", json!({})),
        ("calendar", json!({})),
        ("artistictext", json!({"text": "x"})),
        ("artistic-text", json!({"text": "x"})),
        ("emoji", json!({})),
        ("icon", json!({})),
        ("emoticon", json!({})),
    ];
    assert_eq!(defs.len(), TYPES.len(), "注册表类型数量应与测试覆盖一致");
    for (kind, options) in defs {
        let element = Element::from_parts(kind, options).expect(kind);
        assert!(TYPES.contains(&element.kind()) || element.kind() == "artistic-text");
    }
}
