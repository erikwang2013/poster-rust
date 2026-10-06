//! 滑块验证码，对应 PHP `SliderCaptcha`。
//!
//! 从背景切出拼图块并在随机位置挖出缺口，用户把拼图块拖回缺口；校验按 **x** 比对，容差 ±4px。
//!
//! 形状两种：
//! - `square`：矩形缺口；
//! - `jigsaw`：凹凸拼图 —— 每边中点一个半径 `k = 短边/5`（hard 下 8px）的半圆，
//!   四边各自随机凸/凹（16 种组合）；**缺口与拼图块共用同一轮廓**，拼图块是外扩后的
//!   外接矩形 PNG（`puzzle_w/h` 仍是本体尺寸），轮廓外透明。
//!
//! 存的 `x`/`y` 是拼图块 PNG 的左上角（jigsaw 下即外扩矩形的左上角），
//! 前端把整块 PNG 放到 `(x, y)` 即与缺口对齐。

use rand::Rng;
use serde_json::json;

use crate::drivers::{ImageDriver, ShapeOptions};
use crate::error::{PosterError, Result};

use super::{Base, CaptchaBuilder, CaptchaResult, CaptchaType, Difficulty, SliderShape, png_data_uri};

/// 缺口/拼图块的颜色（半透明黑，同 PHP）。
const GAP_COLOR: &str = "#00000040";
/// jigsaw 轮廓半圆的分段数（同 PHP）。
const ARC_SEGMENTS: usize = 12;
/// 全图混淆噪点个数（同 PHP）。
const NOISE_COUNT: usize = 30;

/// 生成滑块验证码。
pub(crate) fn generate(builder: CaptchaBuilder) -> Result<CaptchaResult> {
    let difficulty = builder.difficulty();
    let shape = match builder.shape() {
        Some(shape) => shape,
        None => SliderShape::parse(&builder.ctx().config.captcha.slider_shape)?,
    };
    let (width, height) = builder.canvas_size();

    let mut base = Base::new(
        builder.ctx().clone(),
        CaptchaType::Slider,
        difficulty,
        builder.background(),
        width,
        height,
    );
    let mut bg = base.create_background()?;

    let (puzzle_w, puzzle_h) = match difficulty {
        Difficulty::Hard => (40u32, 40u32),
        _ => (50u32, 50u32),
    };

    // 画布过小时缺口会被钳到唯一位置（盲提交固定值即可通过）：明确拒绝，不做静默退化
    let (min_width, min_height) = (4 * puzzle_w, 2 * puzzle_h);
    if base.width < min_width || base.height < min_height {
        return Err(PosterError::Captcha(format!(
            "画布 {}x{} 放不下滑块验证码：至少需要 {}x{}（拼图 {}x{}），否则缺口位置可被穷举",
            base.width, base.height, min_width, min_height, puzzle_w, puzzle_h
        )));
    }

    // 边距随画布缩放；默认 300×200 + 50×50 拼图下等价于旧版固定值 xMin=50 / yMin=20
    let (width, height) = (base.width as i32, base.height as i32);
    let pad_x = (puzzle_w as i32).max(width / 6);
    let pad_y = ((puzzle_h / 3) as i32).max(height / 10);
    let x_min = pad_x;
    let x_max = (width - puzzle_w as i32 - pad_x).max(x_min);
    let y_min = pad_y;
    let y_max = (height - puzzle_h as i32 - pad_y).max(y_min);
    let mut rng = rand::rng();
    let puzzle_x = rng.random_range(x_min..=x_max);
    let puzzle_y = rng.random_range(y_min..=y_max);

    // 凸出/凹陷半径；缺口外接矩形比本体每边各多 knob（square 时为 0）。
    // 放置边距恒大于 knob（padX ≥ 拼图宽、padY ≥ 拼图高/3，均 > 短边/5），凸出不会越出画布
    let knob = match shape {
        SliderShape::Jigsaw => ((puzzle_w.min(puzzle_h) / 5) as i32).max(4),
        SliderShape::Square => 0,
    };

    // 先从背景切块（此时缺口还没挖）
    let mut piece = ImageDriver::from_image(bg.image().clone())?;
    match shape {
        SliderShape::Jigsaw => {
            // 四边凸/凹随机，破解方无法假设固定轮廓
            let tabs = [
                rng.random_bool(0.5),
                rng.random_bool(0.5),
                rng.random_bool(0.5),
                rng.random_bool(0.5),
            ];
            let points = jigsaw_points(puzzle_w as f32, puzzle_h as f32, knob as f32, tabs);

            // 拼图块：裁外接矩形 → 轮廓掩膜裁掉矩形外的部分
            let (bw, bh) = (
                puzzle_w as i32 + 2 * knob,
                puzzle_h as i32 + 2 * knob,
            );
            piece.crop(puzzle_x - knob, puzzle_y - knob, bw as u32, bh as u32)?;
            let mut mask = ImageDriver::create(bw as u32, bh as u32)?;
            mask.polygon(
                &points,
                &ShapeOptions {
                    color: "#FFFFFF".into(),
                    filled: true,
                    ..Default::default()
                },
            )?;
            piece.mask(&mask);

            // 缺口：同一轮廓平移到画布坐标（局部坐标下本体左上角在 (knob, knob)）
            let translated: Vec<(f32, f32)> = points
                .iter()
                .map(|(x, y)| (x + (puzzle_x - knob) as f32, y + (puzzle_y - knob) as f32))
                .collect();
            bg.polygon(
                &translated,
                &ShapeOptions {
                    color: GAP_COLOR.into(),
                    filled: true,
                    ..Default::default()
                },
            )?;
        }
        SliderShape::Square => {
            piece.crop(puzzle_x, puzzle_y, puzzle_w, puzzle_h)?;
            bg.rectangle(
                puzzle_x,
                puzzle_y,
                puzzle_w,
                puzzle_h,
                &ShapeOptions {
                    color: GAP_COLOR.into(),
                    filled: true,
                    ..Default::default()
                },
            )?;
        }
    }

    // 混淆：全图撒同色同尺寸噪点块，让「找最暗区域」的扫描无法唯一定位缺口
    for _ in 0..NOISE_COUNT {
        bg.ellipse(
            rng.random_range(0..bg.width()) as i32,
            rng.random_range(0..bg.height()) as i32,
            rng.random_range((puzzle_w / 3)..=(puzzle_w / 2)) as f32,
            rng.random_range((puzzle_h / 3)..=(puzzle_h / 2)) as f32,
            &ShapeOptions {
                color: GAP_COLOR.into(),
                filled: true,
                ..Default::default()
            },
        )?;
    }

    let bg_png = bg.encode("png", None)?;
    let piece_png = piece.encode("png", None)?;

    // jigsaw 下存的 x/y 是拼图块 PNG（含外扩）的左上角
    base.finish(
        &bg_png,
        json!({
            "puzzle": png_data_uri(&piece_png),
            "puzzle_w": puzzle_w,
            "puzzle_h": puzzle_h,
        }),
        json!({ "x": puzzle_x - knob, "y": puzzle_y - knob }),
    )
}

/// 凹凸拼图轮廓（局部坐标：外接矩形左上角为 `(0, 0)`，本体位于 `(k, k)-(k+w, k+h)`）。
///
/// `tabs` 为 `[上, 右, 下, 左]`，`true` = 外凸半圆、`false` = 内凹半圆。
/// 每边中点一个半径 `k` 的半圆；`k = 短边/5 < 边长/2`，轮廓闭合且不自交。
#[allow(clippy::type_complexity)] // 四条边的 (起点, 中点, 方向, 法线) 元组表
fn jigsaw_points(w: f32, h: f32, k: f32, tabs: [bool; 4]) -> Vec<(f32, f32)> {
    // [起点, 边中点, 沿边方向, 外法线]，顺时针绕行（图像坐标 y 向下）
    let sides: [((f32, f32), (f32, f32), (f32, f32), (f32, f32)); 4] = [
        ((k, k), (k + w / 2.0, k), (1.0, 0.0), (0.0, -1.0)),
        ((k + w, k), (k + w, k + h / 2.0), (0.0, 1.0), (1.0, 0.0)),
        ((k + w, k + h), (k + w / 2.0, k + h), (-1.0, 0.0), (0.0, 1.0)),
        ((k, k + h), (k, k + h / 2.0), (0.0, -1.0), (-1.0, 0.0)),
    ];

    let mut points = Vec::with_capacity(4 * (ARC_SEGMENTS + 2));
    for (i, (start, mid, dir, normal)) in sides.iter().enumerate() {
        points.push(*start);
        // t 从 0 到 π：沿 dir 进入半圆、绕到离开；凸取外法线、凹取反向
        let sign = if tabs[i] { 1.0 } else { -1.0 };
        for step in 0..=ARC_SEGMENTS {
            let t = std::f32::consts::PI * step as f32 / ARC_SEGMENTS as f32;
            points.push((
                mid.0 - dir.0 * k * t.cos() + normal.0 * sign * k * t.sin(),
                mid.1 - dir.1 * k * t.cos() + normal.1 * sign * k * t.sin(),
            ));
        }
    }
    points
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每条边中点（半圆顶点）在点列里的下标：每边 = 起点 1 个 + 半圆 ARC_SEGMENTS+1 个。
    fn side_mid(points: &[(f32, f32)], side: usize) -> (f32, f32) {
        points[side * (ARC_SEGMENTS + 2) + 1 + ARC_SEGMENTS / 2]
    }

    #[test]
    fn outline_is_closed_and_bulges_by_knob() {
        let points = jigsaw_points(50.0, 50.0, 10.0, [true, false, true, false]);
        assert_eq!(points.len(), 4 * (ARC_SEGMENTS + 2));

        // 上边外凸：顶点比本体边线高出一个 knob（y = k - k = 0）
        let top = side_mid(&points, 0);
        assert!((top.1 - 0.0).abs() < 0.01, "上边应外凸 10px, got {top:?}");
        // 右边内凹：顶点缩进一个 knob（x = k+w-k = 50）
        let right = side_mid(&points, 1);
        assert!((right.0 - 50.0).abs() < 0.01, "右边应内凹 10px, got {right:?}");
        // 下边外凸
        assert!((side_mid(&points, 2).1 - 70.0).abs() < 0.01);
        // 左边内凹：顶点朝本体里缩进一个 knob（x = k + k = 20）
        let left = side_mid(&points, 3);
        assert!((left.0 - 20.0).abs() < 0.01, "左边应内凹 10px, got {left:?}");

        // 轮廓不出外接矩形 [0, 70] × [0, 70]
        for (x, y) in &points {
            assert!((-0.01..=70.01).contains(x) && (-0.01..=70.01).contains(y), "越界 {x},{y}");
        }
    }

    #[test]
    fn four_tabs_give_four_distinct_directions() {
        let all_out = jigsaw_points(50.0, 50.0, 10.0, [true; 4]);
        let all_in = jigsaw_points(50.0, 50.0, 10.0, [false; 4]);
        assert_ne!(all_out, all_in);

        // 全凸：四条边的顶点都顶到外接矩形边界
        assert!(side_mid(&all_out, 0).1 < 0.01);
        assert!(side_mid(&all_out, 1).0 > 69.99);
        assert!(side_mid(&all_out, 2).1 > 69.99);
        assert!(side_mid(&all_out, 3).0 < 0.01);
        // 全凹：顶点朝本体里缩进一个 knob（上/左边线在内侧 +k，右/下边线在内侧 -k）
        assert!((side_mid(&all_in, 0).1 - 20.0).abs() < 0.01);
        assert!((side_mid(&all_in, 1).0 - 50.0).abs() < 0.01);
        assert!((side_mid(&all_in, 2).1 - 50.0).abs() < 0.01);
        assert!((side_mid(&all_in, 3).0 - 20.0).abs() < 0.01);
    }

    #[test]
    fn unknown_config_shape_is_an_error() {
        assert!(SliderShape::parse("triangle").is_err());
    }
}
