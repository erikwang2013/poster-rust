//! 水印元素，对应 PHP `WatermarkElement`：按间距在整张画布上平铺文字。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::drivers::{ImageDriver, TextAlign, TextOptions};
use crate::error::{PosterError, Result};

use super::{ElementRender, RenderCtx, positive};

/// 平铺瓦片数上限：间距过小会在画布上铺出天量文本（曾可被模板 JSON 触发的 CPU 打满）。
const MAX_TILES: u64 = 20_000;

/// 默认旋转角度（度）；与 [`TextOptions`] 的 0 不同，故本元素不直接复用其默认值。
fn default_angle() -> f32 {
    -30.0
}

/// 文字水印。
///
/// 选项键与驱动的 `text()` 一致，另加 `spacing_x` / `spacing_y` / `spacing`（snake_case，
/// 与 PHP 版逐字一致）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WatermarkElement {
    /// 水印文字；空串跳过绘制。
    pub text: String,
    /// 字体路径；`None` = 配置默认字体。
    pub font: Option<PathBuf>,
    pub size: f32,
    pub color: String,
    /// 旋转角度（度，逆时针为正），默认 -30。
    #[serde(default = "default_angle")]
    pub angle: f32,
    /// 自动换行宽度；0 = 不换行。
    pub max_width: f32,
    pub align: TextAlign,
    /// 行高；`None` = 字号 × 1.5。
    pub line_height: Option<f32>,
    /// 横纵间距的公共回落值。
    pub spacing: Option<u32>,
    /// 横向间距（px），缺省取 `spacing`，再缺省 150。
    #[serde(rename = "spacing_x")]
    pub spacing_x: Option<u32>,
    /// 纵向间距（px），缺省取 `spacing`，再缺省 100。
    #[serde(rename = "spacing_y")]
    pub spacing_y: Option<u32>,
}

impl Default for WatermarkElement {
    fn default() -> Self {
        Self {
            text: String::new(),
            font: None,
            size: 16.0,
            color: "#000000".into(),
            angle: default_angle(),
            max_width: 0.0,
            align: TextAlign::Left,
            line_height: None,
            spacing: None,
            spacing_x: None,
            spacing_y: None,
        }
    }
}

impl ElementRender for WatermarkElement {
    fn render(&self, canvas: &mut ImageDriver, _ctx: &RenderCtx<'_>) -> Result<()> {
        if self.text.is_empty() {
            return Ok(());
        }
        let (width, height) = canvas.size();
        if width == 0 || height == 0 {
            return Ok(());
        }

        let spacing_x = positive(self.spacing_x.or(self.spacing).unwrap_or(150), "spacing_x")?;
        let spacing_y = positive(self.spacing_y.or(self.spacing).unwrap_or(100), "spacing_y")?;

        let tiles = width.div_ceil(spacing_x) as u64 * height.div_ceil(spacing_y) as u64;
        if tiles > MAX_TILES {
            return Err(PosterError::Template(format!(
                "水印间距过小：{spacing_x}x{spacing_y} 会在 {width}x{height} 画布上绘制 {tiles} 个瓦片（上限 {MAX_TILES}）"
            )));
        }

        let opts = TextOptions {
            font: self.font.clone(),
            size: self.size,
            color: self.color.clone(),
            angle: self.angle,
            max_width: self.max_width,
            align: self.align,
            line_height: self.line_height,
        };
        let mut y = 0;
        while y < height {
            let mut x = 0;
            while x < width {
                canvas.text(&self.text, x as f32, y as f32, &opts)?;
                x += spacing_x;
            }
            y += spacing_y;
        }
        Ok(())
    }
}
