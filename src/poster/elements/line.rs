//! 直线元素，对应 PHP `LineElement`。

use serde::{Deserialize, Serialize};

use crate::drivers::{ImageDriver, LineOptions};
use crate::error::Result;

use super::{ElementRender, RenderCtx};

/// 直线。
///
/// 终点缺省回落到 `x` / `y`（PHP 的 `x2 ?? x ?? 100`、`y2 ?? y ?? 0`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LineElement {
    pub x1: i32,
    pub y1: i32,
    /// 终点 x；`None` = 取 `x`，都没有则 100。
    pub x2: Option<i32>,
    /// 终点 y；`None` = 取 `y`，都没有则 0。
    pub y2: Option<i32>,
    /// 终点 x 的回落值。
    pub x: Option<i32>,
    /// 终点 y 的回落值。
    pub y: Option<i32>,
    /// `color` / `width`。
    #[serde(flatten)]
    pub style: LineOptions,
}

impl Default for LineElement {
    fn default() -> Self {
        Self {
            x1: 0,
            y1: 0,
            x2: None,
            y2: None,
            x: None,
            y: None,
            style: LineOptions::default(),
        }
    }
}

impl ElementRender for LineElement {
    fn render(&self, canvas: &mut ImageDriver, _ctx: &RenderCtx<'_>) -> Result<()> {
        let x2 = self.x2.or(self.x).unwrap_or(100);
        let y2 = self.y2.or(self.y).unwrap_or(0);
        canvas.line(self.x1, self.y1, x2, y2, &self.style)
    }
}
