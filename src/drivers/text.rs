//! TTF 文字：测量、自动换行（CJK 逐字 / 拉丁按词）、绘制（含旋转）。
//!
//! 对应 PHP 版 `Drivers/TextTrait` + `GdDriver::text()`：
//! - 换行规则一致：含 CJK 按字符切、否则按空白切词；断点用累加宽度，行尾用整行实测校正。
//! - `y` 是基线位置（同 `imagettftext`），`align` 时 `x` 是中/右锚点。

use std::collections::HashMap;
use std::path::Path;

use ab_glyph::{Font as _, FontArc, PxScale, ScaleFont as _};
use image::{Rgba, RgbaImage};
use imageproc::drawing::draw_text_mut;
use imageproc::geometric_transformations::{Interpolation, rotate_about_center};

use crate::error::{PosterError, Result};

/// 已加载的字体（内部 `FontArc`，可廉价克隆/共享）。
#[derive(Clone)]
pub struct Font {
    inner: FontArc,
}

/// 行高缺省比例，与 PHP 一致：`size * 1.5`。
pub const DEFAULT_LINE_HEIGHT_RATIO: f32 = 1.5;

impl Font {
    /// 从文件加载字体。
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path)
            .map_err(|e| PosterError::Font(format!("字体不可读 {}: {e}", path.display())))?;
        Self::from_bytes(bytes)
    }

    /// 从字节加载字体。
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        FontArc::try_from_vec(bytes)
            .map(|inner| Self { inner })
            .map_err(|e| PosterError::Font(format!("字体解析失败: {e}")))
    }

    /// 单行文本的推进宽度（px）。
    pub fn measure(&self, text: &str, size: f32) -> f32 {
        let scaled = self.inner.as_scaled(PxScale::from(size));
        text.chars()
            .map(|c| scaled.h_advance(scaled.glyph_id(c)))
            .sum()
    }

    /// 基线以上的上升高度（px，正数）。
    pub fn ascent(&self, size: f32) -> f32 {
        self.inner.as_scaled(PxScale::from(size)).ascent()
    }

    /// 基线以下的下降高度（px，正数）。
    pub fn descent(&self, size: f32) -> f32 {
        -self.inner.as_scaled(PxScale::from(size)).descent()
    }

    /// 自动换行；`max_width <= 0` 时仅按 `\n` 分段。
    pub fn wrap(&self, text: &str, size: f32, max_width: f32) -> Vec<String> {
        if max_width <= 0.0 {
            return text.split('\n').map(str::to_string).collect();
        }
        let mut lines = Vec::new();
        let mut cache: HashMap<String, f32> = HashMap::new();
        for paragraph in text.split('\n') {
            lines.extend(self.wrap_paragraph(paragraph, size, max_width, &mut cache));
        }
        if lines.is_empty() {
            lines.push(text.to_string());
        }
        lines
    }

    fn wrap_paragraph(
        &self,
        paragraph: &str,
        size: f32,
        max_width: f32,
        cache: &mut HashMap<String, f32>,
    ) -> Vec<String> {
        let width_of = |cache: &mut HashMap<String, f32>, token: &str| -> f32 {
            *cache
                .entry(token.to_string())
                .or_insert_with(|| self.measure(token, size))
        };

        let mut lines: Vec<String> = Vec::new();
        let mut current: Vec<String> = Vec::new();
        let mut current_width = 0.0f32;

        for token in split_tokens(paragraph) {
            if token.is_empty() {
                continue;
            }
            let token_width = width_of(cache, &token);
            if !current.is_empty() && current_width + token_width > max_width {
                // 行尾实测校正：累加宽度与整行测量有偏差（字距），保证每行不超 max_width
                let mut carry: Vec<String> = Vec::new();
                loop {
                    let joined = current.join("");
                    let exact = width_of(cache, &joined);
                    if current.len() > 1 && exact > max_width {
                        carry.insert(0, current.pop().unwrap());
                    } else {
                        break;
                    }
                }
                lines.push(current.join(""));
                current = carry;
                current_width = current.iter().map(|t| width_of(cache, t)).sum();
            }
            current.push(token);
            current_width += token_width;
        }

        // 收尾：末行同样校正
        while !current.is_empty() {
            let mut carry: Vec<String> = Vec::new();
            loop {
                let joined = current.join("");
                let exact = width_of(cache, &joined);
                if current.len() > 1 && exact > max_width {
                    carry.insert(0, current.pop().unwrap());
                } else {
                    break;
                }
            }
            lines.push(current.join(""));
            current = carry;
        }

        lines
    }

    /// 画一行文本；`(x, baseline_y)` 是基线锚点，`angle` 为逆时针度数（0 = 不旋转）。
    pub fn draw_line(
        &self,
        img: &mut RgbaImage,
        text: &str,
        x: f32,
        baseline_y: f32,
        size: f32,
        color: Rgba<u8>,
        angle: f32,
    ) {
        if text.is_empty() {
            return;
        }
        if angle.abs() < 0.001 {
            let y_top = baseline_y - self.ascent(size);
            draw_text_mut(
                img,
                color,
                x.round() as i32,
                y_top.round() as i32,
                PxScale::from(size),
                &self.inner,
                text,
            );
            return;
        }

        // 旋转：文本画在以锚点为中心的正方形临时画布上，绕中心旋转后贴回。
        let w = self.measure(text, size);
        let asc = self.ascent(size).max(0.0);
        let desc = self.descent(size);
        let radius = (w * w + asc.max(desc) * asc.max(desc)).sqrt().ceil() + 8.0;
        let side = (radius * 2.0).ceil().max(2.0) as u32;
        let mut temp = RgbaImage::from_pixel(side, side, Rgba([0, 0, 0, 0]));
        let anchor = (side / 2) as f32;
        draw_text_mut(
            &mut temp,
            color,
            anchor.round() as i32,
            (anchor - asc).round() as i32,
            PxScale::from(size),
            &self.inner,
            text,
        );
        let rotated = rotate_about_center(
            &temp,
            angle.to_radians(),
            Interpolation::Bilinear,
            Rgba([0, 0, 0, 0]),
        );
        let dx = (x - anchor).round() as i64;
        let dy = (baseline_y - anchor).round() as i64;
        image::imageops::overlay(img, &rotated, dx, dy);
    }
}

/// 切分换行 token：含 CJK 逐字，否则按空白切词（保留空白 token，与 PHP 一致）。
fn split_tokens(text: &str) -> Vec<String> {
    if text.chars().any(is_cjk) {
        return text.chars().map(|c| c.to_string()).collect();
    }
    let mut tokens = Vec::new();
    let mut buf = String::new();
    let mut in_space = false;
    for c in text.chars() {
        let is_space = c.is_whitespace();
        if !buf.is_empty() && is_space != in_space {
            tokens.push(std::mem::take(&mut buf));
        }
        in_space = is_space;
        buf.push(c);
    }
    if !buf.is_empty() {
        tokens.push(buf);
    }
    tokens
}

fn is_cjk(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_font() -> Font {
        Font::load(&crate::assets::default_font_path()).expect("默认字体应能加载")
    }

    #[test]
    fn measures_wider_text_as_wider() {
        let f = test_font();
        assert!(f.measure("海报", 40.0) > f.measure("海", 40.0));
        assert!(f.measure("", 40.0) == 0.0);
    }

    #[test]
    fn cjk_wraps_per_character() {
        let f = test_font();
        let lines = f.wrap("海报生成工具包", 40.0, 85.0);
        assert!(lines.len() >= 2, "CJK 应按字断行: {lines:?}");
        for line in &lines {
            assert!(f.measure(line, 40.0) <= 85.0 + 1.0, "行宽超限: {line}");
        }
        // 内容不丢
        assert_eq!(lines.concat(), "海报生成工具包");
    }

    #[test]
    fn latin_wraps_per_word_and_keeps_spaces() {
        let f = test_font();
        let lines = f.wrap("hello world poster", 20.0, f.measure("hello wor", 20.0));
        assert_eq!(lines, vec!["hello ", "world ", "poster"]);
    }

    #[test]
    fn explicit_newlines_are_kept() {
        let f = test_font();
        let lines = f.wrap("上\n下", 20.0, 0.0);
        assert_eq!(lines, vec!["上", "下"]);
    }
}
