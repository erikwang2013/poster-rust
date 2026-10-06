//! 颜文字元素，对应 PHP `EmoticonElement`：表情名 → 颜文字字符串。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::drivers::{ImageDriver, TextOptions};
use crate::error::Result;

use super::{ElementRender, RenderCtx};

/// 常用颜文字（与 PHP 的 `KAOMOJI` 逐条一致）。
const KAOMOJI: [(&str, &str); 12] = [
    ("happy", "(｡•̀ᴗ-)✧"),
    ("love", "(♡°▽°♡)"),
    ("cry", "(╥﹏╥)"),
    ("angry", "(╬ Ò﹏Ó)"),
    ("surprised", "(⊙_⊙)"),
    ("cool", "(⌐■_■)"),
    ("sleepy", "(－_－) zzZ"),
    ("wave", "(・∀・)ノ"),
    ("think", "(ー_ーゞ"),
    ("shrug", "¯\\_(ツ)_/¯"),
    ("tableflip", "(╯°□°）╯︵ ┻━┻"),
    ("lenny", "( ͡° ͜ʖ ͡°)"),
];

/// 颜文字。
///
/// `text` 优先；为空时用 `expression` 从 [`KAOMOJI`] 取。`font` 只认存在的文件，
/// 否则用配置默认字体（同 PHP）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct EmoticonElement {
    /// 直接给出的颜文字。
    pub text: String,
    /// 预置表情名，见 [`expressions()`]。
    pub expression: String,
    pub x: i32,
    pub y: i32,
    pub size: u32,
    pub color: String,
    /// 字体文件路径。
    pub font: Option<PathBuf>,
}

impl Default for EmoticonElement {
    fn default() -> Self {
        Self {
            text: String::new(),
            expression: String::new(),
            x: 0,
            y: 0,
            size: 24,
            color: "#333333".into(),
            font: None,
        }
    }
}

impl ElementRender for EmoticonElement {
    fn render(&self, canvas: &mut ImageDriver, _ctx: &RenderCtx<'_>) -> Result<()> {
        let content = if !self.text.is_empty() {
            self.text.clone()
        } else {
            kaomoji(&self.expression).unwrap_or_default().to_string()
        };
        if content.is_empty() {
            return Ok(());
        }

        let mut opts = TextOptions {
            size: self.size as f32,
            color: self.color.clone(),
            ..Default::default()
        };
        if let Some(font) = self.font.as_deref().filter(|path| path.is_file()) {
            opts.font = Some(font.to_path_buf());
        }
        canvas.text(&content, self.x as f32, self.y as f32, &opts)
    }
}

/// 预置表情名列表（PHP `EmoticonElement::expressions()`）。
pub fn expressions() -> Vec<&'static str> {
    KAOMOJI.iter().map(|(name, _)| *name).collect()
}

/// 表情名 → 颜文字；未收录返回 `None`。
pub fn kaomoji(name: &str) -> Option<&'static str> {
    KAOMOJI
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, text)| *text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kaomoji_table_matches_php() {
        assert_eq!(expressions().len(), 12);
        assert_eq!(kaomoji("shrug"), Some("¯\\_(ツ)_/¯"));
        assert_eq!(kaomoji("tableflip"), Some("(╯°□°）╯︵ ┻━┻"));
        assert_eq!(kaomoji("nope"), None);
    }
}
