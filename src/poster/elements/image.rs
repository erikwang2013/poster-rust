//! 图片元素，对应 PHP `ImageElement`：`src` + 叠加选项。

use serde::{Deserialize, Serialize};

use crate::drivers::{ImageDriver, OverlayOptions};
use crate::error::Result;

use super::{ElementRender, RenderCtx, load_image, positive};

/// 叠加一张图片。
///
/// 缺图（`src` 不存在）时按 `config.poster.placeholder` 画占位图，未配置则跳过。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct ImageElement {
    /// 图片路径。
    pub src: String,
    /// 目标位置 x（px）。
    pub x: i32,
    /// 目标位置 y（px）。
    pub y: i32,
    /// `width` / `height` / `radius` / `shadow`。
    #[serde(flatten)]
    pub style: OverlayOptions,
}


impl ElementRender for ImageElement {
    fn render(&self, canvas: &mut ImageDriver, ctx: &RenderCtx<'_>) -> Result<()> {
        let Some(img) = load_image(ctx, &self.src)? else {
            return Ok(());
        };
        // 尺寸兜底：<= 0 的 width/height 会让缩放静默什么都不画（PHP 同）
        if let Some(width) = self.style.width {
            positive(width, "width")?;
        }
        if let Some(height) = self.style.height {
            positive(height, "height")?;
        }
        canvas.overlay(&img, self.x, self.y, &self.style)
    }
}
