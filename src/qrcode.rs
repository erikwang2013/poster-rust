//! 二维码：`qrcode` crate 封装（PHP 版是自研纯 PHP 生成器，Rust 侧直接复用成熟 crate）。
//!
//! 输出为像素级可控的画布：模块按整数倍缩放绘制，边缘锐利；支持模块静区、颜色自定义。

use qrcode::QrCode;
use qrcode::types::Color as QrColor;
use serde::{Deserialize, Serialize};

use crate::drivers::canvas::ImageDriver;
use crate::drivers::color;
use crate::error::{PosterError, Result};

/// 纠错级别，对应 PHP 生成器的 L/M/Q/H。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EcLevel {
    L,
    #[default]
    M,
    Q,
    H,
}

impl EcLevel {
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "l" => Ok(Self::L),
            "m" => Ok(Self::M),
            "q" => Ok(Self::Q),
            "h" => Ok(Self::H),
            other => Err(PosterError::Other(format!(
                "未知纠错级别: '{other}' (expected L/M/Q/H)"
            ))),
        }
    }

    fn to_qrcode(self) -> qrcode::EcLevel {
        match self {
            Self::L => qrcode::EcLevel::L,
            Self::M => qrcode::EcLevel::M,
            Self::Q => qrcode::EcLevel::Q,
            Self::H => qrcode::EcLevel::H,
        }
    }
}

/// 模块矩阵：`width × width`，`true` = 深色模块。
pub struct QrMatrix {
    pub width: usize,
    pub modules: Vec<bool>,
}

impl QrMatrix {
    pub fn get(&self, x: usize, y: usize) -> bool {
        self.modules[y * self.width + x]
    }
}

/// 生成模块矩阵。
pub fn matrix(content: &str, ec: EcLevel) -> Result<QrMatrix> {
    let code = QrCode::with_error_correction_level(content, ec.to_qrcode())
        .map_err(|e| PosterError::Other(format!("二维码生成失败: {e}")))?;
    let width = code.width();
    let modules = code
        .to_colors()
        .into_iter()
        .map(|c| c == QrColor::Dark)
        .collect();
    Ok(QrMatrix { width, modules })
}

/// 渲染成画布：`size` 为总边长（px），`margin` 为模块数静区（默认 4，同标准）。
pub fn render(
    content: &str,
    ec: EcLevel,
    size: u32,
    margin_modules: u32,
    dark: &str,
    light: &str,
) -> Result<ImageDriver> {
    let m = matrix(content, ec)?;
    let total_modules = m.width as u32 + margin_modules * 2;
    if size as u64 * size as u64 > 40_000_000 {
        return Err(PosterError::Other(format!("二维码尺寸过大: {size}")));
    }
    let scale = (size as f32 / total_modules as f32).max(1.0);
    let dark_rgba = color::parse(dark)?;
    let light_rgba = color::parse(light)?;

    let mut canvas = ImageDriver::create(size, size)?;
    // 先铺浅色底（保持不透明，扫码更稳）
    for p in canvas.image_mut().pixels_mut() {
        *p = light_rgba;
    }
    let offset = margin_modules as f32 * scale;
    for my in 0..m.width {
        for mx in 0..m.width {
            if !m.get(mx, my) {
                continue;
            }
            let x0 = (offset + mx as f32 * scale).round() as i64;
            let y0 = (offset + my as f32 * scale).round() as i64;
            let x1 = (offset + (mx + 1) as f32 * scale).round();
            let y1 = (offset + (my + 1) as f32 * scale).round();
            for y in y0..(y1 as i64) {
                for x in x0..(x1 as i64) {
                    if x >= 0 && y >= 0 && (x as u32) < size && (y as u32) < size {
                        canvas.image_mut().put_pixel(x as u32, y as u32, dark_rgba);
                    }
                }
            }
        }
    }
    Ok(canvas)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_square_matrix() {
        let m = matrix("https://erik.xyz", EcLevel::M).unwrap();
        assert!(m.width >= 21, "最小 QR 是 21 模块");
        assert_eq!(m.modules.len(), m.width * m.width);
        // 三个定位角：左上 7×7 的 (0,0) 必须深色
        assert!(m.get(0, 0));
    }

    #[test]
    fn renders_to_requested_pixel_size() {
        let c = render("hi", EcLevel::M, 200, 4, "#000000", "#FFFFFF").unwrap();
        assert_eq!(c.size(), (200, 200));
    }

    #[test]
    fn ec_level_parses_case_insensitively() {
        assert_eq!(EcLevel::parse("h").unwrap(), EcLevel::H);
        assert_eq!(EcLevel::parse("Q").unwrap(), EcLevel::Q);
        assert!(EcLevel::parse("x").is_err());
    }
}
