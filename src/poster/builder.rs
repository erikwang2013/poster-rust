//! 海报 Builder，对应 PHP `PosterBuilder`：链式拼装 → 渲染 → 保存 / 输出。

use std::cell::{Ref, RefCell};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use image::Rgba;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::assets;
use crate::config;
use crate::drivers::{ImageDriver, ShapeOptions, color};
use crate::error::Result;

use super::elements::{
    Element, ElementRender, RenderCtx, artistic_text::ArtisticTextElement, avatar::AvatarElement,
    calendar::CalendarElement, chart::ChartElement, emoji::EmojiElement, emoticon::EmoticonElement,
    icon::IconElement, image::ImageElement, line::LineElement, qrcode::QrcodeElement,
    shape::ShapeElement, table::TableElement, text::TextElement, watermark::WatermarkElement,
};
use super::template::PosterTemplate;

/// 渐变方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// 自上而下。
    Vertical,
    /// 自左向右。
    Horizontal,
}

/// 海报构建器。
///
/// ```no_run
/// use poster::poster::{PosterBuilder, elements::text::TextElement};
///
/// let mut builder = PosterBuilder::new()?;
/// builder.background("#F7F7F7").add_text("你好", TextElement { y: 120, ..Default::default() });
/// builder.save("out.png", None)?;
/// # Ok::<(), poster::PosterError>(())
/// ```
///
/// 所有 `add_*` 返回 `&mut Self`，可链式调用；`render()` 可重复调用，结果内部
/// 缓存（等价 PHP 的 `$rendered` 标志），无变更时不再重绘。
pub struct PosterBuilder {
    width: Option<u32>,
    height: Option<u32>,
    elements: Vec<Element>,
    template: Option<PosterTemplate>,
    vars: BTreeMap<String, String>,
    replace_elements: bool,
    bg_color: Option<String>,
    bg_image: Option<PathBuf>,
    gradient: Option<(String, String, Direction)>,
    /// 渲染结果缓存；任意变更方法通过 [`Self::invalidate`] 作废。
    /// 构建器按单线程使用（`RefCell`，非 `Sync`）。
    cache: RefCell<Option<ImageDriver>>,
}

impl Default for PosterBuilder {
    fn default() -> Self {
        Self {
            width: None,
            height: None,
            elements: Vec::new(),
            template: None,
            vars: BTreeMap::new(),
            // 默认 true = 模板整体替换手写元素（PHP 的历史行为）
            replace_elements: true,
            bg_color: None,
            bg_image: None,
            gradient: None,
            cache: RefCell::new(None),
        }
    }
}

impl PosterBuilder {
    // ── 画布 ────────────────────────────────────────────

    /// 新建构建器；尺寸缺省取 `poster.default_width` / `poster.default_height`（默认 750×1334）。
    pub fn new() -> Result<Self> {
        Ok(Self::default())
    }

    /// 画布宽（覆盖模板 / 配置默认值）。
    pub fn width(&mut self, width: u32) -> &mut Self {
        self.invalidate();
        self.width = Some(width);
        self
    }

    /// 画布高。
    pub fn height(&mut self, height: u32) -> &mut Self {
        self.invalidate();
        self.height = Some(height);
        self
    }

    /// 背景：`#RRGGBB` 之类色值，或存在的图片路径（图片自动裁剪铺满）。
    ///
    /// 与 PHP 一致：不认识的值直接忽略。三种背景的优先级是渐变 > 图片 > 纯色。
    pub fn background(&mut self, color_or_path: &str) -> &mut Self {
        self.invalidate();
        if is_hex_color(color_or_path) {
            self.bg_color = Some(color_or_path.to_string());
        } else if Path::new(color_or_path).is_file() {
            self.bg_image = Some(PathBuf::from(color_or_path));
        }
        self
    }

    /// 渐变背景（8px 一档色带近似，同 PHP）。
    pub fn background_gradient(&mut self, from: &str, to: &str, direction: Direction) -> &mut Self {
        self.invalidate();
        self.gradient = Some((from.to_string(), to.to_string(), direction));
        self
    }

    // ── 元素 ────────────────────────────────────────────

    /// 文字。
    pub fn add_text(&mut self, text: impl Into<String>, mut options: TextElement) -> &mut Self {
        self.invalidate();
        options.text = text.into();
        self.elements.push(Element::Text(options));
        self
    }

    /// 图片；文件缺失时按 `poster.placeholder` 兜底。
    pub fn add_image(&mut self, src: impl Into<String>, mut options: ImageElement) -> &mut Self {
        self.invalidate();
        options.src = src.into();
        self.elements.push(Element::Image(options));
        self
    }

    /// 项目宠物 Posty（`assets/pet.png`），等价于 `add_image(assets::pet_path(), options)`。
    pub fn add_pet(&mut self, mut options: ImageElement) -> &mut Self {
        self.invalidate();
        options.src = assets::pet_path().to_string_lossy().into_owned();
        self.elements.push(Element::Image(options));
        self
    }

    /// 二维码。
    pub fn add_qrcode(&mut self, content: impl Into<String>, mut options: QrcodeElement) -> &mut Self {
        self.invalidate();
        options.content = content.into();
        self.elements.push(Element::Qrcode(options));
        self
    }

    /// 头像（默认圆形）。
    pub fn add_avatar(&mut self, src: impl Into<String>, mut options: AvatarElement) -> &mut Self {
        self.invalidate();
        options.src = src.into();
        self.elements.push(Element::Avatar(options));
        self
    }

    /// 形状：`rect` / `circle` / `ellipse`。
    pub fn add_shape(&mut self, shape: &str, mut options: ShapeElement) -> &mut Self {
        self.invalidate();
        options.shape = shape.into();
        self.elements.push(Element::Shape(options));
        self
    }

    /// 直线。
    pub fn add_line(&mut self, options: LineElement) -> &mut Self {
        self.invalidate();
        self.elements.push(Element::Line(options));
        self
    }

    /// 平铺水印。
    pub fn add_watermark(
        &mut self,
        text: impl Into<String>,
        mut options: WatermarkElement,
    ) -> &mut Self {
        self.invalidate();
        options.text = text.into();
        self.elements.push(Element::Watermark(options));
        self
    }

    /// 表格。
    pub fn add_table(&mut self, options: TableElement) -> &mut Self {
        self.invalidate();
        self.elements.push(Element::Table(options));
        self
    }

    /// 图表：`bar` / `pie` / `line`。
    pub fn add_chart(&mut self, chart_type: &str, data: Vec<Value>, mut options: ChartElement) -> &mut Self {
        self.invalidate();
        options.chart = chart_type.into();
        options.data = data;
        self.elements.push(Element::Chart(options));
        self
    }

    /// 日历。
    pub fn add_calendar(&mut self, options: CalendarElement) -> &mut Self {
        self.invalidate();
        self.elements.push(Element::Calendar(options));
        self
    }

    /// 艺术字：`stroke` / `shadow` / `gradient` / `neon`。
    pub fn add_artistic_text(
        &mut self,
        text: impl Into<String>,
        style: &str,
        mut options: ArtisticTextElement,
    ) -> &mut Self {
        self.invalidate();
        options.text = text.into();
        options.style = style.into();
        self.elements.push(Element::ArtisticText(options));
        self
    }

    /// Emoji。
    pub fn add_emoji(&mut self, emoji: impl Into<String>, mut options: EmojiElement) -> &mut Self {
        self.invalidate();
        options.emoji = emoji.into();
        self.elements.push(Element::Emoji(options));
        self
    }

    /// 图标（FontAwesome 名，见 `elements::icon::icon_char`）。
    pub fn add_icon(&mut self, icon: &str, mut options: IconElement) -> &mut Self {
        self.invalidate();
        options.icon = icon.into();
        self.elements.push(Element::Icon(options));
        self
    }

    /// 颜文字（预置表情名，见 `elements::emoticon::expressions`）。
    pub fn add_emoticon(
        &mut self,
        expression: &str,
        mut options: EmoticonElement,
    ) -> &mut Self {
        self.invalidate();
        options.expression = expression.into();
        self.elements.push(Element::Emoticon(options));
        self
    }

    /// 按类型名追加元素（类型见 `elements::TYPES`），未知类型报错。
    pub fn add(&mut self, kind: &str, options: Value) -> Result<&mut Self> {
        self.elements.push(Element::from_parts(kind, options)?);
        self.invalidate();
        Ok(self)
    }

    // ── 模板 ────────────────────────────────────────────

    /// 使用模板。
    ///
    /// 默认模板元素**整体替换**手写元素；要两者共存用 [`Self::replace_elements`]`(false)`。
    pub fn use_template(&mut self, template: PosterTemplate) -> &mut Self {
        self.invalidate();
        self.template = Some(template);
        self
    }

    /// `true`（默认）= 模板整体替换手写元素；`false` = 追加，先手写元素后模板元素。
    pub fn replace_elements(&mut self, replace: bool) -> &mut Self {
        self.invalidate();
        self.replace_elements = replace;
        self
    }

    /// 设置模板变量（整体替换，同 PHP 的 `with()`）。
    pub fn with<I, K, V>(&mut self, variables: I) -> &mut Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: ToString,
    {
        self.invalidate();
        self.vars = variables
            .into_iter()
            .map(|(key, value)| (key.into(), value.to_string()))
            .collect();
        self
    }

    /// 导出为模板结构：`PosterTemplate::from_config(builder.to_array())` 可还原等价模板。
    ///
    /// 已设模板时按替换 / 追加语义把模板元素展开进来。变量缺失不在这里报错
    /// （签名不可失败），未替换的 `{{var}}` 原样导出，`render()` 仍会如实报错。
    pub fn to_array(&self) -> Value {
        let (width, height) = self.dimensions();
        let elements: Vec<Value> = self
            .assemble()
            .into_iter()
            .map(|mut element| {
                let _ = element.resolve_vars(&self.vars);
                serde_json::to_value(&element).unwrap_or(Value::Null)
            })
            .collect();
        serde_json::json!({
            "width": width,
            "height": height,
            "elements": elements,
        })
    }

    // ── 渲染与输出 ──────────────────────────────────────

    /// 渲染成画布。
    ///
    /// 无变更时命中缓存（同 PHP 的 `$rendered` 标志），只克隆一次已有画布；
    /// **返回的是缓存画布的副本**，改动它不影响后续调用。任何变更方法
    /// （`width`/`height`/背景/`add_*`/`with` 等）都会作废缓存。
    pub fn render(&self) -> Result<ImageDriver> {
        let cached = self.cached_canvas()?;
        ImageDriver::from_image(cached.image().clone())
    }

    /// 保存到文件：格式按扩展名推断（jpg/jpeg/png/webp/gif，未知扩展名回落 jpg）；
    /// `quality` 为 `None` 时按驱动规则取配置（JPEG 用 `image.quality`，PNG 用 `poster.png_compression`）。
    pub fn save(&self, path: impl AsRef<Path>, quality: Option<u8>) -> Result<()> {
        self.cached_canvas()?.save(path.as_ref(), None, quality)
    }

    /// 输出 data URI（`data:image/png;base64,…`）。
    pub fn output(&self, format: &str, quality: Option<u8>) -> Result<String> {
        self.cached_canvas()?.output(format, quality)
    }

    /// 输出原始编码字节（不经 base64）：直接写 HTTP 响应体用这个，
    /// 需要 data URI 用 [`Self::output`]。格式 / 质量语义与 `output` 一致。
    pub fn output_bytes(&self, format: &str, quality: Option<u8>) -> Result<Vec<u8>> {
        self.cached_canvas()?.encode(format, quality)
    }

    // ── 内部 ────────────────────────────────────────────

    /// 作废渲染缓存；所有 `&mut self` 变更方法都调用它。
    fn invalidate(&mut self) {
        self.cache.get_mut().take();
    }

    /// 借用缓存画布，未命中时先渲染（`save` / `output` / `output_bytes` 用，避免克隆）。
    fn cached_canvas(&self) -> Result<Ref<'_, ImageDriver>> {
        if self.cache.borrow().is_none() {
            *self.cache.borrow_mut() = Some(self.draw()?);
        }
        Ok(Ref::map(self.cache.borrow(), |cache| {
            cache.as_ref().expect("刚渲染")
        }))
    }

    /// 实际绘制（缓存未命中时调用）。
    fn draw(&self) -> Result<ImageDriver> {
        let (width, height) = self.dimensions();
        let mut canvas = self.background_canvas(width, height)?;
        let ctx = RenderCtx {
            config: config::global(),
        };
        for element in &self.build_elements()? {
            element.render(&mut canvas, &ctx)?;
        }
        Ok(canvas)
    }

    /// 生效尺寸：显式 > 模板 > 配置默认。
    fn dimensions(&self) -> (u32, u32) {
        let config = config::global();
        let width = self
            .width
            .or_else(|| self.template.as_ref().map(PosterTemplate::width))
            .unwrap_or(config.poster.default_width);
        let height = self
            .height
            .or_else(|| self.template.as_ref().map(PosterTemplate::height))
            .unwrap_or(config.poster.default_height);
        (width, height)
    }

    /// 元素列表：模板按替换 / 追加语义展开（尚未做变量替换）。
    ///
    /// 替换语义只在设了模板时生效，同 PHP——没模板时手写元素照样参与渲染。
    fn assemble(&self) -> Vec<Element> {
        let mut elements = if self.template.is_some() && self.replace_elements {
            Vec::new()
        } else {
            self.elements.clone()
        };
        if let Some(template) = &self.template {
            elements.extend(template.elements().iter().cloned());
        }
        elements
    }

    /// 最终元素列表：模板展开 + 变量替换。
    fn build_elements(&self) -> Result<Vec<Element>> {
        let mut elements = self.assemble();
        if !self.vars.is_empty() {
            for element in &mut elements {
                element.resolve_vars(&self.vars)?;
            }
        }
        Ok(elements)
    }

    /// 背景画布：渐变 > 图片 > 纯色（同 PHP 的优先级）。
    fn background_canvas(&self, width: u32, height: u32) -> Result<ImageDriver> {
        if let Some((from, to, direction)) = &self.gradient {
            return gradient_canvas(width, height, from, to, *direction);
        }
        if let Some(path) = &self.bg_image {
            return cover(path, width, height);
        }
        ImageDriver::filled(width, height, self.bg_color.as_deref().unwrap_or("#FFFFFF"))
    }
}

/// PHP 的 `preg_match('/^#?[0-9a-fA-F]{3,8}$/')`。
fn is_hex_color(value: &str) -> bool {
    let hex = value.strip_prefix('#').unwrap_or(value);
    (3..=8).contains(&hex.len()) && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

/// 渐变背景：8px 一档色带近似线性渐变（同 PHP，比逐像素快得多）。
fn gradient_canvas(
    width: u32,
    height: u32,
    from: &str,
    to: &str,
    direction: Direction,
) -> Result<ImageDriver> {
    let from = color::parse(from)?;
    let to = color::parse(to)?;
    let steps = match direction {
        Direction::Vertical => height,
        Direction::Horizontal => width,
    };
    let band = 8u32;
    let mut canvas = ImageDriver::create(width, height)?;
    let mut offset = 0;
    while offset < steps {
        // 取色带中点比例（同 PHP），避免首末档偏色
        let ratio = (offset as f32 + band as f32 / 2.0) / steps.saturating_sub(1).max(1) as f32;
        let length = band.min(steps - offset);
        let options = ShapeOptions {
            color: mix(from, to, ratio),
            filled: true,
            ..Default::default()
        };
        match direction {
            Direction::Vertical => canvas.rectangle(0, offset as i32, width, length, &options)?,
            Direction::Horizontal => canvas.rectangle(offset as i32, 0, length, height, &options)?,
        }
        offset += band;
    }
    Ok(canvas)
}

/// 按比例混色 → `#RRGGBB`（`as u8` 截断，同 PHP 的 `intval()`）。
fn mix(from: Rgba<u8>, to: Rgba<u8>, ratio: f32) -> String {
    let channel = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * ratio) as u8;
    format!(
        "#{:02X}{:02X}{:02X}",
        channel(from.0[0], to.0[0]),
        channel(from.0[1], to.0[1]),
        channel(from.0[2], to.0[2]),
    )
}

/// 背景图铺满画布：等比放大到覆盖后再居中裁剪。
///
/// 与 PHP 的差异：PHP 直接拉伸变形，这里用 cover 保持比例（见交付说明）。
fn cover(path: &Path, width: u32, height: u32) -> Result<ImageDriver> {
    let mut background = ImageDriver::load(path)?;
    let (source_width, source_height) = background.size();
    let scale = (width as f32 / source_width.max(1) as f32)
        .max(height as f32 / source_height.max(1) as f32);
    let scaled_width = ((source_width as f32 * scale).ceil() as u32).max(width);
    let scaled_height = ((source_height as f32 * scale).ceil() as u32).max(height);
    background.resize(scaled_width, scaled_height)?;
    background.crop(
        ((scaled_width - width) / 2) as i32,
        ((scaled_height - height) / 2) as i32,
        width,
        height,
    )?;
    Ok(background)
}
