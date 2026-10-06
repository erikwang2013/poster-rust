//! 二维码元素，对应 PHP `QrcodeElement`：码 + 可选中心 logo + 底部文案。

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config;
use crate::drivers::{ImageDriver, OverlayOptions, TextAlign, TextOptions};
use crate::error::Result;
use crate::qrcode::EcLevel;

use super::{ElementRender, RenderCtx, positive};

/// 二维码静区（模块数）；PHP `QrcodeGenerator` 默认 2。
const MARGIN_MODULES: u32 = 2;

/// 二维码。
///
/// `size` 是请求边长：PHP 版按模块数向下量化（实际略小），Rust 版按请求尺寸精确输出，
/// 因此 `label` 的 y 用 `size` 定位（PHP 用实际量化高度）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct QrcodeElement {
    /// 二维码内容；空串跳过绘制。
    pub content: String,
    pub x: i32,
    pub y: i32,
    /// 边长（px）。
    pub size: u32,
    /// 纠错级别 `L` / `M` / `Q` / `H`（默认 H）。
    pub level: String,
    /// 中心 logo 图片路径；不存在则忽略。
    pub logo: Option<String>,
    /// 码下方的文案；空 = 不画。
    pub label: Option<String>,
    /// 文案字号（PHP 键名是 snake_case 的 `label_size`，此处保持一致）。
    #[serde(rename = "label_size")]
    pub label_size: u32,
    /// 文案颜色。
    #[serde(rename = "label_color")]
    pub label_color: String,
    /// `radius` / `shadow` / `width` / `height` 等叠加选项。
    #[serde(flatten)]
    pub style: OverlayOptions,
}

impl Default for QrcodeElement {
    fn default() -> Self {
        Self {
            content: String::new(),
            x: 0,
            y: 0,
            size: 200,
            level: "H".into(),
            logo: None,
            label: None,
            label_size: 14,
            label_color: "#999999".into(),
            style: OverlayOptions::default(),
        }
    }
}

impl ElementRender for QrcodeElement {
    fn render(&self, canvas: &mut ImageDriver, ctx: &RenderCtx<'_>) -> Result<()> {
        if self.content.is_empty() {
            return Ok(());
        }
        let size = positive(self.size, "size")?;
        let ec = EcLevel::parse(&self.level)?;

        let mut qr = crate::qrcode::render(&self.content, ec, size, MARGIN_MODULES, "#000000", "#FFFFFF")?;

        if let Some(logo) = self.logo.as_deref() {
            if !logo.is_empty() && Path::new(logo).is_file() {
                let logo_size = positive((size as f32 * 0.22) as u32, "size")?;
                let mut logo_img = ImageDriver::load(Path::new(logo))?;
                logo_img.resize(logo_size, logo_size)?;
                let offset = ((size - logo_size) / 2) as i32;
                qr.overlay(&logo_img, offset, offset, &OverlayOptions::default())?;
            }
        }

        canvas.overlay(&qr, self.x, self.y, &self.style)?;

        if let Some(label) = self.label.as_deref() {
            if !label.is_empty() {
                let opts = TextOptions {
                    font: Some(config::resolve_font(ctx.config, None)),
                    size: self.label_size as f32,
                    color: self.label_color.clone(),
                    align: TextAlign::Center,
                    ..Default::default()
                };
                // 居中：x 取码中心（align=center 让文本以此为中心）
                canvas.text(
                    label,
                    (self.x + size as i32 / 2) as f32,
                    (self.y + size as i32 + 20) as f32,
                    &opts,
                )?;
            }
        }
        Ok(())
    }
}
