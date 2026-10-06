//! 画布：`image` crate 驱动的像素操作，方法面对应 PHP `GdDriver`。
//!
//! 语义对齐要点（与 PHP 版逐项一致）：
//! - `rectangle(x, y, w, h)` 覆盖 `[x, x+w) × [y, y+h)`；圆角仅在 `filled = true` 生效
//! - `mask()` 在掩膜「透明度过半」处擦除目标像素（GD alpha >= 64 的等价）
//! - `rotate()` 正角度顺时针（PHP `imagerotate(-$angle)`），画布自动扩展
//! - `save()`/`output()` 的质量缺省取配置：PNG 用 `poster.png_compression`，其余用 `image.quality`

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use image::codecs::gif::GifEncoder;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::codecs::webp::WebPEncoder;
use image::{ExtendedColorType, ImageEncoder, Rgba, RgbaImage};


use crate::config;
use crate::drivers::text::{self, Font};
use crate::drivers::{LineOptions, OverlayOptions, ShapeOptions, TextAlign, TextOptions, color};
use crate::error::{PosterError, Result};

/// 像素预算（与 PHP 无 memory_limit 时的历史阈值一致）。
const MAX_PIXELS: u64 = 40_000_000;

/// 画布。
pub struct ImageDriver {
    img: RgbaImage,
    fonts: HashMap<PathBuf, Font>,
}

impl ImageDriver {
    // ── 构造 ──────────────────────────────────────────────

    /// 新建全透明画布。
    pub fn create(width: u32, height: u32) -> Result<Self> {
        guard_size(width, height)?;
        Ok(Self {
            img: RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 0])),
            fonts: HashMap::new(),
        })
    }

    /// 新建纯色画布。
    pub fn filled(width: u32, height: u32, color: &str) -> Result<Self> {
        let rgba = color::parse(color)?;
        guard_size(width, height)?;
        Ok(Self {
            img: RgbaImage::from_pixel(width, height, rgba),
            fonts: HashMap::new(),
        })
    }

    /// 从文件载入（jpg/png/gif/webp，按内容识别）。
    pub fn load(path: &Path) -> Result<Self> {
        let img = image::open(path)?;
        Self::from_image(img.to_rgba8())
    }

    /// 从字节载入。
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let img = image::load_from_memory(bytes)?;
        Self::from_image(img.to_rgba8())
    }

    /// 包裹现成图像。
    pub fn from_image(img: RgbaImage) -> Result<Self> {
        guard_size(img.width(), img.height())?;
        Ok(Self {
            img,
            fonts: HashMap::new(),
        })
    }

    pub fn width(&self) -> u32 {
        self.img.width()
    }

    pub fn height(&self) -> u32 {
        self.img.height()
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width(), self.height())
    }

    pub fn image(&self) -> &RgbaImage {
        &self.img
    }

    pub fn image_mut(&mut self) -> &mut RgbaImage {
        &mut self.img
    }

    pub fn into_image(self) -> RgbaImage {
        self.img
    }

    // ── 字体 ──────────────────────────────────────────────

    /// 取（并缓存）指定路径的字体。
    pub fn font(&mut self, path: &Path) -> Result<&Font> {
        if !self.fonts.contains_key(path) {
            let font = Font::load(path)?;
            self.fonts.insert(path.to_path_buf(), font);
        }
        Ok(self.fonts.get(path).expect("刚插入"))
    }

    /// 解析文字选项对应的字体路径（显式 > 配置 > 随包默认）。
    fn resolve_font_path(explicit: Option<&Path>) -> PathBuf {
        config::resolve_font(config::global(), explicit)
    }

    // ── 变换 ──────────────────────────────────────────────

    /// 缩放到精确尺寸。
    pub fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        guard_size(width, height)?;
        self.img = image::imageops::resize(&self.img, width, height, image::imageops::FilterType::Triangle);
        Ok(())
    }

    /// 旋转（正角度顺时针，围绕画布中心，画布自动扩展；`bg` 支持 `transparent`）。
    pub fn rotate(&mut self, angle: f32, bg: &str) -> Result<()> {
        let bg_color = if bg.eq_ignore_ascii_case("transparent") {
            Rgba([0, 0, 0, 0])
        } else {
            color::parse(bg)?
        };
        self.img = rotate_image(&self.img, angle, bg_color);
        Ok(())
    }

    /// 缩放到正方形并裁剪成圆。
    pub fn circle(&mut self, diameter: u32) -> Result<()> {
        guard_size(diameter, diameter)?;
        self.resize(diameter, diameter)?;
        if diameter > 1 {
            self.img = rounded_image(&self.img, diameter / 2);
        }
        Ok(())
    }

    /// 裁剪出 `[x, x+width) × [y, y+height)`（越界处透明，同 `imagecopy`）。
    pub fn crop(&mut self, x: i32, y: i32, width: u32, height: u32) -> Result<()> {
        guard_size(width, height)?;
        let mut out = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 0]));
        let (sw, sh) = self.size();
        for oy in 0..height {
            let sy = y + oy as i32;
            if sy < 0 || sy as u32 >= sh {
                continue;
            }
            for ox in 0..width {
                let sx = x + ox as i32;
                if sx < 0 || sx as u32 >= sw {
                    continue;
                }
                out.put_pixel(ox, oy, *self.img.get_pixel(sx as u32, sy as u32));
            }
        }
        self.img = out;
        Ok(())
    }

    /// 圆角副本（不修改自身）；半径夹取到短边一半。
    pub fn rounded(&self, radius: u32) -> Self {
        Self {
            img: rounded_image(&self.img, radius),
            fonts: HashMap::new(),
        }
    }

    // ── 绘制 ──────────────────────────────────────────────

    /// 文字；`(x, y)` 为基线锚点，选项同 PHP `text()`。
    pub fn text(&mut self, text: &str, x: f32, y: f32, opts: &TextOptions) -> Result<()> {
        let font_path = Self::resolve_font_path(opts.font.as_deref());
        let Self { img, fonts } = self;
        if !fonts.contains_key(&font_path) {
            fonts.insert(font_path.clone(), Font::load(&font_path)?);
        }
        let font = fonts.get(&font_path).expect("刚插入");

        let rgba = color::parse(&opts.color)?;
        let line_height = opts
            .line_height
            .unwrap_or(opts.size * text::DEFAULT_LINE_HEIGHT_RATIO);
        let lines = font.wrap(text, opts.size, opts.max_width);
        for (i, line) in lines.iter().enumerate() {
            let line_w = font.measure(line, opts.size);
            let lx = match opts.align {
                TextAlign::Left => x,
                TextAlign::Center => x - line_w / 2.0,
                TextAlign::Right => x - line_w,
            };
            font.draw_line(img, line, lx, y + i as f32 * line_height, opts.size, rgba, opts.angle);
        }
        Ok(())
    }

    /// 叠加图片（缩放 → 圆角 → 阴影 → 合成），选项同 PHP `image()`。
    pub fn overlay(&mut self, ov: &ImageDriver, x: i32, y: i32, opts: &OverlayOptions) -> Result<()> {
        let (dw, dh) = (
            opts.width.unwrap_or(ov.width()),
            opts.height.unwrap_or(ov.height()),
        );
        guard_size(dw, dh)?;

        // 先缩到目标尺寸再做圆角/阴影（与 PHP 一致，代价按目标像素算）
        let mut src = ov.img.clone();
        if (dw, dh) != (ov.width(), ov.height()) {
            src = image::imageops::resize(&src, dw, dh, image::imageops::FilterType::Triangle);
        }
        if opts.radius > 0 {
            src = rounded_image(&src, opts.radius);
        }
        if let Some(shadow) = &opts.shadow {
            self.draw_shadow(shadow, x, y, dw, dh)?;
        }
        blend_overlay(&mut self.img, &src, x as i64, y as i64);
        Ok(())
    }

    /// 矩形（`radius > 0` 且填充时为圆角矩形）。
    pub fn rectangle(&mut self, x: i32, y: i32, width: u32, height: u32, opts: &ShapeOptions) -> Result<()> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        let rgba = color::parse_with_opacity(&opts.color, opts.opacity)?;
        if opts.filled {
            if opts.radius > 0 {
                fill_rounded_rect(
                    &mut self.img,
                    x as f32,
                    y as f32,
                    width as f32,
                    height as f32,
                    opts.radius as f32,
                    rgba,
                );
            } else {
                fill_rect(&mut self.img, x as i64, y as i64, width as i64, height as i64, rgba);
            }
        } else {
            let r = |v: i64| v as f32;
            let (x0, y0) = (r(x as i64), r(y as i64));
            let (x1, y1) = (r(x as i64 + width as i64 - 1), r(y as i64 + height as i64 - 1));
            draw_thick_line(&mut self.img, (x0, y0), (x1, y0), 1.0, rgba);
            draw_thick_line(&mut self.img, (x1, y0), (x1, y1), 1.0, rgba);
            draw_thick_line(&mut self.img, (x1, y1), (x0, y1), 1.0, rgba);
            draw_thick_line(&mut self.img, (x0, y1), (x0, y0), 1.0, rgba);
        }
        Ok(())
    }

    /// 椭圆（`rx`/`ry` 为半轴）。
    pub fn ellipse(&mut self, cx: i32, cy: i32, rx: f32, ry: f32, opts: &ShapeOptions) -> Result<()> {
        let rgba = color::parse_with_opacity(&opts.color, opts.opacity)?;
        fill_ellipse(&mut self.img, cx as f32, cy as f32, rx, ry, rgba, opts.filled);
        Ok(())
    }

    /// 饼形扇区（角度：0 = 三点钟方向，顺时针增加，同 GD）。
    pub fn filled_arc(
        &mut self,
        cx: i32,
        cy: i32,
        w: u32,
        h: u32,
        start_angle: f32,
        end_angle: f32,
        opts: &ShapeOptions,
    ) -> Result<()> {
        let rgba = color::parse_with_opacity(&opts.color, opts.opacity)?;
        let (rx, ry) = (w as f32 / 2.0, h as f32 / 2.0);
        let mut points = vec![(cx as f32, cy as f32)];
        let steps = (((end_angle - start_angle).abs() / 3.0).ceil() as usize).max(1);
        for i in 0..=steps {
            let a = (start_angle + (end_angle - start_angle) * i as f32 / steps as f32).to_radians();
            points.push((cx as f32 + rx * a.cos(), cy as f32 + ry * a.sin()));
        }
        fill_polygon(&mut self.img, &points, rgba);
        Ok(())
    }

    /// 多边形（自动闭合；至少 3 个顶点）。
    pub fn polygon(&mut self, points: &[(f32, f32)], opts: &ShapeOptions) -> Result<()> {
        if points.len() < 3 {
            return Err(PosterError::Other(format!(
                "Polygon needs at least 3 points, got {}",
                points.len()
            )));
        }
        let rgba = color::parse_with_opacity(&opts.color, opts.opacity)?;
        if opts.filled {
            fill_polygon(&mut self.img, points, rgba);
        } else {
            for i in 0..points.len() {
                let a = points[i];
                let b = points[(i + 1) % points.len()];
                draw_thick_line(&mut self.img, a, b, 1.0, rgba);
            }
        }
        Ok(())
    }

    /// 直线。
    pub fn line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, opts: &LineOptions) -> Result<()> {
        let rgba = color::parse(&opts.color)?;
        draw_thick_line(
            &mut self.img,
            (x1 as f32, y1 as f32),
            (x2 as f32, y2 as f32),
            opts.width.max(1) as f32,
            rgba,
        );
        Ok(())
    }

    /// 按掩膜裁剪：掩膜透明度过半处，目标像素置空（同 PHP mask 语义）。
    pub fn mask(&mut self, mask: &ImageDriver) {
        let w = self.width().min(mask.width());
        let h = self.height().min(mask.height());
        for y in 0..h {
            for x in 0..w {
                if mask.img.get_pixel(x, y).0[3] < 128 {
                    self.img.put_pixel(x, y, Rgba([0, 0, 0, 0]));
                }
            }
        }
    }

    /// 高斯模糊（大半径走 1/4 降采样路径，同 PHP 的性能策略）。
    pub fn blur(&mut self, radius: u32) {
        if radius < 1 {
            return;
        }
        let sigma = radius as f32 / 2.0;
        if radius <= 2 {
            self.img = image::imageops::blur(&self.img, sigma);
            return;
        }
        let (w, h) = self.size();
        let (sw, sh) = ((w / 4).max(1), (h / 4).max(1));
        let small = image::imageops::resize(&self.img, sw, sh, image::imageops::FilterType::Triangle);
        let passes = (radius as f32 / 2.0).clamp(1.0, 8.0);
        let small = image::imageops::blur(&small, passes * 0.6);
        self.img = image::imageops::resize(&small, w, h, image::imageops::FilterType::Triangle);
    }

    /// 锐化：3×3 卷积核，中心 `4a+1`、四邻 `-a`（同 PHP 的 imageconvolution 参数）。
    pub fn sharpen(&mut self, amount: f32) {
        let a = amount.clamp(0.0, 3.0);
        if a == 0.0 {
            return;
        }
        let kernel: [f32; 9] = [0.0, -a, 0.0, -a, 4.0 * a + 1.0, -a, 0.0, -a, 0.0];
        self.img = convolve3x3(&self.img, &kernel);
    }

    /// 马赛克：按块均值降采样再最近邻放大。
    pub fn pixelate(&mut self, block: u32) {
        let block = block.max(1);
        let (w, h) = self.size();
        let (sw, sh) = ((w / block).max(1), (h / block).max(1));
        let small = image::imageops::resize(&self.img, sw, sh, image::imageops::FilterType::Triangle);
        self.img = image::imageops::resize(&small, w, h, image::imageops::FilterType::Nearest);
    }

    // ── 输出 ──────────────────────────────────────────────

    /// 保存到文件（自动建目录）；`format` 缺省按扩展名推断。
    pub fn save(&self, path: &Path, format: Option<&str>, quality: Option<u8>) -> Result<()> {
        if let Some(dir) = path.parent()
            && !dir.as_os_str().is_empty()
        {
            std::fs::create_dir_all(dir)?;
        }
        let format = normalize_format(format.or_else(|| path.extension().and_then(|e| e.to_str())));
        let bytes = self.encode(&format, quality)?;
        std::fs::write(path, bytes)?;
        Ok(())
    }

    /// 编码为 base64 data URI（同 PHP `output()`）。
    pub fn output(&self, format: &str, quality: Option<u8>) -> Result<String> {
        use base64::Engine;
        let format = normalize_format(Some(format));
        let bytes = self.encode(&format, quality)?;
        Ok(format!(
            "data:{};base64,{}",
            mime_of(&format),
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ))
    }

    /// 编码为字节（集成层直出 `image/png` 用）。
    pub fn encode(&self, format: &str, quality: Option<u8>) -> Result<Vec<u8>> {
        let cfg = config::global();
        let format = normalize_format(Some(format));
        let mut buf = Vec::new();
        match format.as_str() {
            "png" => {
                let level = match quality {
                    // 显式 quality 仍是 0-100，按「质量越高压缩越少」映射到 0-9（同 PHP）
                    Some(q) => ((100 - q.min(100)) as f32 * 9.0 / 100.0).round() as u8,
                    None => cfg.poster.png_compression,
                };
                let compression = match level {
                    0..=3 => CompressionType::Fast,
                    4..=6 => CompressionType::Default,
                    _ => CompressionType::Best,
                };
                PngEncoder::new_with_quality(&mut buf, compression, FilterType::Adaptive)
                    .write_image(
                        self.img.as_raw(),
                        self.width(),
                        self.height(),
                        ExtendedColorType::Rgba8,
                    )?;
            }
            "gif" => {
                GifEncoder::new(&mut buf).encode(
                    self.img.as_raw(),
                    self.width(),
                    self.height(),
                    ExtendedColorType::Rgba8,
                )?;
            }
            "webp" => {
                WebPEncoder::new_lossless(&mut buf).encode(
                    self.img.as_raw(),
                    self.width(),
                    self.height(),
                    ExtendedColorType::Rgba8,
                )?;
            }
            _ => {
                let q = quality.unwrap_or(cfg.image.quality).min(100);
                let rgb = image::DynamicImage::ImageRgba8(self.img.clone()).to_rgb8();
                JpegEncoder::new_with_quality(&mut buf, q).encode(
                    rgb.as_raw(),
                    rgb.width(),
                    rgb.height(),
                    ExtendedColorType::Rgb8,
                )?;
            }
        }
        Ok(buf)
    }

    // ── 内部 ──────────────────────────────────────────────

    /// 阴影：1/4 降采样画实心块 → 模糊 → 放大上色合成（同 PHP `drawShadowGD` 策略）。
    fn draw_shadow(&mut self, shadow: &crate::drivers::ShadowOptions, x: i32, y: i32, w: u32, h: u32) -> Result<()> {
        let rgba = color::parse_with_opacity(&shadow.color, shadow.opacity)?;
        let blur = shadow.blur as i32;
        let pad = blur * 2;
        let sw = (w as i32 + pad).max(1) as u32;
        let sh = (h as i32 + pad).max(1) as u32;

        let (sw2, sh2) = ((sw / 4).max(1), (sh / 4).max(1));
        let mut mask = RgbaImage::from_pixel(sw2, sh2, Rgba([0, 0, 0, 0]));
        let (b2, w2, h2) = (
            (blur / 4).max(0) as i64,
            (w as i64 / 4).max(1),
            (h as i64 / 4).max(1),
        );
        fill_rect(&mut mask, b2, b2, w2, h2, Rgba([255, 255, 255, 255]));

        let passes = (blur as f32 / 2.0).clamp(1.0, 8.0);
        let mask = image::imageops::blur(&mask, passes * 0.6);
        let mask = image::imageops::resize(&mask, sw, sh, image::imageops::FilterType::Triangle);

        let mut shadow_img = RgbaImage::from_pixel(sw, sh, Rgba([0, 0, 0, 0]));
        for (px, py, p) in mask.enumerate_pixels() {
            let lum = p.0[0];
            if lum == 0 {
                continue;
            }
            let alpha = (lum as f32 / 255.0 * rgba.0[3] as f32).round() as u8;
            shadow_img.put_pixel(px, py, Rgba([rgba.0[0], rgba.0[1], rgba.0[2], alpha]));
        }

        blend_overlay(
            &mut self.img,
            &shadow_img,
            (x + shadow.offset_x - blur) as i64,
            (y + shadow.offset_y - blur) as i64,
        );
        Ok(())
    }
}

// ── 像素工具 ──────────────────────────────────────────────

fn guard_size(width: u32, height: u32) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(PosterError::Other(format!(
            "Width and height must be greater than 0, got {width}x{height}"
        )));
    }
    if width as u64 * height as u64 > MAX_PIXELS {
        return Err(PosterError::Other(format!(
            "Image too large: {width}x{height} exceeds the pixel budget of {MAX_PIXELS} pixels"
        )));
    }
    Ok(())
}

/// src-over 混合单个像素。
fn blend_pixel(dst: &mut Rgba<u8>, src: Rgba<u8>) {
    let sa = src.0[3] as f32 / 255.0;
    if sa <= 0.0 {
        return;
    }
    if sa >= 1.0 {
        *dst = src;
        return;
    }
    let da = dst.0[3] as f32 / 255.0;
    let out_a = sa + da * (1.0 - sa);
    for i in 0..3 {
        let s = src.0[i] as f32;
        let d = dst.0[i] as f32;
        dst.0[i] = ((s * sa + d * da * (1.0 - sa)) / out_a).round() as u8;
    }
    dst.0[3] = (out_a * 255.0).round() as u8;
}

/// 以 `coverage`（0-1）缩放 alpha 后混合。
fn blend_pixel_coverage(img: &mut RgbaImage, x: i64, y: i64, color: Rgba<u8>, coverage: f32) {
    if x < 0 || y < 0 || x as u32 >= img.width() || y as u32 >= img.height() || coverage <= 0.0 {
        return;
    }
    let mut c = color;
    c.0[3] = (c.0[3] as f32 * coverage.clamp(0.0, 1.0)).round() as u8;
    blend_pixel(img.get_pixel_mut(x as u32, y as u32), c);
}

/// 填充矩形 `[x, x+w) × [y, y+h)`。
fn fill_rect(img: &mut RgbaImage, x: i64, y: i64, w: i64, h: i64, color: Rgba<u8>) {
    for py in y.max(0)..(y + h).min(img.height() as i64) {
        for px in x.max(0)..(x + w).min(img.width() as i64) {
            blend_pixel(img.get_pixel_mut(px as u32, py as u32), color);
        }
    }
}

/// 圆角矩形填充：两个交叉矩形 + 四角圆（SDF 抗锯齿）。
fn fill_rounded_rect(img: &mut RgbaImage, x: f32, y: f32, w: f32, h: f32, radius: f32, color: Rgba<u8>) {
    let r = radius.min(w / 2.0).min(h / 2.0);
    let (x0, y0, x1, y1) = (x, y, x + w, y + h);
    for py in (y0.floor().max(0.0) as i64)..((y1.ceil()).min(img.height() as f32) as i64) {
        for px in (x0.floor().max(0.0) as i64)..((x1.ceil()).min(img.width() as f32) as i64) {
            let (cx, cy) = (px as f32 + 0.5, py as f32 + 0.5);
            let dx = (x0 + r - cx).max(cx - (x1 - r)).max(0.0);
            let dy = (y0 + r - cy).max(cy - (y1 - r)).max(0.0);
            let dist = (dx * dx + dy * dy).sqrt();
            let coverage = (r - dist + 0.5).clamp(0.0, 1.0);
            blend_pixel_coverage(img, px, py, color, coverage);
        }
    }
}

/// 填充椭圆（`filled = false` 时画轮廓）。
fn fill_ellipse(img: &mut RgbaImage, cx: f32, cy: f32, rx: f32, ry: f32, color: Rgba<u8>, filled: bool) {
    if rx <= 0.0 || ry <= 0.0 {
        return;
    }
    let (x0, x1) = ((cx - rx - 1.0).floor().max(0.0) as i64, (cx + rx + 1.0).ceil());
    let (y0, y1) = ((cy - ry - 1.0).floor().max(0.0) as i64, (cy + ry + 1.0).ceil());
    for py in y0..(y1.min(img.height() as f32) as i64) {
        for px in x0..(x1.min(img.width() as f32) as i64) {
            let (dx, dy) = (px as f32 + 0.5 - cx, py as f32 + 0.5 - cy);
            let norm = ((dx / rx).powi(2) + (dy / ry).powi(2)).sqrt();
            let coverage = if filled {
                1.0 - smoothstep_edge(norm, 1.0, 2.0 / rx.max(ry))
            } else {
                let band = smoothstep_edge(norm, 1.0, 2.0 / rx.max(ry));
                band * (1.0 - smoothstep_edge(norm, 1.0 + 2.0 / rx.min(ry), 2.0 / rx.min(ry)))
            };
            blend_pixel_coverage(img, px, py, color, coverage);
        }
    }
}

/// 归一化距离 `v` 距 `edge` 的覆盖率（`width` 为过渡带宽）。
fn smoothstep_edge(v: f32, edge: f32, width: f32) -> f32 {
    ((v - edge) / width.max(1e-6) + 0.5).clamp(0.0, 1.0)
}

/// 扫描线填充多边形（非零环绕；点按像素中心采样）。
fn fill_polygon(img: &mut RgbaImage, points: &[(f32, f32)], color: Rgba<u8>) {
    if points.len() < 3 {
        return;
    }
    let y_min = points.iter().map(|p| p.1).fold(f32::INFINITY, f32::min).floor().max(0.0);
    let y_max = points.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max).ceil();
    let y_max = y_max.min(img.height() as f32) as i64;
    for py in y_min as i64..y_max {
        let sy = py as f32 + 0.5;
        let mut spans: Vec<f32> = Vec::new();
        for i in 0..points.len() {
            let (x0, y0) = points[i];
            let (x1, y1) = points[(i + 1) % points.len()];
            if (y0 <= sy && y1 > sy) || (y1 <= sy && y0 > sy) {
                spans.push(x0 + (sy - y0) / (y1 - y0) * (x1 - x0));
            }
        }
        spans.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        for pair in spans.chunks(2) {
            if let [xa, xb] = pair {
                for px in (xa.floor().max(0.0) as i64)..(xb.ceil().min(img.width() as f32) as i64) {
                    let coverage = (xb - (px as f32 + 0.5)).min((px as f32 + 0.5) - xa).min(0.5) + 0.5;
                    blend_pixel_coverage(img, px, py, color, coverage.clamp(0.0, 1.0));
                }
            }
        }
    }
}

/// 粗线：段距离 + 圆头抗锯齿。
fn draw_thick_line(img: &mut RgbaImage, a: (f32, f32), b: (f32, f32), width: f32, color: Rgba<u8>) {
    let half = (width / 2.0).max(0.5);
    let (x0, x1) = ((a.0.min(b.0) - half - 1.0).floor().max(0.0), (a.0.max(b.0) + half + 1.0).ceil());
    let (y0, y1) = ((a.1.min(b.1) - half - 1.0).floor().max(0.0), (a.1.max(b.1) + half + 1.0).ceil());
    for py in y0 as i64..(y1.min(img.height() as f32) as i64) {
        for px in x0 as i64..(x1.min(img.width() as f32) as i64) {
            let (cx, cy) = (px as f32 + 0.5, py as f32 + 0.5);
            let dist = point_segment_distance((cx, cy), a, b);
            let coverage = (half + 0.5 - dist).clamp(0.0, 1.0);
            blend_pixel_coverage(img, px, py, color, coverage);
        }
    }
}

fn point_segment_distance(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (abx, aby) = (b.0 - a.0, b.1 - a.1);
    let (apx, apy) = (p.0 - a.0, p.1 - a.1);
    let len2 = abx * abx + aby * aby;
    let t = if len2 <= f32::EPSILON {
        0.0
    } else {
        ((apx * abx + apy * aby) / len2).clamp(0.0, 1.0)
    };
    let (dx, dy) = (apx - abx * t, apy - aby * t);
    (dx * dx + dy * dy).sqrt()
}

/// 圆角遮罩副本（SDF + 抗锯齿）。
fn rounded_image(img: &RgbaImage, radius: u32) -> RgbaImage {
    let (w, h) = (img.width(), img.height());
    let r = radius.min(w.min(h) / 2) as f32;
    let mut out = img.clone();
    if r < 1.0 {
        return out;
    }
    for y in 0..h {
        for x in 0..w {
            let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
            let dx = (r - cx).max(cx - (w as f32 - r)).max(0.0);
            let dy = (r - cy).max(cy - (h as f32 - r)).max(0.0);
            let coverage = (r - (dx * dx + dy * dy).sqrt() + 0.5).clamp(0.0, 1.0);
            let p = out.get_pixel_mut(x, y);
            p.0[3] = (p.0[3] as f32 * coverage).round() as u8;
        }
    }
    out
}

/// src-over 合成（含越界裁剪，`x`/`y` 可为负）。
fn blend_overlay(dst: &mut RgbaImage, src: &RgbaImage, x: i64, y: i64) {
    for sy in 0..src.height() as i64 {
        let dy = y + sy;
        if dy < 0 || dy >= dst.height() as i64 {
            continue;
        }
        for sx in 0..src.width() as i64 {
            let dx = x + sx;
            if dx < 0 || dx >= dst.width() as i64 {
                continue;
            }
            let s = *src.get_pixel(sx as u32, sy as u32);
            if s.0[3] == 0 {
                continue;
            }
            blend_pixel(dst.get_pixel_mut(dx as u32, dy as u32), s);
        }
    }
}

/// 旋转并扩展画布：正角度顺时针（同 PHP `imagerotate(-$angle)`），逆映射 + 双线性采样。
fn rotate_image(src: &RgbaImage, angle_deg: f32, bg: Rgba<u8>) -> RgbaImage {
    let (sw, sh) = (src.width() as f32, src.height() as f32);
    let rad = angle_deg.to_radians();
    let (sin, cos) = (rad.sin(), rad.cos());
    let nw = (sw * cos.abs() + sh * sin.abs() - 1e-4).ceil().max(1.0) as u32;
    let nh = (sw * sin.abs() + sh * cos.abs() - 1e-4).ceil().max(1.0) as u32;
    let mut out = RgbaImage::from_pixel(nw, nh, bg);
    let (scx, scy) = (sw / 2.0, sh / 2.0);
    let (dcx, dcy) = (nw as f32 / 2.0, nh as f32 / 2.0);
    for oy in 0..nh {
        for ox in 0..nw {
            let (u, v) = (ox as f32 + 0.5 - dcx, oy as f32 + 0.5 - dcy);
            // 顺时针 θ 的逆映射
            let sx = u * cos + v * sin + scx;
            let sy = -u * sin + v * cos + scy;
            if sx < 0.0 || sy < 0.0 || sx >= sw || sy >= sh {
                continue; // 保留背景
            }
            out.put_pixel(ox, oy, sample_bilinear(src, sx - 0.5, sy - 0.5));
        }
    }
    out
}

/// 双线性采样（越界分量按透明参与，边缘轻微发暗可接受）。
fn sample_bilinear(src: &RgbaImage, x: f32, y: f32) -> Rgba<u8> {
    let (w, h) = (src.width() as i64, src.height() as i64);
    let (x0, y0) = (x.floor() as i64, y.floor() as i64);
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let get = |px: i64, py: i64| -> [f32; 4] {
        if px < 0 || py < 0 || px >= w || py >= h {
            [0.0; 4]
        } else {
            let p = src.get_pixel(px as u32, py as u32).0;
            [p[0] as f32, p[1] as f32, p[2] as f32, p[3] as f32]
        }
    };
    let (p00, p10, p01, p11) = (get(x0, y0), get(x0 + 1, y0), get(x0, y0 + 1), get(x0 + 1, y0 + 1));
    let mut out = [0u8; 4];
    for i in 0..4 {
        let top = p00[i] + (p10[i] - p00[i]) * fx;
        let bot = p01[i] + (p11[i] - p01[i]) * fx;
        out[i] = (top + (bot - top) * fy).round().clamp(0.0, 255.0) as u8;
    }
    Rgba(out)
}

/// 3×3 卷积（RGB 通道；alpha 原样保留）。
fn convolve3x3(img: &RgbaImage, kernel: &[f32; 9]) -> RgbaImage {
    let (w, h) = (img.width(), img.height());
    let mut out = img.clone();
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0f32; 3];
            for (ki, &k) in kernel.iter().enumerate() {
                let (kx, ky) = ((ki % 3) as i64 - 1, (ki / 3) as i64 - 1);
                let (sx, sy) = (x as i64 + kx, y as i64 + ky);
                if sx < 0 || sy < 0 || sx >= w as i64 || sy >= h as i64 {
                    continue;
                }
                let p = img.get_pixel(sx as u32, sy as u32);
                for c in 0..3 {
                    acc[c] += p.0[c] as f32 * k;
                }
            }
            let p = out.get_pixel_mut(x, y);
            for c in 0..3 {
                p.0[c] = acc[c].clamp(0.0, 255.0).round() as u8;
            }
        }
    }
    out
}

fn normalize_format(format: Option<&str>) -> String {
    match format.map(|f| f.to_ascii_lowercase()) {
        Some(f) if f == "jpeg" || f == "jpg" => "jpg".into(),
        Some(f) if ["png", "gif", "webp"].contains(&f.as_str()) => f,
        _ => "jpg".into(),
    }
}

fn mime_of(format: &str) -> &'static str {
    match format {
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "image/jpeg",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_fill() {
        let c = ImageDriver::create(10, 10).unwrap();
        assert_eq!(c.size(), (10, 10));
        assert_eq!(c.image().get_pixel(0, 0).0[3], 0);
        let c = ImageDriver::filled(4, 4, "#FF0000").unwrap();
        assert_eq!(c.image().get_pixel(3, 3).0, [255, 0, 0, 255]);
    }

    #[test]
    fn guard_rejects_bad_sizes() {
        assert!(ImageDriver::create(0, 10).is_err());
        assert!(ImageDriver::create(10_000, 10_000).is_err()); // 1 亿像素 > 4000 万
    }

    #[test]
    fn rectangle_covers_half_open_range() {
        let mut c = ImageDriver::create(10, 10).unwrap();
        let opts = ShapeOptions {
            color: "#00FF00".into(),
            ..Default::default()
        };
        c.rectangle(2, 2, 3, 3, &opts).unwrap();
        assert_eq!(c.image().get_pixel(2, 2).0, [0, 255, 0, 255]);
        assert_eq!(c.image().get_pixel(4, 4).0, [0, 255, 0, 255]);
        assert_eq!(c.image().get_pixel(5, 5).0[3], 0);
    }

    #[test]
    fn rounded_corners_clear_corner_pixels() {
        let mut c = ImageDriver::filled(20, 20, "#FF0000").unwrap();
        c = c.rounded(6);
        assert_eq!(c.image().get_pixel(0, 0).0[3], 0, "角落应被裁掉");
        assert_eq!(c.image().get_pixel(10, 10).0[3], 255, "中心应保留");
    }

    #[test]
    fn mask_clears_where_transparent() {
        let mut c = ImageDriver::filled(4, 4, "#FF0000").unwrap();
        let mut mask = ImageDriver::create(4, 4).unwrap();
        mask.image_mut().put_pixel(1, 1, Rgba([0, 0, 0, 255]));
        c.mask(&mask);
        assert_eq!(c.image().get_pixel(1, 1).0[3], 255, "掩膜不透明处保留");
        assert_eq!(c.image().get_pixel(0, 0).0[3], 0, "掩膜透明处置空");
    }

    #[test]
    fn rotate_90_clockwise_moves_top_to_right() {
        let mut c = ImageDriver::filled(10, 20, "#000000").unwrap();
        // 顶边中点画红点
        c.rectangle(4, 0, 2, 2, &ShapeOptions {
            color: "#FF0000".into(),
            ..Default::default()
        }).unwrap();
        c.rotate(90.0, "transparent").unwrap();
        assert_eq!(c.size(), (20, 10), "画布应扩展");
        // 顺时针 90°：顶部 → 右侧
        let p = c.image().get_pixel(19, 4);
        assert!(p.0[0] > 200 && p.0[1] < 60, "顶部应转到右侧, got {p:?}");
    }

    #[test]
    fn text_draws_pixels_and_respects_align() {
        let mut c = ImageDriver::filled(200, 60, "#FFFFFF").unwrap();
        let opts = TextOptions {
            size: 24.0,
            color: "#000000".into(),
            ..Default::default()
        };
        c.text("Hi", 10.0, 40.0, &opts).unwrap();
        let dark = c.image().pixels().filter(|p| p.0[0] < 128).count();
        assert!(dark > 20, "文字应画出像素, got {dark}");
    }

    #[test]
    fn output_is_data_uri_of_png() {
        let c = ImageDriver::filled(4, 4, "#000000").unwrap();
        let uri = c.output("png", None).unwrap();
        assert!(uri.starts_with("data:image/png;base64,"));
    }

    #[test]
    fn encode_jpg_drops_alpha() {
        use base64::Engine;
        let c = ImageDriver::filled(4, 4, "#123456").unwrap();
        let uri = c.output("jpg", Some(90)).unwrap();
        let b64 = uri.strip_prefix("data:image/jpeg;base64,").unwrap();
        let bytes = base64::engine::general_purpose::STANDARD.decode(b64).unwrap();
        assert_eq!(&bytes[..2], &[0xFF, 0xD8], "应是 JPEG 魔数");
    }
}
