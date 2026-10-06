//! 颜色解析：只接受 `#RGB` / `#RRGGBB` / `#RRGGBBAA`，其它形式报错（与 PHP 版一致）。

use image::Rgba;

use crate::error::{PosterError, Result};

/// 解析十六进制颜色。
pub fn parse(color: &str) -> Result<Rgba<u8>> {
    let hex = color.trim().trim_start_matches('#');
    let valid_len = matches!(hex.len(), 3 | 6 | 8);
    if !valid_len || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(PosterError::Color(format!(
            "Invalid color: '{color}' (expected #RGB, #RRGGBB or #RRGGBBAA)"
        )));
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0);
    Ok(match hex.len() {
        3 => {
            let dup = |i: usize| {
                let v = u8::from_str_radix(&hex[i..i + 1], 16).unwrap_or(0);
                v * 17
            };
            Rgba([dup(0), dup(1), dup(2), 255])
        }
        6 => Rgba([byte(0), byte(2), byte(4), 255]),
        _ => Rgba([byte(0), byte(2), byte(4), byte(6)]),
    })
}

/// 透明度 0-1（>1 视为 0-100 写法）夹取后换算为 alpha 字节。
pub fn opacity_to_alpha(opacity: f32) -> u8 {
    let v = if opacity > 1.0 { opacity / 100.0 } else { opacity };
    ((1.0 - v.clamp(0.0, 1.0)) * 255.0).round() as u8
}

/// 取色 + 可选 opacity 覆盖（opacity 优先于颜色自带 alpha）。
pub fn parse_with_opacity(color: &str, opacity: Option<f32>) -> Result<Rgba<u8>> {
    let mut rgba = parse(color)?;
    if let Some(op) = opacity {
        rgba.0[3] = opacity_to_alpha(op);
    }
    Ok(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_three_forms() {
        assert_eq!(parse("#FFF").unwrap(), Rgba([255, 255, 255, 255]));
        assert_eq!(parse("#FF6B6B").unwrap(), Rgba([255, 107, 107, 255]));
        assert_eq!(parse("#FF6B6B80").unwrap(), Rgba([255, 107, 107, 128]));
        assert_eq!(parse("FF6B6B").unwrap(), Rgba([255, 107, 107, 255]));
    }

    #[test]
    fn rejects_bad_colors() {
        assert!(parse("#GGGGGG").is_err());
        assert!(parse("#FF").is_err());
        assert!(parse("red").is_err());
        assert!(parse("#1234567").is_err());
    }

    #[test]
    fn opacity_clamps_both_scales() {
        assert_eq!(opacity_to_alpha(1.0), 0);
        assert_eq!(opacity_to_alpha(0.0), 255);
        assert_eq!(opacity_to_alpha(50.0), 128); // 0-100 写法
        assert_eq!(opacity_to_alpha(200.0), 0); // 越界夹取到 1.0 = 全不透明
    }

    #[test]
    fn opacity_overrides_color_alpha() {
        let c = parse_with_opacity("#000000FF", Some(0.0)).unwrap();
        assert_eq!(c.0[3], 255);
        let c = parse_with_opacity("#000000FF", None).unwrap();
        assert_eq!(c.0[3], 255);
    }
}
