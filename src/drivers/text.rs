//! TTF 文字：测量、自动换行（CJK 逐字 / 拉丁按词）、绘制（含旋转）。
//!
//! 对应 PHP 版 `Drivers/TextTrait` + `GdDriver::text()`：
//! - 换行规则一致：含 CJK 按字符切、否则按空白切词；断点用累加宽度，行尾用整行实测校正。
//! - `y` 是基线位置（同 `imagettftext`），`align` 时 `x` 是中/右锚点。
//!
//! 绘制带一层整行缓存：同一字体实例上 `(text, size, color, angle)` 相同的行只光栅化一次
//! （水印/描边艺术字会重复画同一行几十次）。命中与未命中的像素**逐字节一致**：
//! 未旋转分支缓存的是原序覆盖事件、按同一套加权求和回放；旋转分支缓存的就是贴回去的那块位图。

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ab_glyph::{Font as _, FontArc, GlyphId, PxScale, ScaleFont as _, point};
use image::{Rgba, RgbaImage};
use imageproc::drawing::draw_text_mut;
use imageproc::geometric_transformations::{Interpolation, rotate_about_center};
use imageproc::pixelops::weighted_sum;

use crate::error::{PosterError, Result};

/// 字体实例 id 分配器（缓存键的一部分，`load` / `from_bytes` 每次自增）。
static NEXT_FONT_ID: AtomicU64 = AtomicU64::new(1);

/// 整行缓存上限：条目数与总字节数双上限，超限整体清空。
const MAX_CACHE_ENTRIES: usize = 256;
const MAX_CACHE_BYTES: usize = 16 << 20;

/// 已加载的字体（内部 `FontArc`，可廉价克隆/共享；克隆共享同一份行缓存）。
#[derive(Clone)]
pub struct Font {
    inner: FontArc,
    id: u64,
    cache: Arc<Mutex<LineCache>>,
}

/// 一次字形覆盖：相对「绘制原点」的像素偏移 + 覆盖度。
///
/// `a` 就是 `imageproc` 交给 `weighted_sum` 的 gv（clamp 后，未再量化），
/// 回放走同一套加权求和，所以与逐字形绘制逐比特一致。
struct Cover {
    dx: i32,
    dy: i32,
    a: f32,
}

/// 缓存条目：未旋转存覆盖事件（可回放到任意画布），旋转存整块已旋转位图。
#[derive(Clone)]
enum LineEntry {
    Events(Arc<Vec<Cover>>),
    Rotated(Arc<RgbaImage>),
}

impl LineEntry {
    fn bytes(&self) -> usize {
        match self {
            LineEntry::Events(events) => events.len() * std::mem::size_of::<Cover>(),
            LineEntry::Rotated(tile) => tile.as_raw().len(),
        }
    }
}

/// 缓存键：(字体实例 id, 文本, size 位模式, RGBA, 角度位模式)。
type LineKey = (u64, String, u32, [u8; 4], u32);

#[derive(Default)]
struct LineCache {
    map: HashMap<LineKey, LineEntry>,
    bytes: usize,
}

/// 行高缺省比例，与 PHP 一致：`size * 1.5`。
pub const DEFAULT_LINE_HEIGHT_RATIO: f32 = 1.5;

impl Font {
    /// 从文件加载字体。
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path)
            .map_err(|e| PosterError::Font(format!("字体不可读 {}: {e}", path.display())))?;
        Self::from_bytes(bytes)
    }

    /// 从字节加载字体。
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        FontArc::try_from_vec(bytes)
            .map(|inner| Self {
                inner,
                id: NEXT_FONT_ID.fetch_add(1, Ordering::Relaxed),
                cache: Arc::new(Mutex::new(LineCache::default())),
            })
            .map_err(|e| PosterError::Font(format!("字体解析失败: {e}")))
    }

    /// PHP 的 `size` 到 ab_glyph `PxScale` 的换算系数。
    ///
    /// 两处口径差一次校正：
    /// 1. PHP `imagettftext($size)` 的 size 是**磅**，96 dpi 下 1 磅 = 4/3 像素；
    /// 2. ab_glyph 的 `PxScale` 按「行高」（ascent−descent）而非 em 缩放。
    ///
    /// 于是同数值 `size` 在 Rust 与 PHP 的视觉大小一致（与 PHP 版对拍实测：size=48
    /// 的「海报Ag」墨迹高均约 65px）。
    fn gd_scale(&self) -> f32 {
        // 字体未声明 unitsPerEm 的罕见情况按 1000 兜底（TrueType 惯例值）
        let upem = self.inner.units_per_em().unwrap_or(1000.0);
        (96.0 / 72.0) * (self.inner.height_unscaled() / upem)
    }

    /// 把 PHP 口径的 size 换算成 ab_glyph 的像素尺度。
    fn px_scale(&self, size: f32) -> PxScale {
        PxScale::from(size * self.gd_scale())
    }

    /// 单行文本的推进宽度（px）。
    pub fn measure(&self, text: &str, size: f32) -> f32 {
        let scaled = self.inner.as_scaled(self.px_scale(size));
        text.chars()
            .map(|c| scaled.h_advance(scaled.glyph_id(c)))
            .sum()
    }

    /// 基线以上的上升高度（px，正数）。
    pub fn ascent(&self, size: f32) -> f32 {
        self.inner.as_scaled(self.px_scale(size)).ascent()
    }

    /// 基线以下的下降高度（px，正数）。
    pub fn descent(&self, size: f32) -> f32 {
        -self.inner.as_scaled(self.px_scale(size)).descent()
    }

    /// 自动换行；`max_width <= 0` 时仅按 `\n` 分段。
    pub fn wrap(&self, text: &str, size: f32, max_width: f32) -> Vec<String> {
        if max_width <= 0.0 {
            return text.split('\n').map(str::to_string).collect();
        }
        let mut lines = Vec::new();
        let mut cache: HashMap<String, f32> = HashMap::new();
        for paragraph in text.split('\n') {
            lines.extend(self.wrap_paragraph(paragraph, size, max_width, &mut cache));
        }
        if lines.is_empty() {
            lines.push(text.to_string());
        }
        lines
    }

    fn wrap_paragraph(
        &self,
        paragraph: &str,
        size: f32,
        max_width: f32,
        cache: &mut HashMap<String, f32>,
    ) -> Vec<String> {
        let width_of = |cache: &mut HashMap<String, f32>, token: &str| -> f32 {
            *cache
                .entry(token.to_string())
                .or_insert_with(|| self.measure(token, size))
        };

        let mut lines: Vec<String> = Vec::new();
        let mut current: Vec<String> = Vec::new();
        let mut current_width = 0.0f32;

        for token in split_tokens(paragraph) {
            if token.is_empty() {
                continue;
            }
            let token_width = width_of(cache, &token);
            if !current.is_empty() && current_width + token_width > max_width {
                // 行尾实测校正：累加宽度与整行测量有偏差（字距），保证每行不超 max_width
                let mut carry: Vec<String> = Vec::new();
                loop {
                    let joined = current.join("");
                    let exact = width_of(cache, &joined);
                    if current.len() > 1 && exact > max_width {
                        carry.insert(0, current.pop().unwrap());
                    } else {
                        break;
                    }
                }
                lines.push(current.join(""));
                current = carry;
                current_width = current.iter().map(|t| width_of(cache, t)).sum();
            }
            current.push(token);
            current_width += token_width;
        }

        // 收尾：末行同样校正
        while !current.is_empty() {
            let mut carry: Vec<String> = Vec::new();
            loop {
                let joined = current.join("");
                let exact = width_of(cache, &joined);
                if current.len() > 1 && exact > max_width {
                    carry.insert(0, current.pop().unwrap());
                } else {
                    break;
                }
            }
            lines.push(current.join(""));
            current = carry;
        }

        lines
    }

    /// 画一行文本；`(x, baseline_y)` 是基线锚点，`angle` 为逆时针度数（0 = 不旋转）。
    ///
    /// 命中整行缓存时按缓存回放，未命中时走原绘制流程并顺手把结果存进缓存；
    /// 两条路径的输出逐字节一致（见 `draw_text_record` / `replay_events`）。
    #[allow(clippy::too_many_arguments)] // 一行文本的完整定位/样式参数
    pub fn draw_line(
        &self,
        img: &mut RgbaImage,
        text: &str,
        x: f32,
        baseline_y: f32,
        size: f32,
        color: Rgba<u8>,
        angle: f32,
    ) {
        if text.is_empty() {
            return;
        }
        let key: LineKey = (self.id, text.to_string(), size.to_bits(), color.0, angle.to_bits());
        // 命中的 Arc 克隆后立刻放锁：回放/贴图不占着缓存锁
        let hit = self.cache.lock().ok().and_then(|c| c.map.get(&key).cloned());

        if angle.abs() < 0.001 {
            let x0 = x.round() as i32;
            let y0 = (baseline_y - self.ascent(size)).round() as i32;
            if let Some(LineEntry::Events(events)) = hit.as_ref() {
                replay_events(img, x0, y0, color, events);
                return;
            }
            let mut events = Vec::new();
            self.draw_text_record(img, color, x0, y0, self.px_scale(size), text, &mut events);
            self.store(key, LineEntry::Events(Arc::new(events)));
            return;
        }

        // 旋转：文本画在以锚点为中心的正方形临时画布上，绕中心旋转后贴回。
        let (side, anchor, asc) = self.rotated_layout(text, size);
        let dx = (x - anchor).round() as i64;
        let dy = (baseline_y - anchor).round() as i64;
        if let Some(LineEntry::Rotated(tile)) = hit.as_ref() {
            image::imageops::overlay(img, &**tile, dx, dy);
            return;
        }
        let mut temp = RgbaImage::from_pixel(side, side, Rgba([0, 0, 0, 0]));
        draw_text_mut(
            &mut temp,
            color,
            anchor.round() as i32,
            (anchor - asc).round() as i32,
            self.px_scale(size),
            &self.inner,
            text,
        );
        let rotated = rotate_about_center(
            &temp,
            angle.to_radians(),
            Interpolation::Bilinear,
            Rgba([0, 0, 0, 0]),
        );
        image::imageops::overlay(img, &rotated, dx, dy);
        self.store(key, LineEntry::Rotated(Arc::new(rotated)));
    }

    /// 旋转分支的临时画布边长、中心锚点与上伸高度（只依赖 text/size，重算很便宜）。
    fn rotated_layout(&self, text: &str, size: f32) -> (u32, f32, f32) {
        let w = self.measure(text, size);
        let asc = self.ascent(size).max(0.0);
        let desc = self.descent(size);
        let extent = asc.max(desc);
        let radius = (w * w + extent * extent).sqrt().ceil() + 8.0;
        let side = (radius * 2.0).ceil().max(2.0) as u32;
        (side, (side / 2) as f32, asc)
    }

    /// 逐字形绘制一行，同时把覆盖事件按绘制顺序记进 `events`。
    ///
    /// 排布与混合逐字对应 `imageproc::drawing::draw_text_mut`（同一个 `weighted_sum`、
    /// 同样的 `bb.min.round()` 定位与裁剪），所以未命中时的输出与旧实现逐像素相同；
    /// 事件本身与画布无关（含画布外的点），回放时再按目标画布裁剪。
    #[allow(clippy::too_many_arguments)] // 与 draw_text_mut 对齐的参数面
    fn draw_text_record(
        &self,
        img: &mut RgbaImage,
        color: Rgba<u8>,
        x: i32,
        y: i32,
        scale: PxScale,
        text: &str,
        events: &mut Vec<Cover>,
    ) {
        let (img_w, img_h) = (img.width() as i32, img.height() as i32);
        let font = self.inner.as_scaled(scale);
        let mut pen = 0.0f32;
        let mut last: Option<GlyphId> = None;
        for c in text.chars() {
            let glyph_id = font.glyph_id(c);
            let glyph = glyph_id.with_scale_and_position(scale, point(pen, font.ascent()));
            pen += font.h_advance(glyph_id);
            let Some(outlined) = font.outline_glyph(glyph) else {
                continue;
            };
            if let Some(last) = last {
                pen += font.kern(glyph_id, last);
            }
            last = Some(glyph_id);
            let bb = outlined.px_bounds();
            let bx = x + bb.min.x.round() as i32;
            let by = y + bb.min.y.round() as i32;
            outlined.draw(|gx, gy, gv| {
                let gv = gv.clamp(0.0, 1.0);
                let px = gx as i32 + bx;
                let py = gy as i32 + by;
                // 未覆盖：加权求和恒等（pixel * 1.0 + color * 0.0 == pixel），记录与绘制都可跳过
                if gv == 0.0 {
                    return;
                }
                events.push(Cover { dx: px - x, dy: py - y, a: gv });
                if !(0..img_w).contains(&px) || !(0..img_h).contains(&py) {
                    return;
                }
                paint(img, px as u32, py as u32, color, gv);
            });
        }
    }

    /// 存一条缓存；超过条目/字节上限就整体清空。
    fn store(&self, key: LineKey, entry: LineEntry) {
        let key_text_len = key.1.len();
        let entry_bytes = entry.bytes() + key_text_len;
        if entry_bytes > MAX_CACHE_BYTES {
            return; // 单条就超上限：不缓存（如超大旋转贴图）
        }
        let Ok(mut cache) = self.cache.lock() else {
            return; // 锁中毒：跳缓存，正常绘制
        };
        if cache.map.len() >= MAX_CACHE_ENTRIES || cache.bytes + entry_bytes > MAX_CACHE_BYTES {
            // ponytail: 超限整体清空（简化淘汰，重复绘制的命中率足够）；要精细命中率再上真 LRU
            cache.map.clear();
            cache.bytes = 0;
        }
        cache.bytes += entry_bytes;
        if let Some(old) = cache.map.insert(key, entry) {
            // 覆盖旧条目（键含角度位，实际不可达）；键相同，文本长度也一并扣回
            cache.bytes -= old.bytes() + key_text_len;
        }
    }
}

/// 把缓存事件按原顺序回放到画布：与未命中时逐字形同序同值、同快慢分档。
fn replay_events(img: &mut RgbaImage, x: i32, y: i32, color: Rgba<u8>, events: &[Cover]) {
    let (img_w, img_h) = (img.width() as i32, img.height() as i32);
    for e in events {
        let px = x + e.dx;
        let py = y + e.dy;
        if (0..img_w).contains(&px) && (0..img_h).contains(&py) {
            paint(img, px as u32, py as u32, color, e.a);
        }
    }
}

/// 按覆盖度落一个像素；两条路径共用，保证同值同结果。
///
/// 全覆盖（gv == 1.0）时 `weighted_sum` 逐通道恒为 `clamp(color * 1.0) == color`，
/// 直接写像素即可 —— 这是命中回放里最省的一档（内点占多数），且与逐字形绘制逐比特一致。
#[inline]
fn paint(img: &mut RgbaImage, px: u32, py: u32, color: Rgba<u8>, coverage: f32) {
    if coverage >= 1.0 {
        img.put_pixel(px, py, color);
        return;
    }
    let pixel = *img.get_pixel(px, py);
    img.put_pixel(px, py, weighted_sum(pixel, color, 1.0 - coverage, coverage));
}

/// 切分换行 token：含 CJK 逐字，否则按空白切词（保留空白 token，与 PHP 一致）。
fn split_tokens(text: &str) -> Vec<String> {
    if text.chars().any(is_cjk) {
        return text.chars().map(|c| c.to_string()).collect();
    }
    let mut tokens = Vec::new();
    let mut buf = String::new();
    let mut in_space = false;
    for c in text.chars() {
        let is_space = c.is_whitespace();
        if !buf.is_empty() && is_space != in_space {
            tokens.push(std::mem::take(&mut buf));
        }
        in_space = is_space;
        buf.push(c);
    }
    if !buf.is_empty() {
        tokens.push(buf);
    }
    tokens
}

fn is_cjk(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_font() -> Font {
        Font::load(&crate::assets::default_font_path()).expect("默认字体应能加载")
    }

    #[test]
    fn gd_size_calibration_matches_php() {
        // 与 PHP 版对拍：imagettftext($size=48) 的「海报Ag」墨迹高 65px、y 68..132
        // （本机 PHP 8.5 + Alibaba PuHuiTi 实测）。改字体或改换算系数时这里会先红。
        let f = test_font();
        let mut canvas = image::RgbaImage::from_pixel(700, 220, Rgba([255, 255, 255, 255]));
        f.draw_line(&mut canvas, "海报Ag", 20.0, 120.0, 48.0, Rgba([0, 0, 0, 255]), 0.0);

        let mut min_y = i32::MAX;
        let mut max_y = i32::MIN;
        for (_, y, p) in canvas.enumerate_pixels() {
            if p.0[0] < 128 {
                min_y = min_y.min(y as i32);
                max_y = max_y.max(y as i32);
            }
        }
        assert_eq!(max_y - min_y + 1, 65, "墨迹高应与 PHP 一致（65px）");
        assert_eq!((min_y, max_y), (68, 132), "基线位置也应与 PHP 一致");
    }

    #[test]
    fn measures_wider_text_as_wider() {
        let f = test_font();
        assert!(f.measure("海报", 40.0) > f.measure("海", 40.0));
        assert!(f.measure("", 40.0) == 0.0);
    }

    #[test]
    fn cjk_wraps_per_character() {
        let f = test_font();
        let lines = f.wrap("海报生成工具包", 40.0, 85.0);
        assert!(lines.len() >= 2, "CJK 应按字断行: {lines:?}");
        for line in &lines {
            assert!(f.measure(line, 40.0) <= 85.0 + 1.0, "行宽超限: {line}");
        }
        // 内容不丢
        assert_eq!(lines.concat(), "海报生成工具包");
    }

    #[test]
    fn latin_wraps_per_word_and_keeps_spaces() {
        let f = test_font();
        let lines = f.wrap("hello world poster", 20.0, f.measure("hello wor", 20.0));
        assert_eq!(lines, vec!["hello ", "world ", "poster"]);
    }

    #[test]
    fn explicit_newlines_are_kept() {
        let f = test_font();
        let lines = f.wrap("上\n下", 20.0, 0.0);
        assert_eq!(lines, vec!["上", "下"]);
    }

    fn canvas() -> RgbaImage {
        RgbaImage::from_pixel(400, 220, Rgba([255, 255, 255, 255]))
    }

    #[test]
    fn cache_hit_is_pixel_identical() {
        let cold = test_font(); // 新实例 = 冷缓存，第一笔画必然未命中

        let mut miss = canvas();
        cold.draw_line(&mut miss, "缓存命中Ag", 30.0, 120.0, 40.0, Rgba([12, 34, 56, 200]), 0.0);
        let mut hit = canvas();
        cold.draw_line(&mut hit, "缓存命中Ag", 30.0, 120.0, 40.0, Rgba([12, 34, 56, 200]), 0.0);
        assert_eq!(miss.as_raw(), hit.as_raw(), "未旋转命中与未命中必须逐像素一致");

        // 旋转分支：同样第一笔未命中、第二笔命中
        let mut miss = canvas();
        cold.draw_line(&mut miss, "旋转Ag", 120.0, 150.0, 36.0, Rgba([200, 10, 10, 255]), 17.5);
        let mut hit = canvas();
        cold.draw_line(&mut hit, "旋转Ag", 120.0, 150.0, 36.0, Rgba([200, 10, 10, 255]), 17.5);
        assert_eq!(miss.as_raw(), hit.as_raw(), "旋转命中与未命中必须逐像素一致");
    }

    #[test]
    fn cache_hit_clips_like_direct_draw() {
        let text = "边缘裁剪 Ag";
        let color = Rgba([0, 0, 0, 255]);
        let warm = test_font();
        let mut first = canvas(); // 未命中：先在同一键上填满缓存
        warm.draw_line(&mut first, text, 300.0, 100.0, 48.0, color, 0.0);

        let mut expect = canvas(); // 冷实例直接画：命中时部分出画布
        test_font().draw_line(&mut expect, text, -60.0, 20.0, 48.0, color, 0.0);
        let mut hit = canvas(); // 命中：换位置，裁剪要与直接绘制一致
        warm.draw_line(&mut hit, text, -60.0, 20.0, 48.0, color, 0.0);
        assert_eq!(expect.as_raw(), hit.as_raw(), "命中与直接绘制应在同一位置同样裁剪");
    }

    #[test]
    fn cache_variants_do_not_collide() {
        let warm = test_font(); // 同一实例反复画，第 2 笔必命中
        let cases: &[(&str, f32, [u8; 4], f32)] = &[
            ("海报Ag", 48.0, [0, 0, 0, 255], 0.0),
            ("海报Ag", 48.0, [255, 0, 0, 255], 0.0), // 颜色不同
            ("海报Ag", 32.0, [0, 0, 0, 255], 0.0),   // size 不同
            ("海报Ag", 48.0, [0, 0, 0, 255], 12.0),  // 角度不同（旋转分支）
            ("海报Ag", 48.0, [0, 0, 0, 128], 0.0),   // 半透明
            ("另一个字", 48.0, [0, 0, 0, 255], 0.0), // 文本不同
        ];
        for &(text, size, rgba, angle) in cases {
            // 冷字体实例 = 未命中基准
            let mut expect = canvas();
            test_font().draw_line(&mut expect, text, 40.0, 130.0, size, Rgba(rgba), angle);
            // 同一实例第 2 笔 = 命中
            let mut first = canvas();
            warm.draw_line(&mut first, text, 40.0, 130.0, size, Rgba(rgba), angle);
            let mut second = canvas();
            warm.draw_line(&mut second, text, 40.0, 130.0, size, Rgba(rgba), angle);
            assert_eq!(expect.as_raw(), second.as_raw(), "命中结果串键: {text} {size} {rgba:?} {angle}");
            assert_eq!(first.as_raw(), second.as_raw(), "复用同一键不一致: {text} {size} {rgba:?} {angle}");
        }
    }

    /// 记录路径必须与 `imageproc::drawing::draw_text_mut` 逐字节一致（含负坐标裁剪），
    /// 否则「未命中 = 旧输出」的前提就破了 —— 也是 imageproc 升级时的漂移哨兵。
    #[test]
    fn record_path_matches_imageproc() {
        let f = test_font();
        let cases: &[(&str, f32, i32, i32)] = &[
            ("海报Ag", 48.0, 20, 60),
            ("cache 缓存命中", 24.0, 0, 0),
            ("AvTo 囗", 40.0, -30, -10), // 部分出画布：裁剪要与 imageproc 一致
            ("海报Ag", 48.0, 380, 200),  // 大部分出画布
        ];
        for &(text, size, x, y) in cases {
            let color = Rgba([7, 200, 30, 180]);
            let mut expect = RgbaImage::from_pixel(300, 200, Rgba([255, 255, 255, 255]));
            draw_text_mut(&mut expect, color, x, y, f.px_scale(size), &f.inner, text);
            let mut actual = RgbaImage::from_pixel(300, 200, Rgba([255, 255, 255, 255]));
            let mut events = Vec::new();
            f.draw_text_record(&mut actual, color, x, y, f.px_scale(size), text, &mut events);
            assert_eq!(expect.as_raw(), actual.as_raw(), "记录路径与 imageproc 不一致: {text}");
        }
    }

    #[test]
    fn cache_stays_within_bounds() {
        let f = test_font();
        for size in 8..280 {
            let mut img = RgbaImage::from_pixel(80, 80, Rgba([255, 255, 255, 255]));
            f.draw_line(&mut img, "A", 4.0, 60.0, size as f32, Rgba([0, 0, 0, 255]), 0.0);
        }
        let cache = f.cache.lock().expect("测试内可 unwrap");
        assert!(cache.map.len() <= MAX_CACHE_ENTRIES, "条目数超上限: {}", cache.map.len());
        assert!(cache.bytes <= MAX_CACHE_BYTES, "字节数超上限: {}", cache.bytes);
    }
}
