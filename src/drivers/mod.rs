//! 图像驱动：纯 Rust 画布（`image` crate），对应 PHP 版的 GD 驱动。
//!
//! PHP 侧有 GD / Imagick 双驱动，Rust 侧单一纯 Rust 实现、无系统依赖；
//! 方法面与 `ImageDriverInterface` 一一对应，选项键与 PHP 版一致（JSON 模板可直接互用）。

pub mod canvas;
pub mod color;
pub mod text;

pub use canvas::ImageDriver;
pub use text::Font;

use serde::{Deserialize, Serialize};

/// 文本对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// 文字绘制选项，键名对齐 PHP `text()` 的 options。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TextOptions {
    /// 自定义字体路径；`None` = 用配置默认字体（随包分发的阿里巴巴普惠体）。
    pub font: Option<std::path::PathBuf>,
    /// 字号（px）。
    pub size: f32,
    /// 颜色 `#RGB` / `#RRGGBB` / `#RRGGBBAA`。
    pub color: String,
    /// 旋转角度（度，逆时针为正，围绕 x/y 锚点）。
    pub angle: f32,
    /// 自动换行宽度；0 = 不换行（仅按 `\n` 分段）。
    pub max_width: f32,
    /// 对齐：x 是左/中/右锚点。
    pub align: TextAlign,
    /// 行高；`None` = 字号 × 1.5。
    pub line_height: Option<f32>,
}

impl Default for TextOptions {
    fn default() -> Self {
        Self {
            font: None,
            size: 16.0,
            color: "#000000".into(),
            angle: 0.0,
            max_width: 0.0,
            align: TextAlign::Left,
            line_height: None,
        }
    }
}

/// 形状（矩形 / 椭圆 / 多边形）选项。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ShapeOptions {
    pub color: String,
    /// 圆角半径（仅矩形，`filled = true` 时生效；与 PHP 一致，描边矩形忽略圆角）。
    pub radius: u32,
    pub filled: bool,
    /// 透明度 0-1（0-100 也接受）；`None` = 由颜色自带 alpha 决定。
    pub opacity: Option<f32>,
}

impl Default for ShapeOptions {
    fn default() -> Self {
        Self {
            color: "#FFFFFF".into(),
            radius: 0,
            filled: true,
            opacity: None,
        }
    }
}

/// 直线选项。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LineOptions {
    pub color: String,
    /// 线宽（px）。
    pub width: u32,
}

impl Default for LineOptions {
    fn default() -> Self {
        Self {
            color: "#000000".into(),
            width: 1,
        }
    }
}

/// 图片叠加的阴影选项（PHP `image()` 的 `shadow`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ShadowOptions {
    pub color: String,
    pub offset_x: i32,
    pub offset_y: i32,
    pub blur: u32,
    /// 透明度 0-1（0-100 也接受）；`None` = 由颜色自带 alpha 决定。
    pub opacity: Option<f32>,
}

impl Default for ShadowOptions {
    fn default() -> Self {
        Self {
            color: "#00000033".into(),
            offset_x: 4,
            offset_y: 4,
            blur: 8,
            opacity: None,
        }
    }
}

/// 图片叠加选项（PHP `image()` 的 options）。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OverlayOptions {
    /// 目标宽；`None` = 原图宽。
    pub width: Option<u32>,
    /// 目标高；`None` = 原图高。
    pub height: Option<u32>,
    /// 圆角（目标像素）。
    pub radius: u32,
    /// 阴影；`None` = 不画。
    pub shadow: Option<ShadowOptions>,
}
