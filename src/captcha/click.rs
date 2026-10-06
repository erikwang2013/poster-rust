//! 点击验证码，对应 PHP `ClickCaptcha`。
//!
//! 难度决定目标个数（easy 2 / medium 3 / hard 4）；目标按「槽位网格 + 整体随机平移」布点，
//! 两两间距 >= 2×容差，保证任意点最多落进一个目标的容差圆；画布装不下直接报错而不是静默退化。
//!
//! `set_target_type("icon")` 把目标文字换成 11 种程序化矢量图形（图元现画，不依赖素材），
//! `extra.texts[].thumb` 给出该图形的小图 data URI，颜色用中性色以免泄露画布配色。

use rand::Rng;
use rand::seq::SliceRandom;
use serde_json::{Value, json};

use crate::drivers::{LineOptions, OverlayOptions, ShapeOptions, TextAlign, TextOptions};
use crate::drivers::ImageDriver;
use crate::error::{PosterError, Result};

use super::{Base, CaptchaBuilder, CaptchaResult, CaptchaType, Difficulty, TargetType};

/// 字体目标字号（同 PHP）。
const TEXT_SIZE: f32 = 16.0;
/// 图标外接尺寸（同 PHP）。
const ICON_SIZE: i32 = 22;
/// 图标画布留白：旋转后的外接尺寸（同 PHP `+6`）。
const ICON_PADDING: u32 = 6;
/// 提示缩略图的中性色：用画布上的随机色等于把目标颜色告诉攻击者（PHP `THUMB_COLOR`）。
const THUMB_COLOR: &str = "#37474F";

/// 图标形状，全部由驱动图元现画（PHP `ClickCaptcha::SHAPES`）。
const SHAPES: [&str; 11] = [
    "circle",
    "ring",
    "square",
    "rounded",
    "bar",
    "cross",
    "x",
    "chevron",
    "semicircle",
    "wedge",
    "asterisk",
];

/// 一个待绘制的目标。
#[derive(Debug, Clone)]
struct Target {
    x: i32,
    y: i32,
    text: String,
    order: usize,
    color: String,
    angle: i32,
}

/// 生成点击验证码。
pub(crate) fn generate(builder: CaptchaBuilder) -> Result<CaptchaResult> {
    let difficulty = builder.difficulty();
    let target_type = builder.target_type();
    let words = builder.words();
    let (width, height) = builder.canvas_size();

    let mut base = Base::new(
        builder.ctx().clone(),
        CaptchaType::Click,
        difficulty,
        builder.background(),
        width,
        height,
    );
    let mut bg = base.create_background()?;

    let count = match difficulty {
        Difficulty::Easy => 2,
        Difficulty::Hard => 4,
        Difficulty::Medium => 3,
    };

    let mut targets = place_targets(&base, count, target_type, words)?;
    let mut rng = rand::rng();
    for target in &mut targets {
        // 色相随机 + 整体随机旋转：恒定色正立绘制会被按颜色分离精确还原坐标
        target.color = random_target_color();
        target.angle = rng.random_range(-15..=15);

        if target_type == TargetType::Icon {
            let color = format!("{}{}", target.color, random_alpha());
            let icon = render_icon(&target.text, &color, target.angle)?;
            let (w, h) = icon.size();
            bg.overlay(
                &icon,
                target.x - (w / 2) as i32,
                target.y - (h / 2) as i32,
                &OverlayOptions::default(),
            )?;
        } else {
            bg.text(
                &target.text,
                target.x as f32,
                (target.y + 6) as f32,
                &TextOptions {
                    size: TEXT_SIZE,
                    color: target.color.clone(),
                    align: TextAlign::Center,
                    angle: target.angle as f32,
                    ..Default::default()
                },
            )?;
        }
    }

    draw_overlay_noise(&mut bg, &targets)?;

    let extra = json!({ "texts": hints(&targets, target_type)? });
    let png = bg.encode("png", None)?;
    let answer = json!({ "targets": targets.iter().map(target_json).collect::<Vec<Value>>() });
    base.finish(&png, extra, answer)
}

/// 目标 → 存储载荷（前端拿不到目标坐标，只存服务端）。
fn target_json(target: &Target) -> Value {
    json!({
        "x": target.x,
        "y": target.y,
        "text": target.text,
        "order": target.order,
        "color": target.color,
        "angle": target.angle,
    })
}

/// 目标布点：槽位网格 + 整体随机平移，两两间距恒定（同 PHP `placeTargets()`）。
fn place_targets(
    base: &Base,
    count: usize,
    target_type: TargetType,
    words: Option<Vec<String>>,
) -> Result<Vec<Target>> {
    let tolerance = base.ctx.config.captcha.tolerance.click;
    let spacing = 24.0f32.max(2.0 * tolerance);
    let pad = 12i32.max((base.width.min(base.height) / 5) as i32);

    let cols = (count as f32).sqrt().ceil() as usize;
    let rows = (count as f32 / cols as f32).ceil() as usize;
    let jitter = spacing / 3.0;
    let region_w = base.width as f32 - 2.0 * pad as f32;
    let region_h = base.height as f32 - 3.0 * pad as f32;
    if region_w < (cols - 1) as f32 * spacing + jitter || region_h < (rows - 1) as f32 * spacing + jitter
    {
        return Err(PosterError::Captcha(format!(
            "画布 {}x{} 放不下 {} 个点击目标（最小间距 {spacing:.0}px）：请放大到约 {} 或降低难度",
            base.width,
            base.height,
            count,
            minimum_canvas(cols, rows, spacing)
        )));
    }

    let mut slots: Vec<(usize, usize)> = (0..cols)
        .flat_map(|c| (0..rows).map(move |r| (c, r)))
        .collect();
    let mut rng = rand::rng();
    slots.shuffle(&mut rng);

    let offset_x =
        pad as f32 + rng.random_range(0..=(region_w - (cols - 1) as f32 * spacing).round() as i32) as f32;
    let offset_y =
        pad as f32 + rng.random_range(0..=(region_h - (rows - 1) as f32 * spacing).round() as i32) as f32;

    let pool = target_pool(base, count, target_type, words);
    Ok((0..count)
        .map(|i| Target {
            x: (offset_x + slots[i].0 as f32 * spacing).round() as i32,
            y: (offset_y + slots[i].1 as f32 * spacing).round() as i32,
            text: pool[i].clone(),
            order: i + 1,
            color: String::new(),
            angle: 0,
        })
        .collect())
}

/// 报给调用方的最小画布尺寸（正方形估算，仅用于错误提示，同 PHP `minimumCanvas()`）。
fn minimum_canvas(cols: usize, rows: usize, spacing: f32) -> String {
    let need_w = (cols - 1) as f32 * spacing + spacing / 3.0;
    let need_h = (rows - 1) as f32 * spacing + spacing / 3.0;
    let side = 60.max((5.0 * (need_w / 3.0).max(need_h / 2.0)).ceil() as i32);
    format!("{side}x{side}")
}

/// 目标文案池：icon 用形状名，text 用词表（自定义 > 配置 > 难度兜底），不足时循环取用。
fn target_pool(
    base: &Base,
    count: usize,
    target_type: TargetType,
    words: Option<Vec<String>>,
) -> Vec<String> {
    let mut pool: Vec<String> = if target_type == TargetType::Icon {
        SHAPES.iter().map(|s| (*s).to_string()).collect()
    } else {
        // 空数组走到兜底（PHP 用 ?: 而非 ??，避免取模除零）
        words
            .filter(|w| !w.is_empty())
            .or_else(|| {
                let configured = base.ctx.config.captcha.click_words.clone();
                (!configured.is_empty()).then_some(configured)
            })
            .unwrap_or_else(|| {
                match base.difficulty {
                    Difficulty::Easy => vec!["云", "风"],
                    Difficulty::Hard => vec!["星", "雨", "山", "火"],
                    Difficulty::Medium => vec!["云", "风", "山"],
                }
                .into_iter()
                .map(str::to_string)
                .collect()
            })
    };
    let mut rng = rand::rng();
    pool.shuffle(&mut rng);
    (0..count).map(|i| pool[i % pool.len()].clone()).collect()
}

/// 前端提示项：icon 模式附缩略图（与画布同一渲染路径，形状与角度一致）。
fn hints(targets: &[Target], target_type: TargetType) -> Result<Value> {
    let mut items = Vec::with_capacity(targets.len());
    for target in targets {
        let mut item = json!({ "text": target.text, "order": target.order });
        if target_type == TargetType::Icon {
            let icon = render_icon(&target.text, THUMB_COLOR, target.angle)?;
            item["thumb"] = json!(super::png_data_uri(&icon.encode("png", None)?));
        }
        items.push(item);
    }
    Ok(Value::Array(items))
}

/// 渲染单个图标：透明小画布 + 整体旋转；画布与缩略图共用，保证形状、角度一致。
fn render_icon(shape: &str, color: &str, angle: i32) -> Result<ImageDriver> {
    let mut icon = ImageDriver::create(
        (ICON_SIZE as u32) + ICON_PADDING,
        (ICON_SIZE as u32) + ICON_PADDING,
    )?;
    let (w, h) = icon.size();
    draw_shape(&mut icon, shape, (w / 2) as i32, (h / 2) as i32, ICON_SIZE, color)?;
    if angle != 0 {
        icon.rotate(angle as f32, "transparent")?;
    }
    Ok(icon)
}

/// 图元画出的形状，中心 `(cx, cy)`，`size` 为外接尺寸（同 PHP `drawShape()`）。
fn draw_shape(
    canvas: &mut ImageDriver,
    shape: &str,
    cx: i32,
    cy: i32,
    size: i32,
    color: &str,
) -> Result<()> {
    let r = size / 2;
    let stroke = (size / 8).max(2);
    let line = LineOptions {
        color: color.to_string(),
        width: stroke as u32,
    };
    let filled = ShapeOptions {
        color: color.to_string(),
        filled: true,
        ..Default::default()
    };
    let outline = ShapeOptions {
        filled: false,
        ..filled.clone()
    };

    match shape {
        "circle" => canvas.ellipse(cx, cy, r as f32, r as f32, &filled)?,
        "ring" => {
            for i in 0..stroke {
                let radius = r.saturating_sub(i).max(1) as f32;
                canvas.ellipse(cx, cy, radius, radius, &outline)?;
            }
        }
        "square" => canvas.rectangle(cx - r, cy - r, size as u32, size as u32, &filled)?,
        "rounded" => canvas.rectangle(
            cx - r,
            cy - r,
            size as u32,
            size as u32,
            &ShapeOptions {
                radius: (stroke * 2) as u32,
                ..filled.clone()
            },
        )?,
        "bar" => canvas.rectangle(
            cx - r,
            cy - size / 6,
            size as u32,
            (size / 3).max(4) as u32,
            &filled,
        )?,
        "cross" => {
            canvas.rectangle(cx - r, cy - stroke / 2, size as u32, stroke as u32, &filled)?;
            canvas.rectangle(cx - stroke / 2, cy - r, stroke as u32, size as u32, &filled)?;
        }
        "x" => {
            canvas.line(cx - r, cy - r, cx + r, cy + r, &line)?;
            canvas.line(cx + r, cy - r, cx - r, cy + r, &line)?;
        }
        "chevron" => {
            canvas.line(cx - r, cy - r, cx + r, cy, &line)?;
            canvas.line(cx + r, cy, cx - r, cy + r, &line)?;
        }
        "semicircle" => canvas.filled_arc(cx, cy, size as u32, size as u32, 180.0, 360.0, &filled)?,
        "wedge" => canvas.filled_arc(cx, cy, size as u32, size as u32, 40.0, 140.0, &filled)?,
        "asterisk" => {
            for i in 0..3 {
                let radian = (90.0 + 60.0 * i as f32).to_radians();
                canvas.line(
                    cx - (r as f32 * radian.cos()).round() as i32,
                    cy - (r as f32 * radian.sin()).round() as i32,
                    cx + (r as f32 * radian.cos()).round() as i32,
                    cy + (r as f32 * radian.sin()).round() as i32,
                    &line,
                )?;
            }
        }
        _ => canvas.ellipse(cx, cy, r as f32, r as f32, &filled)?,
    }
    Ok(())
}

/// 目标之上的补噪点：打断「按颜色阈值分离目标 → 直接读出坐标」的还原路径（PHP `drawOverlayNoise`）。
fn draw_overlay_noise(bg: &mut ImageDriver, targets: &[Target]) -> Result<()> {
    let mut rng = rand::rng();
    for target in targets {
        for _ in 0..4 {
            bg.ellipse(
                target.x + rng.random_range(-9..=9),
                target.y + rng.random_range(-9..=9),
                rng.random_range(1..=2) as f32,
                rng.random_range(1..=2) as f32,
                &ShapeOptions {
                    color: format!("{}55", super::random_color()),
                    filled: true,
                    ..Default::default()
                },
            )?;
        }
    }
    // 全图再撒一层：只覆盖目标附近反而等于标出目标位置
    for _ in 0..12 {
        bg.ellipse(
            rng.random_range(0..=bg.width().saturating_sub(1).max(1) as i32),
            rng.random_range(0..=bg.height().saturating_sub(1).max(1) as i32),
            1.0,
            1.0,
            &ShapeOptions {
                color: format!("{}3C", super::random_color()),
                filled: true,
                ..Default::default()
            },
        )?;
    }
    Ok(())
}

/// 随机色相 / 明度：恒定色会被按颜色分离精确还原坐标（PHP `randomTargetColor()`）。
fn random_target_color() -> String {
    let mut rng = rand::rng();
    let hue = rng.random_range(0..=359) as f32 / 60.0;
    let saturation = rng.random_range(65..=100) as f32 / 100.0;
    let value = rng.random_range(45..=75) as f32 / 100.0;
    let sector = (hue.floor() as i32).rem_euclid(6);
    let fraction = hue - hue.floor();
    let p = value * (1.0 - saturation);
    let q = value * (1.0 - saturation * fraction);
    let t = value * (1.0 - saturation * (1.0 - fraction));

    let (r, g, b) = match sector {
        1 => (q, value, p),
        2 => (p, value, t),
        3 => (p, q, value),
        4 => (t, p, value),
        5 => (value, p, q),
        _ => (value, t, p),
    };
    format!(
        "#{:02X}{:02X}{:02X}",
        (r * 255.0) as u8,
        (g * 255.0) as u8,
        (b * 255.0) as u8
    )
}

/// 随机透明度（8 位十六进制颜色后缀）：图元支持 alpha，文字不支持，故只用于图标目标。
fn random_alpha() -> String {
    format!("{:02X}", rand::rng().random_range(0xB0..=0xF0u8))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::captcha::test_ctx;

    fn base(difficulty: Difficulty, width: u32, height: u32) -> Base {
        Base::new(test_ctx(), CaptchaType::Click, difficulty, None, width, height)
    }

    #[test]
    fn difficulty_decides_target_count() {
        for (difficulty, expected) in [
            (Difficulty::Easy, 2),
            (Difficulty::Medium, 3),
            (Difficulty::Hard, 4),
        ] {
            let base = base(difficulty, 300, 200);
            let targets = place_targets(&base, expected, TargetType::Text, None).unwrap();
            assert_eq!(targets.len(), expected);
        }
    }

    #[test]
    fn targets_keep_minimum_spacing() {
        // 反复生成，检查任意两点间距 >= 2×容差（18px → 36px）
        for _ in 0..40 {
            let base = base(Difficulty::Medium, 300, 200);
            let targets = place_targets(&base, 3, TargetType::Text, None).unwrap();
            for i in 0..targets.len() {
                for j in (i + 1)..targets.len() {
                    let dx = (targets[i].x - targets[j].x) as f32;
                    let dy = (targets[i].y - targets[j].y) as f32;
                    assert!(
                        (dx * dx + dy * dy).sqrt() >= 36.0 - 1.0,
                        "目标过近: {:?} / {:?}",
                        targets[i],
                        targets[j]
                    );
                }
            }
        }
    }

    #[test]
    fn small_canvas_is_rejected_with_hint() {
        let base = base(Difficulty::Medium, 100, 100);
        let err = place_targets(&base, 3, TargetType::Text, None).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("放不下"), "{message}");
        assert!(message.contains('x'), "应给出建议尺寸: {message}");
    }

    #[test]
    fn pool_falls_back_and_cycles() {
        let base = base(Difficulty::Medium, 300, 200);
        // 自定义词池只有 2 个词，要凑 4 个目标时循环取用
        let pool = target_pool(&base, 4, TargetType::Text, Some(vec!["猫".into(), "狗".into()]));
        assert_eq!(pool.len(), 4);
        assert!(pool.iter().all(|w| w == "猫" || w == "狗"));

        // 空词池回落配置（默认 28 个词）
        let pool = target_pool(&base, 3, TargetType::Text, Some(vec![]));
        assert_eq!(pool.len(), 3);

        let icons = target_pool(&base, 4, TargetType::Icon, None);
        assert_eq!(icons.len(), 4);
        assert!(icons.iter().all(|name| SHAPES.contains(&name.as_str())));
    }

    #[test]
    fn shapes_draw_without_error_and_rotate() {
        for shape in SHAPES {
            let icon = render_icon(shape, "#FF0000FF", 12).unwrap();
            let opaque = icon.image().pixels().filter(|p| p.0[3] > 0).count();
            assert!(opaque > 20, "形状 {shape} 没画出东西");
        }
    }

    #[test]
    fn target_colors_are_valid_and_varied() {
        let colors: Vec<String> = (0..40).map(|_| random_target_color()).collect();
        for color in &colors {
            crate::drivers::color::parse(color).unwrap();
        }
        assert!(colors.iter().collect::<std::collections::HashSet<_>>().len() > 10);
        let alpha = random_alpha();
        assert!(crate::drivers::color::parse(&format!("#112233{alpha}")).is_ok());
    }
}
