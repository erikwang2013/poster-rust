//! 模板示例：同一份模板 JSON + 不同变量 → 批量出图（导出 → 导入 → 再导出）。
//!
//! ```bash
//! cargo run --example poster_template --offline -j 4
//! ```
//! 输出到 `examples/output/poster_template_*.png`。

use serde_json::json;

use poster::assets;
use poster::poster::{PosterBuilder, PosterTemplate};

fn main() -> poster::Result<()> {
    let pet = assets::pet_path().to_string_lossy().into_owned();

    // 模板：键名与 PHP 版逐字一致（snake_case 的 label_size、别名 headers…）
    let template_json = json!({
        "width": 750,
        "height": 1000,
        "elements": [
            {"type": "shape", "shape": "rect", "x": 0, "y": 0, "width": 750, "height": 240,
             "color": "#4ECDC4", "filled": true},
            {"type": "artistictext", "text": "{{title}}", "style": "shadow",
             "x": 50, "y": 150, "size": 56, "color": "#FFFFFF"},
            {"type": "text", "text": "亲爱的 {{name}}：", "x": 60, "y": 330, "size": 30},
            {"type": "text", "text": "{{message}}", "x": 60, "y": 390, "size": 24,
             "color": "#666666", "max_width": 620},
            {"type": "image", "src": "{{avatar}}", "x": 560, "y": 300, "width": 130, "height": 130,
             "radius": 65},
            {"type": "qrcode", "content": "{{url}}", "x": 60, "y": 560, "size": 200,
             "label": "扫码领取", "label_size": 16},
            {"type": "table", "x": 300, "y": 580, "headers": ["项目", "金额"],
             "rows": [["订单号", "{{order}}"], ["实付", "{{amount}}"]],
             "col_widths": [140, 200], "header_bg": "#4ECDC4", "header_color": "#FFFFFF"},
            {"type": "watermark", "text": "POSTY", "size": 18, "color": "#00000011",
             "angle": -30, "spacing_x": 180, "spacing_y": 140}
        ]
    });

    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/output");
    std::fs::create_dir_all(&output).map_err(poster::PosterError::Io)?;

    // 一份模板，批量出图
    let users = [
        ("张三", "¥1,280", "20261006001"),
        ("李四", "¥3,600", "20261006002"),
    ];
    for (index, (name, amount, order)) in users.iter().enumerate() {
        let template = PosterTemplate::from_config(template_json.clone())?;
        let mut builder = PosterBuilder::new()?;
        builder.use_template(template).with([
            ("title", "专属优惠券"),
            ("name", *name),
            ("message", "感谢支持，这是为你准备的本月专属权益，请查收。"),
            ("avatar", pet.as_str()),
            ("url", "https://erik.xyz/poster-rust"),
            ("order", *order),
            ("amount", *amount),
        ]);
        let path = output.join(format!("poster_template_{}.png", index + 1));
        builder.save(&path, None)?;
        println!("已生成 {}", path.display());
    }

    // 导出模板（builder → JSON 文本），可存库或交给 PHP 版读取
    let mut builder = PosterBuilder::new()?;
    builder.use_template(PosterTemplate::from_config(template_json.clone())?);
    let exported = builder.to_array();
    let text = serde_json::to_string_pretty(&exported)?;
    println!("\n导出模板（{} 个元素，{} 字节）：", exported["elements"].as_array().map_or(0, Vec::len), text.len());
    println!("{}", &text[..text.len().min(400)]);

    Ok(())
}
