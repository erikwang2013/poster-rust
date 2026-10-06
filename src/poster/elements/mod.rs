//! 元素：选项结构、渲染 trait 与公共工具，对应 PHP `src/Poster/Elements/`。
//!
//! - [`Element`] 是 `#[serde(tag = "type")]` 的枚举（kebab-case 类型名），
//!   各元素结构体的 JSON 键名与 PHP 版逐字一致（`maxWidth` / `offsetX` / `spacing_x`…），
//!   因此 PHP 导出的模板 JSON 可以直接读入、Rust 导出的模板也能被 PHP 读回。
//! - 渲染统一走 [`ElementRender`]，配置（默认字体、缺图占位）经 [`RenderCtx`] 传入。
//! - 尺寸兜底、缺图占位、`{{var}}` 替换等公共逻辑在本模块，元素实现只管自己的算法。

pub mod artistic_text;
pub mod avatar;
pub mod calendar;
pub mod chart;
pub mod emoji;
pub mod emoticon;
pub mod icon;
pub mod image;
pub mod line;
pub mod qrcode;
pub mod shape;
pub mod table;
pub mod text;
pub mod watermark;

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{Placeholder, PosterConfig};
use crate::drivers::{ImageDriver, TextAlign};
use crate::error::{PosterError, Result};

/// 渲染上下文：元素读取默认字体、缺图占位等配置的唯一入口。
pub struct RenderCtx<'a> {
    /// 生效配置（渲染入口取 `crate::config::global()`）。
    pub config: &'a PosterConfig,
}

/// 元素渲染。`Element` 按变体分发到各元素的实现。
pub trait ElementRender {
    /// 把元素画到画布上。`canvas` 是整张海报的同一个画布，元素只叠加自己的部分。
    fn render(&self, canvas: &mut ImageDriver, ctx: &RenderCtx<'_>) -> Result<()>;
}

/// 已知元素类型名（顺序与 PHP `ElementRegistry::TYPES` 一致）。
///
/// `artistictext` 与 `artistic-text` 是同一元素的别名（PHP 注册表如此）。
pub const TYPES: [&str; 15] = [
    "text",
    "image",
    "qrcode",
    "avatar",
    "shape",
    "line",
    "watermark",
    "table",
    "chart",
    "calendar",
    "artistictext",
    "artistic-text",
    "emoji",
    "icon",
    "emoticon",
];

/// 海报元素：14 种类型，JSON 以 `type` 为内部标签（kebab-case）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Element {
    Text(text::TextElement),
    Image(image::ImageElement),
    Qrcode(qrcode::QrcodeElement),
    Avatar(avatar::AvatarElement),
    Shape(shape::ShapeElement),
    Line(line::LineElement),
    Watermark(watermark::WatermarkElement),
    Table(table::TableElement),
    Chart(chart::ChartElement),
    Calendar(calendar::CalendarElement),
    /// PHP 注册表里的别名 `artistictext` 同样接受。
    #[serde(alias = "artistictext")]
    ArtisticText(artistic_text::ArtisticTextElement),
    Emoji(emoji::EmojiElement),
    Icon(icon::IconElement),
    Emoticon(emoticon::EmoticonElement),
}

impl Element {
    /// 元素的规范类型名（导出模板、报错提示用）。
    pub fn kind(&self) -> &'static str {
        match self {
            Element::Text(_) => "text",
            Element::Image(_) => "image",
            Element::Qrcode(_) => "qrcode",
            Element::Avatar(_) => "avatar",
            Element::Shape(_) => "shape",
            Element::Line(_) => "line",
            Element::Watermark(_) => "watermark",
            Element::Table(_) => "table",
            Element::Chart(_) => "chart",
            Element::Calendar(_) => "calendar",
            Element::ArtisticText(_) => "artistic-text",
            Element::Emoji(_) => "emoji",
            Element::Icon(_) => "icon",
            Element::Emoticon(_) => "emoticon",
        }
    }

    /// 从模板定义构建（`type` 必须在定义里，见 [`Self::from_parts`]）。
    pub fn from_def(def: Value) -> Result<Self> {
        let kind = def
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if kind.is_empty() {
            return Err(PosterError::Template(format!(
                "元素定义缺少有效的 \"type\" 键。已知类型: {}",
                TYPES.join(", ")
            )));
        }
        Self::from_parts(&kind, def)
    }

    /// 注册表分发：`kind` 为类型名、`def` 为该元素的选项对象；
    /// 定义里已有的 `type` 会被 `kind` 覆盖（同 PHP `ElementRegistry::create()`）。
    pub fn from_parts(kind: &str, def: Value) -> Result<Self> {
        let mut obj = match def {
            Value::Object(obj) => obj,
            other => {
                return Err(PosterError::Template(format!(
                    "元素 \"{kind}\" 的选项必须是 JSON 对象，得到 {other}"
                )));
            }
        };
        if !TYPES.contains(&kind) {
            return Err(PosterError::Template(format!(
                "未知元素类型 \"{kind}\"。已知类型: {}",
                TYPES.join(", ")
            )));
        }
        obj.insert("type".into(), Value::String(kind.into()));
        serde_json::from_value(Value::Object(obj)).map_err(|e| {
            PosterError::Template(format!("元素 \"{kind}\" 选项解析失败: {e}"))
        })
    }

    /// 模板变量替换：递归替换元素所有字符串字段里的 `{{var}}`；变量缺失报错。
    ///
    /// 借 JSON 走一遍是为了覆盖表格 `rows`、图表 `data`、日历 `highlights`
    /// 这类嵌套结构（PHP 的 `AbstractElement::resolveKeys` 逐个键声明，这里全量处理）。
    pub fn resolve_vars(&mut self, vars: &BTreeMap<String, String>) -> Result<()> {
        if vars.is_empty() {
            return Ok(());
        }
        let mut value = serde_json::to_value(&*self)?;
        resolve_value(&mut value, vars)?;
        *self = serde_json::from_value(value)?;
        Ok(())
    }
}

impl ElementRender for Element {
    fn render(&self, canvas: &mut ImageDriver, ctx: &RenderCtx<'_>) -> Result<()> {
        match self {
            Element::Text(e) => e.render(canvas, ctx),
            Element::Image(e) => e.render(canvas, ctx),
            Element::Qrcode(e) => e.render(canvas, ctx),
            Element::Avatar(e) => e.render(canvas, ctx),
            Element::Shape(e) => e.render(canvas, ctx),
            Element::Line(e) => e.render(canvas, ctx),
            Element::Watermark(e) => e.render(canvas, ctx),
            Element::Table(e) => e.render(canvas, ctx),
            Element::Chart(e) => e.render(canvas, ctx),
            Element::Calendar(e) => e.render(canvas, ctx),
            Element::ArtisticText(e) => e.render(canvas, ctx),
            Element::Emoji(e) => e.render(canvas, ctx),
            Element::Icon(e) => e.render(canvas, ctx),
            Element::Emoticon(e) => e.render(canvas, ctx),
        }
    }
}

// ── 公共工具（对应 PHP `AbstractElement` 的 protected 帮助方法）──────────

/// 尺寸兜底：`<= 0` 报错，不让非法值落进绘制路径（PHP `AbstractElement::positive()`）。
pub fn positive(value: u32, key: &str) -> Result<u32> {
    if value == 0 {
        return Err(PosterError::Template(format!(
            "选项 \"{key}\" 必须大于 0，得到 {value}"
        )));
    }
    Ok(value)
}

/// 加载图片；文件缺失时回退 `config.poster.placeholder`（占位图也不存在则返回 `None`）。
///
/// 对应 PHP `AbstractElement::loadImage()`：`None` 表示该元素跳过不绘制。
pub fn load_image(ctx: &RenderCtx<'_>, src: &str) -> Result<Option<ImageDriver>> {
    let path = Path::new(src);
    if path.is_file() {
        return ImageDriver::load(path).map(Some);
    }
    match &ctx.config.poster.placeholder {
        None => Ok(None),
        Some(Placeholder::Pet) => Ok(Some(ImageDriver::from_bytes(crate::assets::PET_PNG)?)),
        Some(Placeholder::Path(p)) => {
            if p.is_file() {
                ImageDriver::load(p).map(Some)
            } else {
                Ok(None)
            }
        }
    }
}

/// 对齐字符串 → [`TextAlign`]（未知值按 `left`，同 PHP 的 `match` 兜底分支）。
pub fn text_align(align: &str) -> TextAlign {
    match align {
        "center" => TextAlign::Center,
        "right" => TextAlign::Right,
        _ => TextAlign::Left,
    }
}

/// 表格 / 图表 / 日历里的单元格取值 → 字符串（PHP 统一 `(string)` 强转）。
pub fn cell_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// 码点值 → 字符：接受 `U+1F600` / `1F600` / `0x1F600` / `\u{1F600}` / 十进制整数。
///
/// 对应 PHP 的 `mb_chr(hexdec(...))`：非法输入返回 `None`（PHP 返回空串）。
pub fn codepoint_char(value: &Value) -> Option<char> {
    match value {
        Value::Number(n) => n.as_u64().and_then(|c| char::from_u32(c as u32)),
        Value::String(s) => {
            let mut hex = s.trim().to_string();
            for prefix in ["\\u{", "\\u", "U+", "u+", "0x", "0X", "}"] {
                hex = hex.replace(prefix, "");
            }
            let hex = hex.trim();
            if hex.is_empty() || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                return None;
            }
            char::from_u32(u32::from_str_radix(hex, 16).ok()?)
        }
        _ => None,
    }
}

// ── 内部：模板变量替换 ──────────────────────────────────

/// 递归替换 JSON 里所有字符串的 `{{var}}`。
fn resolve_value(value: &mut Value, vars: &BTreeMap<String, String>) -> Result<()> {
    match value {
        Value::String(s) => *s = replace_placeholders(s, vars)?,
        Value::Array(items) => {
            for item in items {
                resolve_value(item, vars)?;
            }
        }
        Value::Object(map) => {
            for (_, item) in map.iter_mut() {
                resolve_value(item, vars)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// 字符串占位符替换：`{{ name }}`（名字支持 Unicode 字母数字与下划线，同 PHP 的 `\p{L}\p{N}_`）。
fn replace_placeholders(text: &str, vars: &BTreeMap<String, String>) -> Result<String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match parse_placeholder(after) {
            Some((name, consumed)) => {
                let value = vars.get(&name).ok_or_else(|| {
                    PosterError::Template(format!("模板变量缺失: {{{{{name}}}}}"))
                })?;
                out.push_str(value);
                rest = &after[consumed..];
            }
            // 不是合法占位符（如 `{{ }}`）：原样保留
            None => {
                out.push_str("{{");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    Ok(out)
}

/// 解析 `{{` 之后的内容：返回 `(变量名, 已消费字节数)`，格式不符返回 `None`。
fn parse_placeholder(rest: &str) -> Option<(String, usize)> {
    let bytes = rest.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() && (bytes[idx] as char).is_whitespace() {
        idx += 1;
    }
    let name_start = idx;
    while idx < bytes.len() {
        let ch = rest[idx..].chars().next()?;
        if ch.is_alphanumeric() || ch == '_' {
            idx += ch.len_utf8();
        } else {
            break;
        }
    }
    if idx == name_start {
        return None;
    }
    let name = rest[name_start..idx].to_string();
    while idx < bytes.len() && (bytes[idx] as char).is_whitespace() {
        idx += 1;
    }
    rest[idx..].starts_with("}}").then_some((name, idx + 2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_replacement_handles_spacing_and_unicode() {
        let vars = BTreeMap::from([("名字".to_string(), "海报".to_string()), ("n".to_string(), "3".to_string())]);
        assert_eq!(replace_placeholders("你好 {{ 名字 }} ×{{n}}", &vars).unwrap(), "你好 海报 ×3");
        assert_eq!(replace_placeholders("{{  }}", &vars).unwrap(), "{{  }}", "非法占位符原样保留");
    }

    #[test]
    fn missing_variable_is_an_error() {
        let vars = BTreeMap::from([("a".to_string(), "1".to_string())]);
        let err = replace_placeholders("{{b}}", &vars).unwrap_err();
        assert!(matches!(err, PosterError::Template(_)), "缺变量应报模板错误");
    }

    #[test]
    fn unknown_type_lists_known_types() {
        let err = Element::from_parts("nope", Value::Object(Default::default())).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("nope") && msg.contains("watermark"), "报错要列出已知类型: {msg}");
    }

    #[test]
    fn artistictext_alias_parses_to_same_element() {
        let a = Element::from_parts("artistictext", serde_json::json!({"text": "hi"})).unwrap();
        let b = Element::from_parts("artistic-text", serde_json::json!({"text": "hi"})).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.kind(), "artistic-text");
    }
}
