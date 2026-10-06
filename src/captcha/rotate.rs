//! 旋转验证码，对应 PHP `RotateCaptcha`。
//!
//! 随机旋转圆图 N 度（默认难度下调自 `[30, 200]`，可用 `set_angle_range` 再收窄），
//! 用户拖回正；校验按 **角度** 比对，容差 ±5°，且按圆周差取最短边（`359°` 与 `1°` 相差 2°）。

use rand::Rng;
use serde_json::json;

use crate::drivers::ImageDriver;
use crate::error::Result;

use super::{Base, CaptchaBuilder, CaptchaResult, CaptchaType, Difficulty};

/// GD `IMG_FILTER_CONTRAST` 的强度（同 PHP）。
const CONTRAST: f32 = 12.0;

/// 生成旋转验证码。
pub(crate) fn generate(builder: CaptchaBuilder) -> Result<CaptchaResult> {
    let difficulty = builder.difficulty();
    let size = builder.rotate_size();
    let (min_angle, max_angle) = builder.angle_range();

    let mut base = Base::new(
        builder.ctx().clone(),
        CaptchaType::Rotate,
        difficulty,
        builder.background(),
        size,
        size,
    );
    let mut bg = base.create_background()?;
    contrast(&mut bg, CONTRAST);

    // 按难度调节角度范围；set_angle_range 的上下限优先，并保证 lo <= hi（同 PHP）
    let (range_min, range_max) = match difficulty {
        Difficulty::Easy => (10, 90),
        Difficulty::Hard => (90, 330),
        Difficulty::Medium => (30, 200),
    };
    let lo = (min_angle as i32).max(range_min);
    let hi = lo.max((max_angle as i32).min(range_max));
    let angle = rand::rng().random_range(lo..=hi) as f32;

    bg.rotate(angle, "transparent")?;
    let (rotated_w, rotated_h) = bg.size();
    // 背景图比圆图小时偏移可能为负：用 i32 截断除法，同 PHP intval()
    bg.crop(
        (rotated_w as i32 - size as i32) / 2,
        (rotated_h as i32 - size as i32) / 2,
        size,
        size,
    )?;
    bg.circle(size)?;

    let png = bg.encode("png", None)?;
    base.finish(
        &png,
        json!({}),
        json!({
            "angle": angle,
            "orig_size": { "width": size, "height": size },
        }),
    )
}

/// 对比度增强：绕中灰线性拉伸，`c = (1 + amount/100)²`。
///
/// PHP 侧走 GD 的 `IMG_FILTER_CONTRAST`；这里用等价形态的线性拉伸近似，
/// 具体 LUT 未逐位复刻（属于观感差异，不影响校验口径）。
fn contrast(driver: &mut ImageDriver, amount: f32) {
    if amount == 0.0 {
        return;
    }
    let factor = (1.0 + amount / 100.0).powi(2);
    for pixel in driver.image_mut().pixels_mut() {
        for channel in 0..3 {
            let v = (pixel.0[channel] as f32 / 255.0 - 0.5) * factor + 0.5;
            pixel.0[channel] = (v * 255.0).clamp(0.0, 255.0).round() as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::captcha::test_ctx;
    use crate::drivers::ShapeOptions;

    #[test]
    fn difficulty_narrows_the_angle_range() {
        // 直接验证区间公式：medium 默认 = [30, 200]
        let pick = |difficulty: Difficulty, min: f32, max: f32| {
            let (range_min, range_max) = match difficulty {
                Difficulty::Easy => (10, 90),
                Difficulty::Hard => (90, 330),
                Difficulty::Medium => (30, 200),
            };
            let lo = (min as i32).max(range_min);
            let hi = lo.max((max as i32).min(range_max));
            (lo, hi)
        };
        assert_eq!(pick(Difficulty::Medium, 30.0, 330.0), (30, 200));
        assert_eq!(pick(Difficulty::Easy, 30.0, 330.0), (30, 90));
        assert_eq!(pick(Difficulty::Hard, 200.0, 330.0), (200, 330));
        // 用户范围与难度范围不相交时退化成单点，不会 panic
        assert_eq!(pick(Difficulty::Hard, 10.0, 20.0), (90, 90));
    }

    #[test]
    fn contrast_keeps_mid_gray_and_widens_range() {
        let mut image = ImageDriver::filled(2, 1, "#000000").unwrap();
        image
            .rectangle(
                1,
                0,
                1,
                1,
                &ShapeOptions {
                    color: "#FFFFFF".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        contrast(&mut image, CONTRAST);
        // 纯黑更黑（仍钳在 0）、纯白仍为白
        assert_eq!(image.image().get_pixel(0, 0).0[0], 0);
        assert_eq!(image.image().get_pixel(1, 0).0[0], 255);

        let mut mid = ImageDriver::filled(1, 1, "#808080").unwrap();
        contrast(&mut mid, 0.0); // amount 0 = 不动
        assert_eq!(mid.image().get_pixel(0, 0).0[0], 128);
    }

    #[test]
    fn base_starts_from_the_requested_size() {
        let base = Base::new(test_ctx(), CaptchaType::Rotate, Difficulty::Medium, None, 200, 200);
        assert_eq!((base.width, base.height), (200, 200));
    }
}
