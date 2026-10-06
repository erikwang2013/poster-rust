//! 模板测试：导出 → 导入 → 导出 一致性、`{{var}}` 替换、缺失变量报错、替换/追加语义。

use serde_json::json;

use poster::PosterError;
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
    assert_eq!(reparsed.to_array(), exported, "导出 → 导入 → 导出 应逐键一致");

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
fn php_style_keys_and_values_are_accepted() {
    // PHP 导出的键名：snake_case 的 label_size / col_widths / headers、artistictext 别名
    let template = PosterTemplate::from_config(sample_template()).unwrap();
    let qrcode = &template.elements()[2];
    let value = serde_json::to_value(qrcode).unwrap();
    assert_eq!(value["label_size"], json!(16), "label_size 键名保持 snake_case");
    assert_eq!(template.elements()[6].kind(), "artistic-text", "artistictext 别名应转为规范名");

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

#[test]
fn variables_are_substituted_everywhere() {
    let template = PosterTemplate::from_config(sample_template()).unwrap();
    let mut builder = PosterBuilder::new().unwrap();
    builder
        .use_template(template)
        .with([
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
    assert_eq!(elements[3]["rows"][0][1], json!("¥1,280"), "嵌套数组里的变量也要替换");
    assert_eq!(elements[6]["text"], json!("限时 42"));
}

#[test]
fn missing_variable_is_an_error() {
    let template = PosterTemplate::from_config(sample_template()).unwrap();
    let mut builder = PosterBuilder::new().unwrap();
    builder.use_template(template).with([("name", "世界")]);

    let err = builder.render().err().expect("缺变量应报错");
    assert!(matches!(err, PosterError::Template(_)), "缺变量应报模板错误: {err}");
    assert!(err.to_string().contains("host"), "报错应指出缺哪个变量: {err}");
}

#[test]
fn handwritten_elements_are_replaced_or_appended() {
    let template = PosterTemplate::from_config(sample_template()).unwrap();

    // 默认：模板整体替换手写元素
    let mut replace = PosterBuilder::new().unwrap();
    replace
        .add_text("手写元素", TextElement { y: 10, ..Default::default() })
        .use_template(template.clone());
    assert_eq!(replace.to_array()["elements"].as_array().unwrap().len(), 7);

    // replaceElements(false)：先手写元素，再追加模板元素
    let mut append = PosterBuilder::new().unwrap();
    append
        .add_text("手写元素", TextElement { y: 10, ..Default::default() })
        .use_template(template.clone())
        .replace_elements(false);
    let elements = append.to_array()["elements"].clone();
    assert_eq!(elements.as_array().unwrap().len(), 8);
    assert_eq!(elements[0]["text"], json!("手写元素"), "手写元素排在前面");

    // 无模板时按原样导出
    let mut plain = PosterBuilder::new().unwrap();
    plain.add_text("只有手写", TextElement { y: 10, ..Default::default() });
    assert_eq!(plain.to_array()["elements"].as_array().unwrap().len(), 1);
}
