//! 艺术字元素，对应 PHP `ArtisticTextElement`：描边 / 阴影 / 渐变 / 霓虹四种样式。

use std::path::PathBuf;

use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

use crate::config;
use crate::drivers::{ImageDriver, OverlayOptions, TextAlign, TextOptions, color};
use crate::error::Result;

use super::{ElementRender, RenderCtx};

/// 艺术字。
///
/// `style` 未知时按普通文字画（同 PHP 的 `switch` 兜底分支，不再报错）。
/// 各样式只在用到时读取自己的键：`strokeColor` / `shadowOffsetX` / `color2` / `glowColor`…
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ArtisticTextElement {
    /// 文字内容；模板里也可写作 `content`。
    #[serde(alias = "content")]
    pub text: String,
    pub x: i32,
    pub y: i32,
    pub size: f32,
    /// 字体路径；`None` = 配置默认字体。
    pub font: Option<PathBuf>,
    /// `stroke` / `shadow` / `gradient` / `neon`。
    pub style: String,
    pub angle: f32,
    pub color: String,
    /// 自动换行宽度；0 = 不换行。
    pub max_width: f32,
    pub align: TextAlign,
    /// 描边色（`stroke`）。
    pub stroke_color: String,
    /// 描边粗细（px，`stroke`）。
    pub stroke_width: u32,
    /// 阴影色（`shadow`，8 位色带 alpha）。
    pub shadow_color: String,
    /// 阴影偏移 x（px，`shadow`）。
    pub shadow_offset_x: i32,
    /// 阴影偏移 y（px，`shadow`）。
    pub shadow_offset_y: i32,
    /// 渐变终点色（`gradient`）。
    pub color2: String,
    /// 霓虹光晕色（`neon`）；`None` = 用 `color`。
    pub glow_color: Option<String>,
}

impl Default for ArtisticTextElement {
    fn default() -> Self {
        Self {
            text: String::new(),
            x: 0,
            y: 0,
            size: 48.0,
            font: None,
            style: "stroke".into(),
            angle: 0.0,
            color: "#333333".into(),
            max_width: 0.0,
            align: TextAlign::Left,
            stroke_color: "#000000".into(),
            stroke_width: 1,
            shadow_color: "#00000033".into(),
            shadow_offset_x: 3,
            shadow_offset_y: 3,
            color2: "#FF6B6B".into(),
            glow_color: None,
        }
    }
}

impl ElementRender for ArtisticTextElement {
    fn render(&self, canvas: &mut ImageDriver, ctx: &RenderCtx<'_>) -> Result<()> {
        if self.text.is_empty() {
            return Ok(());
        }
        let base = TextOptions {
            font: Some(config::resolve_font(ctx.config, self.font.as_deref())),
            size: self.size,
            color: self.color.clone(),
            angle: self.angle,
            max_width: self.max_width,
            align: self.align,
            line_height: None,
        };

        match self.style.as_str() {
            "stroke" => self.render_stroke(canvas, &base),
            "shadow" => self.render_shadow(canvas, &base),
            "gradient" => self.render_gradient(canvas, ctx, &base),
            "neon" => self.render_neon(canvas, &base),
            _ => canvas.text(&self.text, self.x as f32, self.y as f32, &base),
        }
    }
}

impl ArtisticTextElement {
    /// 描边：先在四周各偏移 `strokeWidth` 画一圈描边色，再压上本色文字。
    fn render_stroke(&self, canvas: &mut ImageDriver, base: &TextOptions) -> Result<()> {
        let stroke = TextOptions {
            color: self.stroke_color.clone(),
            ..base.clone()
        };
        let width = self.stroke_width as i32;
        for ox in -width..=width {
            for oy in -width..=width {
                if ox == 0 && oy == 0 {
                    continue;
                }
                canvas.text(
                    &self.text,
                    (self.x + ox) as f32,
                    (self.y + oy) as f32,
                    &stroke,
                )?;
            }
        }
        canvas.text(&self.text, self.x as f32, self.y as f32, base)
    }

    /// 阴影：偏移位置先画阴影色，再压上本色文字。
    fn render_shadow(&self, canvas: &mut ImageDriver, base: &TextOptions) -> Result<()> {
        canvas.text(
            &self.text,
            (self.x + self.shadow_offset_x) as f32,
            (self.y + self.shadow_offset_y) as f32,
            &TextOptions {
                color: self.shadow_color.clone(),
                ..base.clone()
            },
        )?;
        canvas.text(&self.text, self.x as f32, self.y as f32, base)
    }

    /// 霓虹：三层放大的半透明光晕 + 白色主体。
    fn render_neon(&self, canvas: &mut ImageDriver, base: &TextOptions) -> Result<()> {
        let glow = self.glow_color.clone().unwrap_or_else(|| self.color.clone());
        for i in (1..=3).rev() {
            canvas.text(
                &self.text,
                self.x as f32,
                self.y as f32,
                &TextOptions {
                    size: self.size + (i * 2) as f32,
                    color: with_alpha(&glow, 20 * i),
                    ..base.clone()
                },
            )?;
        }
        canvas.text(
            &self.text,
            self.x as f32,
            self.y as f32,
            &TextOptions {
                color: "#FFFFFF".into(),
                ..base.clone()
            },
        )
    }

    /// 渐变：文字先画成白色掩膜，再按 y 逐像素上渐变色，最后整体合成。
    fn render_gradient(
        &self,
        canvas: &mut ImageDriver,
        ctx: &RenderCtx<'_>,
        base: &TextOptions,
    ) -> Result<()> {
        let font_path = config::resolve_font(ctx.config, self.font.as_deref());
        // 字体缺失时退回普通文字（同 PHP 的 `!is_file($font)` 分支）
        if !font_path.is_file() {
            return canvas.text(&self.text, self.x as f32, self.y as f32, base);
        }
        let font = canvas.font(&font_path)?.clone();

        let ascent = font.ascent(self.size).max(0.0);
        let descent = font.descent(self.size);
        let width = (font.measure(&self.text, self.size) + 8.0).ceil() as u32;
        let height = (ascent + descent + 8.0).ceil() as u32;
        if width == 0 || height == 0 {
            return Ok(());
        }

        // 透明底上画白色文字掩膜（左右/上下各留 4px，同 PHP）
        let mut mask = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 0]));
        font.draw_line(
            &mut mask,
            &self.text,
            4.0,
            ascent + 4.0,
            self.size,
            Rgba([255, 255, 255, 255]),
            0.0,
        );

        // 按 y 逐行上色；只处理不透明度过半的像素（PHP 的 GD alpha < 64）
        let from = color::parse(&self.color)?;
        let to = color::parse(&self.color2)?;
        let last_row = height.saturating_sub(1).max(1) as f32;
        for (_, py, pixel) in mask.enumerate_pixels_mut() {
            if pixel.0[3] < 128 {
                continue;
            }
            let ratio = py as f32 / last_row;
            for channel in 0..3 {
                pixel.0[channel] = (from.0[channel] as f32
                    + (to.0[channel] as f32 - from.0[channel] as f32) * ratio)
                    as u8;
            }
        }

        let temp = ImageDriver::from_image(mask)?;
        canvas.overlay(
            &temp,
            self.x - 4,
            self.y - descent as i32 - 4,
            &OverlayOptions::default(),
        )
    }
}

/// 追加 alpha 后缀（PHP `$glowColor . dechex(20 * $i)`）；拼不出合法颜色时原样返回。
fn with_alpha(color: &str, alpha: u32) -> String {
    let candidate = format!("{color}{alpha:02x}");
    if color::parse(&candidate).is_ok() {
        candidate
    } else {
        color.to_string()
    }
}
