//! Emoji 元素，对应 PHP `EmojiElement`：按系统 emoji 字体绘制，找不到则退回普通文字。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::drivers::{ImageDriver, OverlayOptions, TextOptions};
use crate::error::Result;

use super::{ElementRender, RenderCtx, codepoint_char};

/// 常见系统的 emoji 字体路径（与 PHP 版逐条一致）。
const EMOJI_FONT_PATHS: [&str; 3] = [
    "/System/Library/Fonts/Apple Color Emoji.ttc",
    "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf",
    r"C:\Windows\Fonts\seguiemj.ttf",
];

/// Emoji。
///
/// `emoji` 为空时用 `codepoint`（`U+1F600` / `1F600` / `0x1F600` / 十进制整数）。
/// 本元素的 `font` 选项**不回落配置字体**：不是文件就去找系统 emoji 字体（同 PHP）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct EmojiElement {
    /// emoji 字符本身。
    pub emoji: String,
    /// 码点，`emoji` 为空时生效。
    pub codepoint: Option<Value>,
    pub x: i32,
    pub y: i32,
    pub size: u32,
    pub color: String,
    /// 指定字体；须为存在的文件，否则走系统 emoji 字体查找。
    pub font: Option<PathBuf>,
}

impl Default for EmojiElement {
    fn default() -> Self {
        Self {
            emoji: String::new(),
            codepoint: None,
            x: 0,
            y: 0,
            size: 64,
            color: "#000000".into(),
            font: None,
        }
    }
}

impl ElementRender for EmojiElement {
    fn render(&self, canvas: &mut ImageDriver, _ctx: &RenderCtx<'_>) -> Result<()> {
        let ch = if !self.emoji.is_empty() {
            self.emoji.clone()
        } else {
            self.codepoint
                .as_ref()
                .and_then(codepoint_char)
                .map(String::from)
                .unwrap_or_default()
        };
        if ch.is_empty() {
            return Ok(());
        }

        let size = self.size as f32;
        let emoji_font = self
            .font
            .as_deref()
            .filter(|path| path.is_file())
            .map(Path::to_path_buf)
            .or_else(find_emoji_font);

        let Some(font) = emoji_font else {
            // 没有 emoji 字体：退回普通文字（PHP 此分支同样忽略 color，硬编码黑色）
            return canvas.text(
                &ch,
                self.x as f32,
                self.y as f32,
                &TextOptions {
                    size,
                    color: "#000000".into(),
                    ..Default::default()
                },
            );
        };

        // 命中的 emoji 字体往往是彩色位图字体（Noto Color Emoji / Apple Color Emoji），
        // ab_glyph 只认轮廓，画出来可能整片空白：先在草稿画布上试画，有内容才叠加，
        // 否则退回普通字体——不让装饰元素静默消失，也不让它炸掉整张海报
        let options = TextOptions {
            size,
            color: self.color.clone(),
            font: Some(font.clone()),
            ..Default::default()
        };
        if let Ok((ascent, descent, width)) = canvas
            .font(&font)
            .map(|loaded| (loaded.ascent(size), loaded.descent(size), loaded.measure(&ch, size)))
        {
            let ascent = ascent.max(0.0);
            let mut probe = ImageDriver::create(
                width.ceil() as u32 + 8,
                (ascent + descent).ceil() as u32 + 8,
            )?;
            probe.text(&ch, 4.0, ascent + 4.0, &options)?;
            if probe.image().pixels().any(|pixel| pixel.0[3] > 0) {
                return canvas.overlay(
                    &probe,
                    self.x - 4,
                    self.y - ascent as i32 - 4,
                    &OverlayOptions::default(),
                );
            }
        }
        canvas.text(
            &ch,
            self.x as f32,
            self.y as f32,
            &TextOptions {
                size,
                color: self.color.clone(),
                ..Default::default()
            },
        )
    }
}

/// 系统里第一个存在的 emoji 字体。
fn find_emoji_font() -> Option<PathBuf> {
    EMOJI_FONT_PATHS
        .iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
}
