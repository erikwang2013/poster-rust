//! 图标元素，对应 PHP `IconElement`：FontAwesome 图标名 → 私有区码点。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::drivers::{ImageDriver, TextOptions};
use crate::error::Result;

use super::{ElementRender, RenderCtx, codepoint_char};

/// FontAwesome 5/6 常用图标的 Unicode 映射（与 PHP 的 `FA_ICONS` 逐条一致）。
const FA_ICONS: [(&str, char); 44] = [
    ("heart", '\u{F004}'),
    ("star", '\u{F005}'),
    ("user", '\u{F007}'),
    ("clock", '\u{F017}'),
    ("home", '\u{F015}'),
    ("cog", '\u{F013}'),
    ("check", '\u{F00C}'),
    ("times", '\u{F00D}'),
    ("search", '\u{F002}'),
    ("envelope", '\u{F0E0}'),
    ("phone", '\u{F095}'),
    ("camera", '\u{F030}'),
    ("play", '\u{F04B}'),
    ("pause", '\u{F04C}'),
    ("shopping-cart", '\u{F07A}'),
    ("tag", '\u{F02B}'),
    ("map-marker", '\u{F3C5}'),
    ("calendar", '\u{F133}'),
    ("comment", '\u{F075}'),
    ("share", '\u{F064}'),
    ("download", '\u{F019}'),
    ("upload", '\u{F093}'),
    ("lock", '\u{F023}'),
    ("globe", '\u{F0AC}'),
    ("link", '\u{F0C1}'),
    ("image", '\u{F03E}'),
    ("music", '\u{F001}'),
    ("video", '\u{F008}'),
    ("bell", '\u{F0F3}'),
    ("bookmark", '\u{F02E}'),
    ("thumbs-up", '\u{F164}'),
    ("eye", '\u{F06E}'),
    ("trash", '\u{F1F8}'),
    ("edit", '\u{F044}'),
    ("plus", '\u{F067}'),
    ("minus", '\u{F068}'),
    ("arrow-right", '\u{F061}'),
    ("arrow-left", '\u{F060}'),
    ("arrow-up", '\u{F062}'),
    ("arrow-down", '\u{F063}'),
    ("location-dot", '\u{F3C5}'),
    ("fire", '\u{F06D}'),
    ("gift", '\u{F06B}'),
    ("rocket", '\u{F135}'),
];

/// 图标。
///
/// `codepoint` 优先于 `icon`；两者都解析不出字符时跳过不绘制。`font` 须为存在的文件，
/// 否则退回 `[icon]` 文字占位（同 PHP）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct IconElement {
    /// 图标名，见 [`FA_ICONS`]。
    pub icon: String,
    /// 码点，优先于 `icon`。
    pub codepoint: Option<Value>,
    pub x: i32,
    pub y: i32,
    pub size: u32,
    pub color: String,
    /// FontAwesome 字体文件路径。
    pub font: Option<PathBuf>,
}

impl Default for IconElement {
    fn default() -> Self {
        Self {
            icon: String::new(),
            codepoint: None,
            x: 0,
            y: 0,
            size: 32,
            color: "#333333".into(),
            font: None,
        }
    }
}

impl ElementRender for IconElement {
    fn render(&self, canvas: &mut ImageDriver, _ctx: &RenderCtx<'_>) -> Result<()> {
        let ch = if let Some(codepoint) = &self.codepoint {
            codepoint_char(codepoint).map(String::from).unwrap_or_default()
        } else if self.icon.is_empty() {
            String::new()
        } else {
            icon_char(&self.icon).map(String::from).unwrap_or_default()
        };
        if ch.is_empty() {
            return Ok(());
        }

        let size = self.size as f32;
        match self.font.as_deref().filter(|path| path.is_file()) {
            Some(font) => canvas.text(
                &ch,
                self.x as f32,
                self.y as f32,
                &TextOptions {
                    size,
                    color: self.color.clone(),
                    font: Some(font.to_path_buf()),
                    ..Default::default()
                },
            ),
            // 没有图标字体：画 `[icon]` 文字占位（同 PHP）
            None => {
                let placeholder = if self.icon.is_empty() {
                    "?".to_string()
                } else {
                    format!("[{}]", self.icon)
                };
                canvas.text(
                    &placeholder,
                    self.x as f32,
                    self.y as f32,
                    &TextOptions {
                        size: size * 0.6,
                        color: self.color.clone(),
                        ..Default::default()
                    },
                )
            }
        }
    }
}

/// 图标名 → 字符；未收录返回 `None`。
pub fn icon_char(name: &str) -> Option<char> {
    FA_ICONS
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, ch)| *ch)
}
