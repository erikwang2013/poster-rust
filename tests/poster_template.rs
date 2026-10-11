//! 模板测试：导出 → 导入 → 导出 一致性、`{{var}}` 替换、缺失变量报错、替换/追加语义。

use serde_json::json;

use poster::PosterError;
use poster::poster::elements::Element;
use poster::poster::elements::text::TextElement;
use poster::poster::{PosterBuilder, PosterTemplate};

/// 一份包含多种元素与嵌套结构的模板。
fn sample_template() -> serde_json::Value {
    json!({
        "width": 640,
        "height": 960,
        "elements": [
            {"type": "text", "text": "你好 {{name}}", "x": 40, "y": 120, "size": 48,
             "color": "#222222", "align": "center"},
            {"type": "image", "src": "photo.png", "x": 40, "y": 200, "width": 200, "height": 200},
            {"type": "qrcode", "content": "https://{{host}}/p/{{id}}", "x": 420, "y": 700,
             "size": 180, "label": "扫码查看", "label_size": 16, "label_color": "#999999"},
            {"type": "table", "x": 40, "y": 460, "headers": ["项目", "金额"],
             "rows": [["合计", "{{total}}"]], "col_widths": [200, 200], "header_bg": "#4ECDC4"},
            {"type": "chart", "chart": "bar", "x": 40, "y": 600,
             "data": [{"label": "一月", "value": 10}, {"label": "二月", "value": 20}]},
            {"type": "calendar", "x": 40, "y": 760, "cell_size": 60, "month": 10,
             "highlights": {"2026-10-01": "国庆"}},
            {"type": "artistictext", "text": "限时 {{id}}", "style": "neon", "x": 40, "y": 300}
        ]
    })
}

#[test]
fn export_import_export_is_stable() {
    let template = PosterTemplate::from_config(sample_template()).unwrap();
    assert_eq!((template.width(), template.height()), (640, 960));
    assert_eq!(template.elements().len(), 7);

    let exported = template.to_array();
    let reparsed = PosterTemplate::from_config(exported.clone()).unwrap();
    assert_eq!(
        reparsed.to_array(),
        exported,
        "导出 → 导入 → 导出 应逐键一致"
    );

    // 再走一轮，确认没有字段在第一轮里丢失
    let third = PosterTemplate::from_config(reparsed.to_array()).unwrap();
    assert_eq!(third.to_array(), exported);
}

#[test]
fn builder_template_round_trip_matches() {
    let template = PosterTemplate::from_config(sample_template()).unwrap();
    let mut builder = PosterBuilder::new().unwrap();
    builder.use_template(template);

    let exported = builder.to_array();
    assert_eq!(exported["width"], json!(640));
    assert_eq!(exported["height"], json!(960), "模板尺寸应覆盖配置默认值");
    assert_eq!(exported["elements"].as_array().unwrap().len(), 7);

    let reparsed = PosterTemplate::from_config(exported.clone()).unwrap();
    assert_eq!(reparsed.to_array(), exported);
}

#[test]
fn snake_case_keys_and_values_are_accepted() {
    // snake_case 键名：label_size / col_widths / headers、artistictext 别名
    let template = PosterTemplate::from_config(sample_template()).unwrap();
    let qrcode = &template.elements()[2];
    let value = serde_json::to_value(qrcode).unwrap();
    assert_eq!(
        value["label_size"],
        json!(16),
        "label_size 键名保持 snake_case"
    );
    assert_eq!(
        template.elements()[6].kind(),
        "artistic-text",
        "artistictext 别名应转为规范名"
    );

    // 元素定义缺 type 报错
    let err = PosterTemplate::from_config(json!({"elements": [{"x": 1}]})).unwrap_err();
    assert!(matches!(err, PosterError::Template(_)));
    assert!(err.to_string().contains("#0"), "报错应带元素下标: {err}");

    // 未知类型报错
    let err = PosterTemplate::from_config(json!({"elements": [{"type": "hologram"}]})).unwrap_err();
    assert!(err.to_string().contains("hologram"));

    // 非法 JSON 报错
    assert!(PosterTemplate::from_json("{ not json").is_err());
}

/// 历史 snake_case 多词键必须与 camelCase 同效（此前被静默忽略，
/// `examples/poster_template.rs` 的 `"max_width"` 即踩坑现场）。
#[test]
fn snake_case_option_keys_are_not_dropped() {
    let template = PosterTemplate::from_config(json!({
        "width": 750,
        "height": 400,
        "elements": [
            {"type": "text", "text": "长文本", "x": 20, "y": 80, "size": 24,
             "max_width": 620, "line_height": 40},
            {"type": "calendar", "x": 0, "y": 0, "cell_size": 40, "start_day": 1,
             "header_bg": "#123456", "today_bg": "#654321"},
            {"type": "artistictext", "text": "艺术", "x": 0, "y": 0,
             "max_width": 300, "stroke_width": 3, "glow_color": "#FFFFFF"},
            {"type": "avatar", "src": "pet.png", "x": 0, "y": 0, "size": 40,
             "border": "#000000", "border_width": 3}
        ]
    }))
    .unwrap();

    match &template.elements()[0] {
        Element::Text(t) => {
            assert_eq!(t.style.max_width, 620.0, "max_width 应生效");
            assert_eq!(t.style.line_height, Some(40.0), "line_height 应生效");
        }
        other => panic!("应为 text 元素: {other:?}"),
    }
    match &template.elements()[1] {
        Element::Calendar(c) => {
            assert_eq!(c.cell_size, 40);
            assert_eq!(c.start_day, 1);
            assert_eq!(c.header_bg, "#123456");
            assert_eq!(c.today_bg, "#654321");
        }
        other => panic!("应为 calendar 元素: {other:?}"),
    }
    match &template.elements()[2] {
        Element::ArtisticText(a) => {
            assert_eq!(a.max_width, 300.0);
            assert_eq!(a.stroke_width, 3);
            assert_eq!(a.glow_color.as_deref(), Some("#FFFFFF"));
        }
        other => panic!("应为 artistic-text 元素: {other:?}"),
    }
    match &template.elements()[3] {
        Element::Avatar(a) => assert_eq!(a.border_width, 3),
        other => panic!("应为 avatar 元素: {other:?}"),
    }
}

/// 端到端：`max_width`（snake_case）与 `maxWidth` 渲染结果逐像素一致，
/// 且长正文确实换行（墨迹不越过 x + max_width）。
#[test]
fn snake_case_max_width_wraps_like_camel_case() {
    let text = "感谢支持，这是为你准备的本月专属权益，请查收。".repeat(2);
    let template = |key: &str| {
        let mut element = serde_json::Map::new();
        element.insert("type".into(), json!("text"));
        element.insert("text".into(), json!(text));
        element.insert("x".into(), json!(60));
        element.insert("y".into(), json!(120));
        element.insert("size".into(), json!(24));
        element.insert(key.into(), json!(620));
        json!({"width": 750, "height": 300, "elements": [serde_json::Value::Object(element)]})
    };

    let render = |key: &str| {
        let mut builder = PosterBuilder::new().unwrap();
        builder.use_template(PosterTemplate::from_config(template(key)).unwrap());
        builder.render().unwrap()
    };

    let camel = render("maxWidth");
    let snake = render("max_width");
    assert_eq!(
        camel.image().as_raw(),
        snake.image().as_raw(),
        "两种键名应同效"
    );

    // 换行生效：墨迹不越过 60 + 620；未换行时正文宽 ≈ 2×620+，会顶到画布右缘 750。
    // 底色是不透明白（alpha 恒 255），墨迹按「非白」判定。
    let ink = |x: u32, y: u32| {
        camel
            .image()
            .get_pixel(x, y)
            .0
            .iter()
            .take(3)
            .any(|channel| *channel < 200)
    };
    let ink_columns: Vec<u32> = (0..camel.width())
        .filter(|x| (0..camel.height()).any(|y| ink(*x, y)))
        .collect();
    assert!(!ink_columns.is_empty(), "应画出文字");
    assert!(
        ink_columns.iter().all(|x| *x < 60 + 620),
        "文本溢出行宽: max_x={:?}",
        ink_columns.iter().max()
    );
    // 两行：墨迹行分成 2 段
    let mut bands = 0;
    let mut in_ink = false;
    for y in 0..camel.height() {
        let row_inked = (0..camel.width()).any(|x| ink(x, y));
        if row_inked && !in_ink {
            bands += 1;
        }
        in_ink = row_inked;
    }
    assert!(bands >= 2, "长正文应至少换 2 行: bands={bands}");
}

#[test]
fn variables_are_substituted_everywhere() {
    let template = PosterTemplate::from_config(sample_template()).unwrap();
    let mut builder = PosterBuilder::new().unwrap();
    builder.use_template(template).with([
        ("name", "世界"),
        ("host", "erik.xyz"),
        ("id", "42"),
        ("total", "¥1,280"),
    ]);

    let canvas = builder.render().unwrap();
    assert_eq!(canvas.size(), (640, 960));

    let elements = builder.to_array()["elements"].clone();
    assert_eq!(elements[0]["text"], json!("你好 世界"));
    assert_eq!(elements[2]["content"], json!("https://erik.xyz/p/42"));
    assert_eq!(
        elements[3]["rows"][0][1],
        json!("¥1,280"),
        "嵌套数组里的变量也要替换"
    );
    assert_eq!(elements[6]["text"], json!("限时 42"));
}

#[test]
fn missing_variable_is_an_error() {
    let template = PosterTemplate::from_config(sample_template()).unwrap();
    let mut builder = PosterBuilder::new().unwrap();
    builder.use_template(template).with([("name", "世界")]);

    let err = builder.render().err().expect("缺变量应报错");
    assert!(
        matches!(err, PosterError::Template(_)),
        "缺变量应报模板错误: {err}"
    );
    assert!(
        err.to_string().contains("host"),
        "报错应指出缺哪个变量: {err}"
    );
}

#[test]
fn handwritten_elements_are_replaced_or_appended() {
    let template = PosterTemplate::from_config(sample_template()).unwrap();

    // 默认：模板整体替换手写元素
    let mut replace = PosterBuilder::new().unwrap();
    replace
        .add_text(
            "手写元素",
            TextElement {
                y: 10,
                ..Default::default()
            },
        )
        .use_template(template.clone());
    assert_eq!(replace.to_array()["elements"].as_array().unwrap().len(), 7);

    // replaceElements(false)：先手写元素，再追加模板元素
    let mut append = PosterBuilder::new().unwrap();
    append
        .add_text(
            "手写元素",
            TextElement {
                y: 10,
                ..Default::default()
            },
        )
        .use_template(template.clone())
        .replace_elements(false);
    let elements = append.to_array()["elements"].clone();
    assert_eq!(elements.as_array().unwrap().len(), 8);
    assert_eq!(elements[0]["text"], json!("手写元素"), "手写元素排在前面");

    // 无模板时按原样导出
    let mut plain = PosterBuilder::new().unwrap();
    plain.add_text(
        "只有手写",
        TextElement {
            y: 10,
            ..Default::default()
        },
    );
    assert_eq!(plain.to_array()["elements"].as_array().unwrap().len(), 1);
}
