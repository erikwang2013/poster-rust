//! 形状元素，对应 PHP `ShapeElement`：矩形与圆。

use serde::{Deserialize, Serialize};

use crate::drivers::{ImageDriver, ShapeOptions};
use crate::error::Result;

use super::{ElementRender, RenderCtx, positive};

/// 矩形 / 圆形。
///
/// `shape` 非 `circle` 一律按矩形画（同 PHP 的 `else` 分支）。`radius` 一把键两用：
/// 画圆时是半径、画矩形时是圆角——与 PHP 一致（两边都从同一个选项键取值）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ShapeElement {
    /// `rect`（默认）或 `circle`。
    pub shape: String,
    pub x: i32,
    pub y: i32,
    /// 圆心 x；`None` = 取 `x`（仅 `circle`）。
    pub cx: Option<i32>,
    /// 圆心 y；`None` = 取 `y`（仅 `circle`）。
    pub cy: Option<i32>,
    /// 宽（px，矩形）。
    pub width: u32,
    /// 高（px，矩形）。
    pub height: u32,
    /// 圆半径（`circle`）/ 圆角半径（`rect`）；`None` = 按 `size` → `width / 2` 推导（仅画圆时）。
    pub radius: Option<u32>,
    /// 圆半径的次选来源。
    pub size: Option<u32>,
    pub color: String,
    /// `false` = 只描边（圆角仅填充时生效，与驱动一致）。
    pub filled: bool,
    /// 透明度 0-1（0-100 也接受）。
    pub opacity: Option<f32>,
}

impl Default for ShapeElement {
    fn default() -> Self {
        Self {
            shape: "rect".into(),
            x: 0,
            y: 0,
            cx: None,
            cy: None,
            width: 100,
            height: 100,
            radius: None,
            size: None,
            color: "#FFFFFF".into(),
            filled: true,
            opacity: None,
        }
    }
}

impl ElementRender for ShapeElement {
    fn render(&self, canvas: &mut ImageDriver, _ctx: &RenderCtx<'_>) -> Result<()> {
        let opts = ShapeOptions {
            color: self.color.clone(),
            radius: self.radius.unwrap_or(0),
            filled: self.filled,
            opacity: self.opacity,
        };
        if self.shape == "circle" {
            let cx = self.cx.unwrap_or(self.x);
            let cy = self.cy.unwrap_or(self.y);
            let radius = match (self.radius, self.size) {
                (Some(radius), _) => positive(radius, "radius")?,
                (None, Some(size)) => positive(size, "size")?,
                // 文档用 width/height 描述圆形外接框，故 width/2 是第三来源（默认 100 → 50）
                (None, None) => positive(self.width / 2, "width")?,
            };
            canvas.ellipse(cx, cy, radius as f32, radius as f32, &opts)
        } else {
            let width = positive(self.width, "width")?;
            let height = positive(self.height, "height")?;
            canvas.rectangle(self.x, self.y, width, height, &opts)
        }
    }
}
