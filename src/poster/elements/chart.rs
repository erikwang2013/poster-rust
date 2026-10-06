//! 图表元素，对应 PHP `ChartElement`：柱状 / 折线 / 饼图。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config;
use crate::drivers::{ImageDriver, LineOptions, ShapeOptions, TextAlign, TextOptions};
use crate::error::{PosterError, Result};

use super::{ElementRender, RenderCtx, cell_text, positive};

/// 支持的图表类型（严格小写，写错不静默画成柱状图）。
pub const TYPES: [&str; 3] = ["bar", "pie", "line"];

/// 默认调色板：`colors` 缺省 / 空数组时使用。
const DEFAULT_COLORS: [&str; 6] = [
    "#FF6B6B", "#4ECDC4", "#45B7D1", "#96CEB4", "#FFEAA7", "#DDA0DD",
];

/// 图表。
///
/// 子类型存在 `chart` 键（不是模板里的元素类型键 `type`），与 PHP 一致：
/// 同一份 JSON 既能当模板元素定义，也能被 PHP 的元素 `toArray()` 还原。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ChartElement {
    /// 图表子类型：`bar`（默认）/ `pie` / `line`。
    pub chart: String,
    /// 数据项：数字，或 `{"label": "一月", "value": 12}`。
    pub data: Vec<Value>,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    /// 调色板；空 = 默认六色。
    pub colors: Vec<String>,
    /// 坐标轴与边界内边距（px）。
    pub padding: u32,
}

impl Default for ChartElement {
    fn default() -> Self {
        Self {
            chart: "bar".into(),
            data: Vec::new(),
            x: 0,
            y: 0,
            width: 600,
            height: 400,
            colors: Vec::new(),
            padding: 40,
        }
    }
}

impl ElementRender for ChartElement {
    fn render(&self, canvas: &mut ImageDriver, ctx: &RenderCtx<'_>) -> Result<()> {
        // `chart` 为元素类型名（模板里少写子类型）时按缺省柱状图处理，同 PHP
        let chart_type = if self.chart.is_empty() || self.chart == "chart" {
            "bar"
        } else {
            self.chart.as_str()
        };
        if !TYPES.contains(&chart_type) {
            return Err(PosterError::Template(format!(
                "未知图表类型 \"{chart_type}\"。已知类型: {}",
                TYPES.join(", ")
            )));
        }
        let width = positive(self.width, "width")?;
        let height = positive(self.height, "height")?;
        let font = config::resolve_font(ctx.config, None);

        match chart_type {
            "pie" => self.draw_pie(canvas, width, height, font),
            "line" => self.draw_line_chart(canvas, width, height, font),
            _ => self.draw_bar(canvas, width, height, font),
        }
    }
}

impl ChartElement {
    /// 取第 `index` 个颜色：`colors` 为空时回落默认调色板（取模，除数恒 > 0）。
    fn color(&self, index: usize) -> &str {
        if self.colors.is_empty() {
            DEFAULT_COLORS[index % DEFAULT_COLORS.len()]
        } else {
            &self.colors[index % self.colors.len()]
        }
    }

    fn draw_bar(
        &self,
        canvas: &mut ImageDriver,
        width: u32,
        height: u32,
        font: PathBuf,
    ) -> Result<()> {
        let padding = self.padding as i32;
        let count = self.data.len();
        if count == 0 {
            return Ok(());
        }

        let slot = (width as i32 - padding * 2) / count as i32;
        let bar_w = (slot - 10).max(0) as u32;
        let max_val = max_value(&self.data);
        let chart_h = height as f64 - (padding * 2) as f64;
        let axis_y = self.y + height as i32 - padding;

        let axis = LineOptions {
            color: "#CCCCCC".into(),
            ..Default::default()
        };
        canvas.line(self.x + padding, self.y + padding, self.x + padding, axis_y, &axis)?;
        canvas.line(
            self.x + padding,
            axis_y,
            self.x + width as i32 - padding,
            axis_y,
            &axis,
        )?;

        for (i, item) in self.data.iter().enumerate() {
            let value = value_of(item);
            let label = label_of(item);
            let bar_h = ((value / max_val) * chart_h) as i32;
            let bx = self.x + padding + i as i32 * slot + 5;
            let by = axis_y - bar_h;
            let color = self.color(i);

            canvas.rectangle(
                bx,
                by,
                bar_w,
                bar_h.max(0) as u32,
                &ShapeOptions {
                    color: color.to_string(),
                    filled: true,
                    ..Default::default()
                },
            )?;

            canvas.text(
                &format_value(value),
                (bx + bar_w as i32 / 2) as f32,
                (by - 5) as f32,
                &label_opts(12.0, "#333333", &font),
            )?;
            if !label.is_empty() {
                canvas.text(
                    &label,
                    (bx + bar_w as i32 / 2) as f32,
                    (axis_y + 18) as f32,
                    &label_opts(11.0, "#666666", &font),
                )?;
            }
        }
        Ok(())
    }

    fn draw_line_chart(
        &self,
        canvas: &mut ImageDriver,
        width: u32,
        height: u32,
        font: PathBuf,
    ) -> Result<()> {
        let line_color = self.color(0).to_string();
        let padding = self.padding as i32;
        let count = self.data.len();
        if count < 2 {
            return Ok(());
        }

        let max_val = max_value(&self.data);
        let chart_h = height as f64 - (padding * 2) as f64;
        let chart_w = width as i32 - padding * 2;
        let axis_y = self.y + height as i32 - padding;
        let step_x = chart_w / (count as i32 - 1);

        let axis = LineOptions {
            color: "#CCCCCC".into(),
            ..Default::default()
        };
        canvas.line(self.x + padding, self.y + padding, self.x + padding, axis_y, &axis)?;
        canvas.line(
            self.x + padding,
            axis_y,
            self.x + width as i32 - padding,
            axis_y,
            &axis,
        )?;

        let grid = LineOptions {
            color: "#EEEEEE".into(),
            ..Default::default()
        };
        for g in 1..=4 {
            let gy = axis_y - ((g as f64 / 4.0) * chart_h) as i32;
            canvas.line(
                self.x + padding,
                gy,
                self.x + width as i32 - padding,
                gy,
                &grid,
            )?;
        }

        let mut points = Vec::with_capacity(count);
        for (i, item) in self.data.iter().enumerate() {
            let value = value_of(item);
            let px = self.x + padding + i as i32 * step_x;
            let py = axis_y - ((value / max_val) * chart_h) as i32;
            points.push((px, py));

            canvas.ellipse(
                px,
                py,
                4.0,
                4.0,
                &ShapeOptions {
                    color: line_color.clone(),
                    filled: true,
                    ..Default::default()
                },
            )?;

            let label = label_of(item);
            if !label.is_empty() {
                canvas.text(
                    &label,
                    px as f32,
                    (axis_y + 18) as f32,
                    &label_opts(11.0, "#666666", &font),
                )?;
            }
        }

        let segment = LineOptions {
            color: line_color,
            width: 2,
        };
        for pair in points.windows(2) {
            canvas.line(pair[0].0, pair[0].1, pair[1].0, pair[1].1, &segment)?;
        }
        Ok(())
    }

    fn draw_pie(
        &self,
        canvas: &mut ImageDriver,
        width: u32,
        height: u32,
        font: PathBuf,
    ) -> Result<()> {
        let total: f64 = self.data.iter().map(value_of).sum();
        if total <= 0.0 {
            return Ok(());
        }

        let cx = self.x + width as i32 / 2;
        let cy = self.y + height as i32 / 2;
        let radius = (width.min(height) / 2) as i32 - 10;
        if radius <= 0 {
            return Ok(());
        }
        let radius = radius as f64;

        let count = self.data.len();
        let mut start = -90.0f64;
        let mut assigned = 0.0f64;

        for (idx, item) in self.data.iter().enumerate() {
            let value = value_of(item);
            let label = label_of(item);

            let slice = if idx == count - 1 {
                360.0 - assigned
            } else {
                ((value / total) * 360.0).round()
            };
            assigned += slice;
            if slice <= 0.0 {
                continue;
            }

            let color = self.color(idx);
            canvas.filled_arc(
                cx,
                cy,
                (radius * 2.0) as u32,
                (radius * 2.0) as u32,
                start as f32,
                (start + slice) as f32,
                &ShapeOptions {
                    color: color.to_string(),
                    filled: true,
                    ..Default::default()
                },
            )?;

            if !label.is_empty() {
                let mid = (start + slice / 2.0).to_radians();
                let lx = cx + (mid.cos() * (radius + 25.0)) as i32;
                let ly = cy + (mid.sin() * (radius + 25.0)) as i32;
                canvas.text(&label, lx as f32, ly as f32, &label_opts(10.0, "#333333", &font))?;
            }

            start += slice;
        }
        Ok(())
    }
}

/// 图例文字选项：字号 + 颜色 + 居中 + 默认字体。
fn label_opts(size: f32, color: &str, font: &Path) -> TextOptions {
    TextOptions {
        font: Some(font.to_path_buf()),
        size,
        color: color.to_string(),
        align: TextAlign::Center,
        ..Default::default()
    }
}

/// 数据项取值：对象取 `value`，标量直接用；非数值按 0（PHP 的数值强转语义）。
fn value_of(item: &Value) -> f64 {
    let raw = match item {
        Value::Object(map) => map.get("value").cloned().unwrap_or(Value::Null),
        other => other.clone(),
    };
    match raw {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::String(s) => s.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// 数据项标签：只对对象形式取 `label`（同 PHP）。
fn label_of(item: &Value) -> String {
    match item {
        Value::Object(map) => map.get("label").map(cell_text).unwrap_or_default(),
        _ => String::new(),
    }
}

/// 最大值兜底：全 0 时用 1，避免除零（PHP 的 `?: 1`）。
fn max_value(data: &[Value]) -> f64 {
    let max = data.iter().map(value_of).fold(f64::NEG_INFINITY, f64::max);
    if !max.is_finite() || max == 0.0 {
        1.0
    } else {
        max
    }
}

/// 数值标签文本：整数不带小数点（同 PHP 的 `(string)` 强转）。
fn format_value(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}
