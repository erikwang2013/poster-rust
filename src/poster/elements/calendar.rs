//! 日历元素，对应 PHP `CalendarElement`：月历网格 + 今天 / 自定义高亮。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{Datelike, Local, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config;
use crate::drivers::{ImageDriver, ShapeOptions, TextAlign, TextOptions};
use crate::error::{PosterError, Result};

use super::{ElementRender, RenderCtx, cell_text, positive};

/// 日历。
///
/// `highlights` 为 `日期 => 文案` 或 `日期 => {"text": ..., "bg": ...}`，
/// 键形如 `2026-10-06`（同 PHP）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CalendarElement {
    /// 年份；`None` = 今年。
    pub year: Option<i32>,
    /// 月份 1-12；`None` = 本月。
    pub month: Option<u32>,
    pub x: i32,
    pub y: i32,
    /// 单元格边长（px）。
    pub cell_size: u32,
    /// 每周起始：0 = 周日，其它 = 周一。
    pub start_day: u32,
    /// 字体路径；`None` = 配置默认字体。
    pub font: Option<PathBuf>,
    /// 标题；`None` = `2026年10月`。
    pub title: Option<String>,
    /// 日期高亮：`"2026-10-01": "国庆"` 或 `{"text": "国庆", "bg": "#FFEAA7"}`。
    pub highlights: BTreeMap<String, Value>,
    pub header_bg: String,
    pub header_color: String,
    pub cell_bg: String,
    pub cell_border: String,
    /// 今天所在格的背景色。
    pub today_bg: String,
    /// 高亮日期所在格的默认背景色。
    pub highlight_bg: String,
    pub text_color: String,
    /// 空白格（月初 / 月末）底色。
    pub dim_color: String,
}

impl Default for CalendarElement {
    fn default() -> Self {
        Self {
            year: None,
            month: None,
            x: 0,
            y: 0,
            cell_size: 60,
            start_day: 0,
            font: None,
            title: None,
            highlights: BTreeMap::new(),
            header_bg: "#333333".into(),
            header_color: "#FFFFFF".into(),
            cell_bg: "#FFFFFF".into(),
            cell_border: "#DDDDDD".into(),
            today_bg: "#FF6B6B".into(),
            highlight_bg: "#FFEAA7".into(),
            text_color: "#333333".into(),
            dim_color: "#CCCCCC".into(),
        }
    }
}

impl ElementRender for CalendarElement {
    fn render(&self, canvas: &mut ImageDriver, ctx: &RenderCtx<'_>) -> Result<()> {
        let now = Local::now().date_naive();
        let year = self.year.unwrap_or_else(|| now.year());
        let month = self.month.unwrap_or_else(|| now.month());
        if !(1..=12).contains(&month) {
            return Err(PosterError::Template(format!(
                "日历月份必须在 1-12，得到 {month}"
            )));
        }
        let first = NaiveDate::from_ymd_opt(year, month, 1).ok_or_else(|| {
            PosterError::Template(format!("日历日期非法: {year}-{month:02}"))
        })?;
        let days_in_month = days_in_month(year, month);
        let first_dow = first.weekday().num_days_from_sunday();
        let adjusted_dow = if self.start_day == 0 {
            first_dow
        } else {
            (first_dow + 6) % 7
        };
        let today = now.format("%Y-%m-%d").to_string();

        let cell_size = positive(self.cell_size, "cellSize")?;
        let cell = cell_size as i32;
        let cell_f = cell_size as f32;
        let font = config::resolve_font(ctx.config, self.font.as_deref());

        let day_names: [&str; 7] = if self.start_day == 0 {
            ["日", "一", "二", "三", "四", "五", "六"]
        } else {
            ["一", "二", "三", "四", "五", "六", "日"]
        };

        let width = cell * 7;

        // 标题栏
        let title = self
            .title
            .clone()
            .unwrap_or_else(|| format!("{year}年{month}月"));
        canvas.rectangle(self.x, self.y, width as u32, cell_size, &fill(&self.header_bg))?;
        canvas.text(
            &title,
            (self.x + width / 2) as f32,
            (self.y + (cell_f * 0.65) as i32) as f32,
            &self.text_opts(18.0, &self.header_color, TextAlign::Center, &font),
        )?;

        // 星期栏
        let header_font_size = (cell_f * 0.22) as i32 as f32;
        for (d, name) in day_names.iter().enumerate() {
            let dx = self.x + d as i32 * cell;
            let dy = self.y + cell;
            canvas.rectangle(dx, dy, cell_size, (cell_f * 0.6) as u32, &fill("#F5F5F5"))?;
            canvas.text(
                name,
                (dx + cell / 2) as f32,
                (dy + (cell_f * 0.42) as i32) as f32,
                &self.text_opts(header_font_size, "#666666", TextAlign::Center, &font),
            )?;
        }

        // 日期格
        let mut row_y = self.y + cell + (cell_f * 0.6) as i32;
        let mut day = 1u32;
        for row in 0..6 {
            for col in 0..7 {
                let cx = self.x + col * cell;

                if (row == 0 && col < adjusted_dow as i32) || day > days_in_month {
                    // 留白格用 dimColor，与有值格拉开层次
                    canvas.rectangle(
                        cx,
                        row_y,
                        cell_size,
                        cell_size,
                        &fill(&self.dim_color),
                    )?;
                    canvas.rectangle(
                        cx,
                        row_y,
                        cell_size,
                        cell_size,
                        &stroke(&self.cell_border),
                    )?;
                    continue;
                }

                let date = format!("{year:04}-{month:02}-{day:02}");
                let bg = if date == today {
                    self.today_bg.clone()
                } else if let Some(highlight) = self.highlights.get(&date) {
                    highlight
                        .get("bg")
                        .and_then(Value::as_str)
                        .unwrap_or(&self.highlight_bg)
                        .to_string()
                } else {
                    self.cell_bg.clone()
                };

                canvas.rectangle(cx, row_y, cell_size, cell_size, &fill(&bg))?;
                canvas.rectangle(
                    cx,
                    row_y,
                    cell_size,
                    cell_size,
                    &stroke(&self.cell_border),
                )?;

                let weekend = if self.start_day == 0 {
                    col == 0 || col == 6
                } else {
                    col == 5 || col == 6
                };
                let day_color = if date == today {
                    "#FFFFFF"
                } else if weekend {
                    "#E74C3C"
                } else {
                    self.text_color.as_str()
                };

                canvas.text(
                    &day.to_string(),
                    (cx + 8) as f32,
                    (row_y + (cell_f * 0.3) as i32) as f32,
                    &self.text_opts((cell_f * 0.28) as i32 as f32, day_color, TextAlign::Left, &font),
                )?;

                if let Some(highlight) = self.highlights.get(&date) {
                    let text = match highlight {
                        Value::Object(map) => map.get("text").map(cell_text).unwrap_or_default(),
                        other => cell_text(other),
                    };
                    if !text.is_empty() {
                        canvas.text(
                            &text,
                            (cx + cell / 2) as f32,
                            (row_y + (cell_f * 0.75) as i32) as f32,
                            &self.text_opts(
                                (cell_f * 0.16) as i32 as f32,
                                "#666666",
                                TextAlign::Center,
                                &font,
                            ),
                        )?;
                    }
                }

                day += 1;
            }
            row_y += cell;
            if day > days_in_month {
                break;
            }
        }
        Ok(())
    }
}

impl CalendarElement {
    /// 日历里的文字选项：统一带上配置字体。
    fn text_opts(&self, size: f32, color: &str, align: TextAlign, font: &Path) -> TextOptions {
        TextOptions {
            font: Some(font.to_path_buf()),
            size,
            color: color.to_string(),
            align,
            ..Default::default()
        }
    }
}

fn fill(color: &str) -> ShapeOptions {
    ShapeOptions {
        color: color.to_string(),
        filled: true,
        ..Default::default()
    }
}

fn stroke(color: &str) -> ShapeOptions {
    ShapeOptions {
        color: color.to_string(),
        filled: false,
        ..Default::default()
    }
}

/// 当月天数（PHP `date('t')`）。
fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        _ => 28,
    }
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}
