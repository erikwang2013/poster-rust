//! 表格元素，对应 PHP `TableElement`：表头 + 斑马纹数据行 + 行底线。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config;
use crate::drivers::{ImageDriver, ShapeOptions, TextOptions};
use crate::error::Result;

use super::{ElementRender, RenderCtx, cell_text, text_align};

/// 表格。
///
/// 选项键同时接受文档里的 camelCase（`header` / `columns` / `headerBg`…）与历史
/// snake_case（`headers` / `col_widths` / `header_bg`…），与 PHP 版一致；导出统一写 camelCase。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TableElement {
    pub x: i32,
    pub y: i32,
    /// 表头单元格；别名 `headers`。
    #[serde(alias = "headers")]
    pub header: Vec<Value>,
    /// 数据行（二维）。
    pub rows: Vec<Vec<Value>>,
    /// 各列宽（px）；空 = 按 `width` 均分。别名 `col_widths`。
    #[serde(alias = "col_widths")]
    pub columns: Vec<u32>,
    /// 表格总宽（`columns` 为空时才用到）。
    pub width: u32,
    /// 表头行高。别名 `header_height`。
    #[serde(alias = "header_height")]
    pub header_height: u32,
    /// 数据行高。别名 `row_height`。
    #[serde(alias = "row_height")]
    pub row_height: u32,
    /// 单元格左右内边距。别名 `cell_padding`。
    #[serde(alias = "cell_padding")]
    pub cell_padding: u32,
    /// 表头背景色。别名 `header_bg`。
    #[serde(alias = "header_bg")]
    pub header_bg: String,
    /// 斑马纹：`["偶", "奇"]` 两色，优先于 `even_bg` / `odd_bg`。
    pub row_bg: Vec<String>,
    /// 数据行偶数行背景（0 起算）；默认 `#FAFAFA`。
    #[serde(rename = "even_bg")]
    pub even_bg: Option<String>,
    /// 数据行奇数行背景；默认 `#FFFFFF`。
    #[serde(rename = "odd_bg")]
    pub odd_bg: Option<String>,
    /// 字号。别名 `font_size`。
    #[serde(alias = "font_size")]
    pub font_size: u32,
    /// 字体路径；`None` = 配置默认字体。
    pub font: Option<PathBuf>,
    /// 表头文字色。别名 `header_color`。
    #[serde(alias = "header_color")]
    pub header_color: String,
    /// 数据行文字色。别名 `row_color`。
    #[serde(alias = "row_color")]
    pub row_color: String,
    /// 行底线颜色。别名 `border_color`。
    #[serde(alias = "border_color")]
    pub border_color: String,
    /// 逐列对齐：`left` / `center` / `right`，缺省 `left`。
    pub alignments: Vec<String>,
}

impl Default for TableElement {
    fn default() -> Self {
        Self {
            x: 0,
            y: 0,
            header: Vec::new(),
            rows: Vec::new(),
            columns: Vec::new(),
            width: 600,
            header_height: 40,
            row_height: 35,
            cell_padding: 10,
            header_bg: "#F5F5F5".into(),
            row_bg: Vec::new(),
            even_bg: None,
            odd_bg: None,
            font_size: 14,
            font: None,
            header_color: "#333333".into(),
            row_color: "#666666".into(),
            border_color: "#EEEEEE".into(),
            alignments: Vec::new(),
        }
    }
}

impl ElementRender for TableElement {
    fn render(&self, canvas: &mut ImageDriver, ctx: &RenderCtx<'_>) -> Result<()> {
        if self.header.is_empty() || self.rows.is_empty() {
            return Ok(());
        }

        let columns = if self.columns.is_empty() {
            let each = self.width / self.header.len() as u32;
            vec![each; self.header.len()]
        } else {
            self.columns.clone()
        };
        let total_width: u32 = columns.iter().sum();
        let pad = self.cell_padding as i32;
        let font_size = self.font_size as f32;
        let font = config::resolve_font(ctx.config, self.font.as_deref());
        let even_bg = self
            .row_bg
            .first()
            .cloned()
            .or_else(|| self.even_bg.clone())
            .unwrap_or_else(|| "#FAFAFA".into());
        let odd_bg = self
            .row_bg
            .get(1)
            .cloned()
            .or_else(|| self.odd_bg.clone())
            .unwrap_or_else(|| "#FFFFFF".into());

        // 各列起点（多一个收尾位置，供累计计算）
        let mut col_xs = vec![self.x];
        for width in &columns {
            let next = col_xs.last().copied().unwrap_or(self.x) + *width as i32;
            col_xs.push(next);
        }

        let fill = |color: &str| ShapeOptions {
            color: color.to_string(),
            filled: true,
            ..Default::default()
        };

        canvas.rectangle(
            self.x,
            self.y,
            total_width,
            self.header_height,
            &fill(&self.header_bg),
        )?;

        for (i, header) in self.header.iter().enumerate() {
            let Some(width) = columns.get(i) else { continue };
            let align = self
                .alignments
                .get(i)
                .map(String::as_str)
                .unwrap_or("left");
            let cx = cell_x(col_xs[i], *width, align, pad);
            canvas.text(
                &cell_text(header),
                cx as f32,
                baseline(self.y, self.header_height, font_size) as f32,
                &TextOptions {
                    font: Some(font.clone()),
                    size: font_size,
                    color: self.header_color.clone(),
                    align: text_align(align),
                    ..Default::default()
                },
            )?;
        }

        let mut current_y = self.y + self.header_height as i32;
        for (ri, row) in self.rows.iter().enumerate() {
            let bg = if ri % 2 == 0 { &even_bg } else { &odd_bg };
            canvas.rectangle(self.x, current_y, total_width, self.row_height, &fill(bg))?;

            for (ci, cell) in row.iter().enumerate() {
                let Some(width) = columns.get(ci) else {
                    continue;
                };
                let align = self
                    .alignments
                    .get(ci)
                    .map(String::as_str)
                    .unwrap_or("left");
                let cx = cell_x(col_xs[ci], *width, align, pad);
                canvas.text(
                    &cell_text(cell),
                    cx as f32,
                    baseline(current_y, self.row_height, font_size) as f32,
                    &TextOptions {
                        font: Some(font.clone()),
                        size: font_size,
                        color: self.row_color.clone(),
                        align: text_align(align),
                        ..Default::default()
                    },
                )?;
            }

            canvas.line(
                self.x,
                current_y + self.row_height as i32 - 1,
                self.x + total_width as i32 - 1,
                current_y + self.row_height as i32 - 1,
                &crate::drivers::LineOptions {
                    color: self.border_color.clone(),
                    ..Default::default()
                },
            )?;

            current_y += self.row_height as i32;
        }
        Ok(())
    }
}

/// 列内文本锚点：左对齐贴内边距、居中取列中点、右对齐贴列右缘。
fn cell_x(col_x: i32, col_width: u32, align: &str, pad: i32) -> i32 {
    match align {
        "center" => col_x + col_width as i32 / 2,
        "right" => col_x + col_width as i32 - pad,
        _ => col_x + pad,
    }
}

/// 文本基线：文字按基线绘制，需下移半个字高才是视觉居中（同 PHP `TableElement::baseline()`）。
fn baseline(top: i32, height: u32, font_size: f32) -> i32 {
    top + ((height as f32 + font_size * 0.72) / 2.0) as i32
}
