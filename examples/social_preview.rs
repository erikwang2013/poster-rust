//! 生成 GitHub Social Preview 图（1200×630）——**用本库自己画**，宠物 Posty 出镜。
//!
//! ```bash
//! cargo run --example social_preview --offline -j 4
//! # 输出 docs/social-preview.png（在 GitHub 仓库 Settings → Social preview 上传）
//! ```

use poster::PosterBuilder;
use poster::poster::builder::Direction;
use poster::poster::elements::{
    image::ImageElement, qrcode::QrcodeElement, shape::ShapeElement, text::TextElement,
};
use poster::drivers::{OverlayOptions, TextAlign, TextOptions};

const TITLE: &str = "poster-rust";
const SUBTITLE_ZH: &str = "Rust 图片验证码与海报生成工具包";
const SUBTITLE_EN: &str = "Rust captcha & poster generation toolkit";
const CHIPS: [&str; 4] = [
    "验证码 3 种 + 随机",
    "海报 14 种元素",
    "Guard + 8 框架集成",
    "纯 Rust · 无系统依赖",
];
const REPO: &str = "github.com/erikwang2013/poster-rust";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::path::Path::new("docs");
    std::fs::create_dir_all(out)?;

    let mut b = PosterBuilder::new()?;
    b.width(1200).height(630);

    // 背景：暖色渐变（Posty 配色）
    b.background_gradient("#FFF6F0", "#FFE3D5", Direction::Vertical);

    // 顶部品牌条
    b.add_shape(
        "rect",
        ShapeElement {
            x: 0,
            width: 1200,
            height: 14,
            color: "#FF6B6B".into(),
            ..Default::default()
        },
    );

    // 标题（艺术字描边）+ 中英副标题
    b.add_artistic_text(
        TITLE,
        "stroke",
        poster::poster::elements::artistic_text::ArtisticTextElement {
            x: 80,
            y: 168,
            size: 84.0,
            color: "#FFFFFF".into(),
            stroke_color: "#C2402F".into(),
            stroke_width: 3,
            ..Default::default()
        },
    );
    b.add_text(
        SUBTITLE_ZH,
        TextElement {
            x: 80,
            y: 236,
            style: TextOptions {
                size: 30.0,
                color: "#2D3436".into(),
                ..Default::default()
            },
            ..Default::default()
        },
    );
    b.add_text(
        SUBTITLE_EN,
        TextElement {
            x: 80,
            y: 278,
            style: TextOptions {
                size: 20.0,
                color: "#8A6A5B".into(),
                ..Default::default()
            },
            ..Default::default()
        },
    );

    // 能力标签：用本库字体测量真实宽度（CJK 宽度约为字号 ×1.8，不能拍脑袋估）
    let font = poster::Font::load(&poster::assets::default_font_path())?;
    let chip_size = 19.0;
    let pad = 26u32;
    let gap = 14i32;
    let mut x = 80i32;
    let mut y = 330i32;
    for chip in CHIPS {
        let text_w = font.measure(chip, chip_size).ceil() as u32;
        let w = text_w + pad * 2;
        if x + w as i32 > 800 {
            x = 80;
            y += 66;
        }
        b.add_shape(
            "rect",
            ShapeElement {
                x,
                y,
                width: w,
                height: 52,
                radius: Some(26),
                color: "#FFFFFF".into(),
                opacity: Some(0.9),
                ..Default::default()
            },
        );
        b.add_text(
            chip,
            TextElement {
                x: x + w as i32 / 2,
                y: y + 34,
                style: TextOptions {
                    size: chip_size,
                    color: "#C2402F".into(),
                    align: TextAlign::Center,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        x += w as i32 + gap;
    }

    // 宠物 Posty 出镜（右侧，避开标签区）
    b.add_pet(ImageElement {
        x: 830,
        y: 348,
        style: OverlayOptions {
            width: Some(260),
            height: Some(225), // 600:520
            ..Default::default()
        },
        ..Default::default()
    });

    // 二维码 + 仓库地址
    b.add_qrcode(
        &format!("https://{REPO}"),
        QrcodeElement {
            x: 1020,
            y: 52,
            size: 128,
            level: "M".into(),
            ..Default::default()
        },
    );
    b.add_text(
        "erikwang2013/poster-rust",
        TextElement {
            x: 1084,
            y: 208,
            style: TextOptions {
                size: 13.0,
                color: "#8A6A5B".into(),
                align: TextAlign::Center,
                ..Default::default()
            },
            ..Default::default()
        },
    );

    // 页脚：版权
    b.add_text(
        "MIT License · Copyright © 2026 erik <erik@erik.xyz> · https://erik.xyz",
        TextElement {
            x: 80,
            y: 600,
            style: TextOptions {
                size: 17.0,
                color: "#8A6A5B".into(),
                ..Default::default()
            },
            ..Default::default()
        },
    );

    let path = out.join("social-preview.png");
    b.save(&path, None)?;
    println!("已生成 {}×{} Social Preview: {}", 1200, 630, path.display());
    Ok(())
}
