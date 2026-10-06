//! 文字元素，对应 PHP `TextElement`：选项直接透传给驱动的 `text()`。

use serde::{Deserialize, Serialize};

use crate::drivers::{ImageDriver, TextOptions};
use crate::error::Result;

use super::{ElementRender, RenderCtx};

/// 文字。
///
/// 文本样式（`size` / `color` / `font` / `angle` / `maxWidth` / `align` / `lineHeight`）
/// 与驱动 [`TextOptions`] 同键同默认值，因此平铺进本结构体。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct TextElement {
    /// 文字内容；模板里也可写作 `content`（PHP 同时接受两个键，优先 `text`）。
    #[serde(alias = "content")]
    pub text: String,
    /// 文本基线锚点的 x（px）。
    pub x: i32,
    /// 文本基线锚点的 y（px，同 `imagettftext`）。
    pub y: i32,
    /// 字体、字号、颜色、换行、旋转等。
    #[serde(flatten)]
    pub style: TextOptions,
}


impl ElementRender for TextElement {
    fn render(&self, canvas: &mut ImageDriver, _ctx: &RenderCtx<'_>) -> Result<()> {
        canvas.text(&self.text, self.x as f32, self.y as f32, &self.style)
    }
}
