//! 头像元素，对应 PHP `AvatarElement`：正方形缩放 + 圆形裁剪 + 边框。

use serde::{Deserialize, Serialize};

use crate::drivers::{ImageDriver, OverlayOptions, ShapeOptions};
use crate::error::Result;

use super::{ElementRender, RenderCtx, load_image, positive};

/// 头像。
///
/// 缺省圆形裁剪（`circle: false` 得到方图）；`border` 给色时先在底层画一圈更大的实心
/// 形状，头像盖上去即留出等宽边框。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AvatarElement {
    /// 头像图片路径。
    pub src: String,
    pub x: i32,
    pub y: i32,
    /// 边长（px，正方形）。
    pub size: u32,
    /// 是否圆形裁剪。
    pub circle: bool,
    /// 边框颜色；`None` / 空串 = 不画边框。
    pub border: Option<String>,
    /// 边框宽度（px），0 = 不画。
    pub border_width: u32,
    /// `radius`（`circle` 时被 `size / 2` 覆盖）/ `shadow` 等叠加选项。
    #[serde(flatten)]
    pub style: OverlayOptions,
}

impl Default for AvatarElement {
    fn default() -> Self {
        Self {
            src: String::new(),
            x: 0,
            y: 0,
            size: 80,
            circle: true,
            border: None,
            border_width: 2,
            style: OverlayOptions::default(),
        }
    }
}

impl ElementRender for AvatarElement {
    fn render(&self, canvas: &mut ImageDriver, ctx: &RenderCtx<'_>) -> Result<()> {
        let Some(mut img) = load_image(ctx, &self.src)? else {
            return Ok(());
        };
        let size = positive(self.size, "size")?;
        img.resize(size, size)?;

        let mut opts = self.style.clone();
        if self.circle {
            opts.radius = size / 2;
        }

        // 边框先画在底层，头像盖上去即留出等宽边
        if let Some(border) = self.border.as_deref() {
            let width = self.border_width;
            if !border.is_empty() && width > 0 {
                let border_opts = ShapeOptions {
                    color: border.to_string(),
                    filled: true,
                    ..Default::default()
                };
                if self.circle {
                    let radius = size / 2;
                    let r = (radius + width) as f32;
                    canvas.ellipse(
                        self.x + radius as i32,
                        self.y + radius as i32,
                        r,
                        r,
                        &border_opts,
                    )?;
                } else {
                    canvas.rectangle(
                        self.x - width as i32,
                        self.y - width as i32,
                        size + width * 2,
                        size + width * 2,
                        &border_opts,
                    )?;
                }
            }
        }

        canvas.overlay(&img, self.x, self.y, &opts)
    }
}
