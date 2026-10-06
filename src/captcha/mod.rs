//! 验证码：点击 / 旋转 / 滑块 + 随机切换，对应 PHP 版 `src/Captcha/*`。
//!
//! 口径与 PHP 逐项对齐：容差 click ±18px / rotate ±5° / slider ±4px、TTL 300s、
//! 同 key 最多 3 次（超出即删）、校验成功即删（一次性）、跨 key 窗口限流 30 次/60s、
//! 轨迹校验默认关闭（min_points 4 / 300–5000ms / max_linearity 0.99）。
//!
//! 与 PHP 的差异（有意为之，逐条见各文件文档注释）：
//! - 图片 PNG 字节**一并持久化**（`{key}:img`），供框架层的 `GET {path}/{key}` 端点直出，
//!   不必把 base64 塞进载荷再解回来；成功校验时与答案一起删除；
//! - `set_background()` 指向的文件不存在时直接报错，不再像 PHP 那样静默回落到配置背景；
//! - 「计数不可用」在 Rust 侧以 `Option` 表达并由调用方失败关闭，语义与 PHP 的返回值 0 相同。
//!
//! 类型从存储的载荷里取，`verify()` 不需要再传 type：`CaptchaType` 与实际载荷不符时
//! 返回 `Ok(false)`（不泄露「类型不匹配」与「答案错误」的区别）；载荷损坏同样返回
//! `Ok(false)`（PHP 侧损坏即视为不存在）。存储故障则以 `Err` 上抛。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use rand::Rng;
use rand::seq::IndexedRandom;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::assets;
use crate::config::{BackgroundSource, PosterConfig};
use crate::drivers::{ImageDriver, LineOptions, ShapeOptions};
use crate::error::{PosterError, Result};
use crate::storage::Storage;

mod click;
mod factory;
mod manager;
mod random;
mod rate_limiter;
mod rotate;
mod slider;
mod trajectory;

pub(crate) use factory::CaptchaFactory;
pub use manager::CaptchaManager;
pub use trajectory::Trajectory;

/// 验证码类型。`Random` 只是**请求**语义，生成时会解析成三种之一（同 PHP 工厂）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptchaType {
    /// 点击文字 / 图标。
    Click,
    /// 旋转回正。
    Rotate,
    /// 拖动滑块。
    Slider,
    /// 随机三选一。
    Random,
}

impl CaptchaType {
    /// 解析类型字符串。
    pub fn parse(text: &str) -> Result<Self> {
        match text {
            "click" => Ok(Self::Click),
            "rotate" => Ok(Self::Rotate),
            "slider" => Ok(Self::Slider),
            "random" => Ok(Self::Random),
            other => Err(PosterError::Captcha(format!(
                "未知验证码类型: {other}（支持 click, rotate, slider, random）"
            ))),
        }
    }

    /// 类型字符串（载荷 `type` 字段与 [`CaptchaResult::captcha_type`] 用）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Click => "click",
            Self::Rotate => "rotate",
            Self::Slider => "slider",
            Self::Random => "random",
        }
    }

    /// 把 `Random` 解析成具体类型（非 Random 原样返回）。
    pub(crate) fn resolve(self) -> Self {
        match self {
            Self::Random => random::pick(),
            other => other,
        }
    }
}

impl std::str::FromStr for CaptchaType {
    type Err = PosterError;

    fn from_str(s: &str) -> Result<Self> {
        Self::parse(s)
    }
}

/// 难度：`easy` 2 个点击目标 / `hard` 4 个。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Difficulty {
    /// 简单。
    Easy,
    /// 中等（默认）。
    #[default]
    Medium,
    /// 困难。
    Hard,
}

impl Difficulty {
    /// 解析难度字符串。
    pub fn parse(text: &str) -> Result<Self> {
        match text {
            "easy" => Ok(Self::Easy),
            "medium" => Ok(Self::Medium),
            "hard" => Ok(Self::Hard),
            other => Err(PosterError::Captcha(format!(
                "未知难度: {other}（支持 easy, medium, hard）"
            ))),
        }
    }

    /// 难度字符串。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Easy => "easy",
            Self::Medium => "medium",
            Self::Hard => "hard",
        }
    }
}

/// 滑块拼图形状。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SliderShape {
    /// 矩形缺口（默认）。
    #[default]
    Square,
    /// 凹凸拼图：四边各自随机半圆凸 / 凹，缺口与拼图块共用同一轮廓。
    Jigsaw,
}

impl SliderShape {
    /// 解析形状字符串。
    pub fn parse(text: &str) -> Result<Self> {
        match text {
            "square" => Ok(Self::Square),
            "jigsaw" => Ok(Self::Jigsaw),
            other => Err(PosterError::Captcha(format!(
                "未知滑块形状: {other}（支持 square, jigsaw）"
            ))),
        }
    }

    /// 形状字符串。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Square => "square",
            Self::Jigsaw => "jigsaw",
        }
    }
}

/// 点击验证码的目标形态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TargetType {
    /// 文字（默认）。
    #[default]
    Text,
    /// 程序化矢量图标。
    Icon,
}

impl TargetType {
    /// 解析目标形态字符串。
    pub fn parse(text: &str) -> Result<Self> {
        match text {
            "text" => Ok(Self::Text),
            "icon" => Ok(Self::Icon),
            other => Err(PosterError::Captcha(format!(
                "未知点击目标类型: {other}（支持 text, icon）"
            ))),
        }
    }

    /// 目标形态字符串。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Icon => "icon",
        }
    }
}

/// 用户提交的答案。
///
/// serde 表示即各框架适配器的 `{"key": …, "answer": …}` 请求体里的 `answer`
/// （默认外部标签）：`{"Slider": 173.0}`、`{"Click": [[120.0, 80.0]]}`、
/// `{"SliderWithTrail": {"x": 173.0, "trail": [[12.0, 3.0, 0.0]], "duration_ms": 1200}}`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Answer {
    /// 点击坐标，顺序须与提示一致。
    Click(Vec<(f32, f32)>),
    /// 旋转角度（度）。
    Rotate(f32),
    /// 滑块 x（拼图块 PNG 左上角）。
    Slider(f32),
    /// 带轨迹的滑块。
    SliderWithTrail {
        /// 滑块 x。
        x: f32,
        /// 轨迹采样点 `(x, y, t_ms)`。
        trail: Vec<(f64, f64, f64)>,
        /// 总耗时（毫秒）。
        duration_ms: u64,
    },
    /// 带轨迹的旋转。
    RotateWithTrail {
        /// 旋转角度（度）。
        angle: f32,
        /// 轨迹采样点 `(x, y, t_ms)`。
        trail: Vec<(f64, f64, f64)>,
        /// 总耗时（毫秒）。
        duration_ms: u64,
    },
}

impl Answer {
    /// 轨迹（旧式的裸数值答案为 `None`）。
    pub(crate) fn trajectory(&self) -> Option<Trajectory> {
        match self {
            Self::SliderWithTrail {
                trail, duration_ms, ..
            }
            | Self::RotateWithTrail {
                trail, duration_ms, ..
            } => Some(Trajectory::new(trail.clone(), *duration_ms as f64)),
            _ => None,
        }
    }
}

/// 生成结果，可直接序列化成前端 JSON。
///
/// 字段名与 PHP 载荷一致：`key` / `type` / `image` / `extra`（`url` 缺省不出现）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptchaResult {
    /// 随机 key（16 字节随机数的 hex），不可预测。
    pub key: String,
    /// 图片 data URI（`data:image/png;base64,…`）。
    pub image: String,
    /// 实际类型：`click` | `rotate` | `slider`（`random` 会被解析成具体类型）。
    #[serde(rename = "type")]
    pub captcha_type: String,
    /// 各类型专属字段，键名与 PHP 一致。
    pub extra: Value,
    /// 图片端点 URL：路由模式下由框架层填（PHP `captcha.route`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// 共享上下文：配置 + 存储。`Arc` 便于管理器与各 Builder 共享。
pub(crate) struct Ctx {
    pub config: Arc<PosterConfig>,
    pub storage: Arc<dyn Storage>,
}

/// 「抽象验证码」：三种实现共用的状态与背景生成（对应 PHP `AbstractCaptcha`）。
pub(crate) struct Base {
    pub ctx: Arc<Ctx>,
    pub kind: CaptchaType,
    pub difficulty: Difficulty,
    /// 显式背景图（优先级最高）。
    pub background: Option<PathBuf>,
    /// 画布宽（载入背景图后会被图片尺寸覆盖，同 PHP）。
    pub width: u32,
    /// 画布高。
    pub height: u32,
    /// 本次生成的 key。
    pub key: String,
}

impl Base {
    /// 新建并生成 key（同 PHP `generateKey()`：16 字节随机数 → hex）。
    pub fn new(
        ctx: Arc<Ctx>,
        kind: CaptchaType,
        difficulty: Difficulty,
        background: Option<PathBuf>,
        width: u32,
        height: u32,
    ) -> Self {
        Self {
            ctx,
            kind,
            difficulty,
            background,
            width,
            height,
            key: generate_key(),
        }
    }

    /// 背景图，三级优先级（同 PHP `createBackground()`）：
    /// 显式背景图 > 配置背景目录（`BackgroundSource::Dir` / `Embedded`）> 程序化生成。
    ///
    /// 目录不存在或目录里没图时静默回落到程序化生成（PHP 同）；显式背景图加载失败则报错。
    pub fn create_background(&mut self) -> Result<ImageDriver> {
        if let Some(path) = self.background.clone() {
            let bg = ImageDriver::load(&path).map_err(|e| {
                PosterError::Captcha(format!("背景图加载失败 {}: {e}", path.display()))
            })?;
            let (width, height) = bg.size();
            self.width = width;
            self.height = height;
            return Ok(bg);
        }

        let (target_w, target_h) = (self.width, self.height);
        match &self.ctx.config.captcha.background_source {
            BackgroundSource::Dir(dir) => {
                if let Some(bg) = load_dir_background(dir, target_w, target_h)? {
                    return Ok(bg);
                }
            }
            BackgroundSource::Embedded => {
                if let Some(bytes) = assets::BACKGROUNDS.choose(&mut rand::rng()).map(|(_, b)| *b) {
                    let mut bg = ImageDriver::from_bytes(bytes)?;
                    bg.resize(target_w, target_h)?;
                    return Ok(bg);
                }
            }
            BackgroundSource::Procedural => {}
        }

        self.procedural_background()
    }

    /// 程序化背景：随机风格（`captcha.background_styles`）的渐变 + 装饰 + 噪点。
    fn procedural_background(&mut self) -> Result<ImageDriver> {
        let styles = self.ctx.config.captcha.background_styles.clone();
        let style = styles
            .choose(&mut rand::rng())
            .cloned()
            .unwrap_or_else(|| "minimal".to_string());
        if !matches!(style.as_str(), "minimal" | "vibrant" | "natural") {
            return Err(PosterError::Captcha(format!(
                "未知背景风格: {style}（支持 minimal, vibrant, natural）"
            )));
        }

        let mut bg = ImageDriver::create(self.width, self.height)?;
        self.generate_gradient(&mut bg, &style)?;
        self.generate_decorations(&mut bg, &style)?;
        self.generate_noise(&mut bg, &style);
        Ok(bg)
    }

    fn generate_gradient(&self, bg: &mut ImageDriver, style: &str) -> Result<()> {
        let palette = palettes_for_style(style)
            .choose(&mut rand::rng())
            .copied()
            .ok_or_else(|| PosterError::Captcha(format!("风格 {style} 没有配色")))?;

        let steps = 120u32;
        for i in 0..steps {
            let t = i as f32 / (steps - 1) as f32;
            let color = interpolate_color(palette.0, palette.1, t)?;
            let y = i * self.height / steps;
            let next_y = (i + 1) * self.height / steps;
            bg.rectangle(
                0,
                y as i32,
                self.width,
                next_y - y,
                &ShapeOptions {
                    color,
                    filled: true,
                    ..Default::default()
                },
            )?;
        }
        bg.blur(2);
        Ok(())
    }

    fn generate_decorations(&self, bg: &mut ImageDriver, style: &str) -> Result<()> {
        let mut rng = rand::rng();
        match style {
            "vibrant" => {
                for _ in 0..rng.random_range(10..=18) {
                    let (x, y) = (
                        rng.random_range(0..=self.width as i32),
                        rng.random_range(0..=self.height as i32),
                    );
                    let r = rng.random_range(10..=70);
                    bg.ellipse(
                        x,
                        y,
                        r as f32,
                        r as f32,
                        &ShapeOptions {
                            color: format!("{}2A", random_color()),
                            filled: true,
                            ..Default::default()
                        },
                    )?;
                }
                for _ in 0..rng.random_range(3..=6) {
                    let (x, y) = (
                        rng.random_range(0..=self.width as i32),
                        rng.random_range(0..=self.height as i32),
                    );
                    let r = rng.random_range(15..=45);
                    bg.ellipse(
                        x,
                        y,
                        r as f32,
                        r as f32,
                        &ShapeOptions {
                            color: format!("{}55", random_color()),
                            filled: false,
                            ..Default::default()
                        },
                    )?;
                }
            }
            "natural" => {
                for _ in 0..rng.random_range(6..=12) {
                    let x = rng.random_range(0..=self.width.saturating_sub(50)) as i32;
                    let y = rng.random_range(0..=self.height.saturating_sub(30)) as i32;
                    bg.rectangle(
                        x,
                        y,
                        rng.random_range(30..=90),
                        rng.random_range(15..=45),
                        &ShapeOptions {
                            color: format!("{}2E", random_light_color()),
                            filled: true,
                            ..Default::default()
                        },
                    )?;
                }
            }
            _ => {
                // minimal
                for _ in 0..rng.random_range(2..=3) {
                    let (x, y) = (
                        rng.random_range(-40..=self.width as i32 + 40),
                        rng.random_range(-40..=self.height as i32 + 40),
                    );
                    let r = rng.random_range(60..=140);
                    bg.ellipse(
                        x,
                        y,
                        r as f32,
                        r as f32,
                        &ShapeOptions {
                            color: "#FFFFFF66".into(),
                            filled: true,
                            ..Default::default()
                        },
                    )?;
                }
                for _ in 0..rng.random_range(1..=2) {
                    let (x1, y1) = (
                        rng.random_range(0..=self.width as i32),
                        rng.random_range(0..=self.height as i32),
                    );
                    let (x2, y2) = (
                        x1 + rng.random_range(-120..=120),
                        y1 + rng.random_range(-80..=80),
                    );
                    bg.line(
                        x1,
                        y1,
                        x2,
                        y2,
                        &LineOptions {
                            color: "#FFFFFF55".into(),
                            width: rng.random_range(2..=4),
                        },
                    )?;
                }
            }
        }
        Ok(())
    }

    fn generate_noise(&self, bg: &mut ImageDriver, style: &str) {
        let mut rng = rand::rng();
        let (count, dot) = match style {
            "vibrant" => (rng.random_range(50..=90), rng.random_range(1..=2)),
            "natural" => (rng.random_range(100..=180), 1),
            _ => (rng.random_range(20..=40), 1),
        };
        for _ in 0..count {
            let x = rng.random_range(0..=self.width.saturating_sub(1).max(1)) as i32;
            let y = rng.random_range(0..=self.height.saturating_sub(1).max(1)) as i32;
            // 噪点是纯装饰，画布边缘裁掉即可，失败不该拖垮整张验证码
            let _ = bg.ellipse(
                x,
                y,
                dot as f32,
                dot as f32,
                &ShapeOptions {
                    color: format!("{}1E", random_color()),
                    filled: true,
                    ..Default::default()
                },
            );
        }
    }

    /// 存答案 + 图片，并组装返回值。
    pub fn finish(&self, png: &[u8], extra: Value, answer: Value) -> Result<CaptchaResult> {
        let mut payload = match answer {
            Value::Object(map) => map,
            other => {
                return Err(PosterError::Captcha(format!(
                    "答案载荷必须是 JSON 对象，得到 {other}"
                )));
            }
        };
        payload.insert("type".into(), json!(self.kind.as_str()));
        payload.insert("attempts".into(), json!(0));
        payload.insert("created_at".into(), json!(chrono::Utc::now().timestamp()));

        let ttl = Duration::from_secs(self.ctx.config.captcha.ttl_secs);
        self.ctx
            .storage
            .set(&self.key, Value::Object(payload).to_string().as_bytes(), ttl)?;
        self.ctx
            .storage
            .set(&image_key(&self.key), png, ttl)?;

        Ok(CaptchaResult {
            key: self.key.clone(),
            image: png_data_uri(png),
            captcha_type: self.kind.as_str().to_string(),
            extra,
            url: None,
        })
    }
}

/// 生成 key：16 字节随机数的 hex（同 PHP `bin2hex(random_bytes(16))`）。
pub(crate) fn generate_key() -> String {
    let bytes: [u8; 16] = rand::rng().random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 图片在存储里的键（与答案同 TTL、同生死）。
pub(crate) fn image_key(key: &str) -> String {
    format!("{key}:img")
}

/// PNG 字节 → data URI。
pub(crate) fn png_data_uri(png: &[u8]) -> String {
    use base64::Engine;
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    )
}

/// 随机色 `#RRGGBB`（各通道 0-200，同 PHP `randomColor()`）。
pub(crate) fn random_color() -> String {
    let mut rng = rand::rng();
    format!(
        "#{:02X}{:02X}{:02X}",
        rng.random_range(0..=200u8),
        rng.random_range(0..=200u8),
        rng.random_range(0..=200u8)
    )
}

/// 随机浅色 `#RRGGBB`（各通道 200-255，同 PHP `randomLightColor()`）。
pub(crate) fn random_light_color() -> String {
    let mut rng = rand::rng();
    format!(
        "#{:02X}{:02X}{:02X}",
        rng.random_range(200..=255u8),
        rng.random_range(200..=255u8),
        rng.random_range(200..=255u8)
    )
}

/// 目录里随机取一张图，缩放到目标尺寸；目录不存在或没有图片时返回 `None`（回落程序化背景）。
fn load_dir_background(dir: &Path, width: u32, height: u32) -> Result<Option<ImageDriver>> {
    if !dir.is_dir() {
        return Ok(None);
    }
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let is_image = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| {
                matches!(
                    e.to_ascii_lowercase().as_str(),
                    "jpg" | "jpeg" | "png" | "gif" | "webp"
                )
            })
            .unwrap_or(false);
        if is_image && path.is_file() {
            files.push(path);
        }
    }
    let Some(path) = files.choose(&mut rand::rng()) else {
        return Ok(None);
    };
    let mut bg = ImageDriver::load(path)
        .map_err(|e| PosterError::Captcha(format!("背景图加载失败 {}: {e}", path.display())))?;
    bg.resize(width, height)?;
    Ok(Some(bg))
}

/// 风格配色，同 PHP `palettesForStyle()`。
fn palettes_for_style(style: &str) -> &'static [(&'static str, &'static str)] {
    match style {
        "vibrant" => &[
            ("#667eea", "#764ba2"),
            ("#f093fb", "#f5576c"),
            ("#4facfe", "#00f2fe"),
            ("#43e97b", "#38f9d7"),
            ("#fa709a", "#fee140"),
            ("#a18cd1", "#fbc2eb"),
        ],
        "natural" => &[
            ("#f5f0e8", "#e8dcc8"),
            ("#faf0e6", "#f5deb3"),
            ("#f0ebe3", "#d9cdb3"),
            ("#fef9ef", "#f5e6c8"),
            ("#f7f2e9", "#e6d5c3"),
        ],
        _ => &[
            ("#e8eaf6", "#c5cae9"),
            ("#e0f2f1", "#b2dfdb"),
            ("#f3e5f5", "#e1bee7"),
            ("#eceff1", "#cfd8dc"),
            ("#e8f5e9", "#c8e6c9"),
        ],
    }
}

/// 两色线性插值（`t` 0-1），同 PHP `interpolateColor()`。
fn interpolate_color(from: &str, to: &str, t: f32) -> Result<String> {
    let a = crate::drivers::color::parse(from)?;
    let b = crate::drivers::color::parse(to)?;
    let mix = |i: usize| ((a.0[i] as f32) + (b.0[i] as f32 - a.0[i] as f32) * t) as u8;
    Ok(format!("#{:02X}{:02X}{:02X}", mix(0), mix(1), mix(2)))
}

/// 单元测试用的上下文（内存存储 + 默认配置）。
#[cfg(test)]
pub(crate) fn test_ctx() -> Arc<Ctx> {
    test_ctx_with(Arc::new(crate::storage::MemoryStorage::new()))
}

/// 单元测试用的上下文，指定存储后端（供断言直接读答案载荷）。
#[cfg(test)]
pub(crate) fn test_ctx_with(storage: Arc<dyn Storage>) -> Arc<Ctx> {
    Arc::new(Ctx {
        config: Arc::new(PosterConfig::default()),
        storage,
    })
}

/// 验证码 Builder：链式设置 → `generate()`。
///
/// 只作用于对应类型的 setter 在其它类型上会被忽略（`set_words` 只对 click 有意义等）。
pub struct CaptchaBuilder {
    ctx: Arc<Ctx>,
    kind: CaptchaType,
    difficulty: Difficulty,
    background: Option<PathBuf>,
    width: u32,
    height: u32,
    words: Option<Vec<String>>,
    target_type: TargetType,
    size: u32,
    angle_range: (f32, f32),
    shape: Option<SliderShape>,
    /// setter 参数的校验错误。链式 setter 一律返回 `Self`（同 PHP 的可链式配置），
    /// 非法取值在这里挂起，由 `generate()` 一并报出——边界仍然卡得住，链不断。
    pending_error: Option<PosterError>,
}

impl CaptchaBuilder {
    /// 由工厂创建；类型必须已是具体类型（`Random` 在 `CaptchaManager::create` 里解析）。
    pub(crate) fn new(ctx: Arc<Ctx>, kind: CaptchaType, difficulty: Difficulty) -> Self {
        Self {
            ctx,
            kind,
            difficulty,
            background: None,
            width: 300,
            height: 200,
            words: None,
            target_type: TargetType::Text,
            size: 200,
            angle_range: (30.0, 330.0),
            shape: None,
            pending_error: None,
        }
    }

    /// 难度：`easy` | `medium` | `hard`。取值非法时由 `generate()` 报错。
    pub fn set_difficulty(mut self, difficulty: &str) -> Self {
        match Difficulty::parse(difficulty) {
            Ok(value) => self.difficulty = value,
            Err(error) => self.pending_error = Some(error),
        }
        self
    }

    /// 指定背景图（单图优先于配置的背景来源）。
    pub fn set_background(mut self, path: impl Into<PathBuf>) -> Self {
        self.background = Some(path.into());
        self
    }

    /// 点击验证码的文字池（不足时循环取用）。
    pub fn set_words(mut self, words: Vec<String>) -> Self {
        self.words = Some(words);
        self
    }

    /// 旋转验证码的圆图直径（钳制到 60-400，同 PHP）。
    pub fn set_size(mut self, size: u32) -> Self {
        self.size = size.clamp(60, 400);
        self
    }

    /// 旋转验证码的角度范围（钳制到 1-359，同 PHP）。
    pub fn set_angle_range(mut self, min: f32, max: f32) -> Self {
        self.angle_range = (min.max(1.0), max.min(359.0));
        self
    }

    /// 滑块形状：`square` | `jigsaw`。取值非法时由 `generate()` 报错。
    pub fn set_shape(mut self, shape: &str) -> Self {
        match SliderShape::parse(shape) {
            Ok(value) => self.shape = Some(value),
            Err(error) => self.pending_error = Some(error),
        }
        self
    }

    /// 点击目标形态：`text` | `icon`。取值非法时由 `generate()` 报错。
    pub fn set_target_type(mut self, target_type: &str) -> Self {
        match TargetType::parse(target_type) {
            Ok(value) => self.target_type = value,
            Err(error) => self.pending_error = Some(error),
        }
        self
    }

    /// 生成验证码：画图 → 存答案与 PNG → 返回可序列化结果。
    ///
    /// 前面 setter 传了非法取值的话，错误在这里报出。
    pub fn generate(mut self) -> Result<CaptchaResult> {
        if let Some(error) = self.pending_error.take() {
            return Err(error);
        }
        match self.kind {
            CaptchaType::Click => click::generate(self),
            CaptchaType::Rotate => rotate::generate(self),
            CaptchaType::Slider => slider::generate(self),
            CaptchaType::Random => Err(PosterError::Captcha(
                "Random 应在创建时解析成具体类型".into(),
            )),
        }
    }

    // ── 生成器用到的访问器 ────────────────────────────────

    pub(crate) fn ctx(&self) -> &Arc<Ctx> {
        &self.ctx
    }

    pub(crate) fn difficulty(&self) -> Difficulty {
        self.difficulty
    }

    pub(crate) fn background(&self) -> Option<PathBuf> {
        self.background.clone()
    }

    pub(crate) fn canvas_size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub(crate) fn words(&self) -> Option<Vec<String>> {
        self.words.clone()
    }

    pub(crate) fn target_type(&self) -> TargetType {
        self.target_type
    }

    pub(crate) fn rotate_size(&self) -> u32 {
        self.size
    }

    pub(crate) fn angle_range(&self) -> (f32, f32) {
        self.angle_range
    }

    pub(crate) fn shape(&self) -> Option<SliderShape> {
        self.shape
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_parse_and_round_trip() {
        for kind in [CaptchaType::Click, CaptchaType::Rotate, CaptchaType::Slider] {
            assert_eq!(CaptchaType::parse(kind.as_str()).unwrap(), kind);
        }
        assert_eq!(CaptchaType::parse("random").unwrap(), CaptchaType::Random);
        assert!(CaptchaType::parse("slide").is_err());

        assert_eq!(Difficulty::parse("hard").unwrap().as_str(), "hard");
        assert!(Difficulty::parse("").is_err());
        assert_eq!(SliderShape::parse("jigsaw").unwrap(), SliderShape::Jigsaw);
        assert!(SliderShape::parse("triangle").is_err());
        assert_eq!(TargetType::parse("icon").unwrap(), TargetType::Icon);
        assert!(TargetType::parse("emoji").is_err());
    }

    #[test]
    fn random_resolves_to_a_concrete_type() {
        for _ in 0..50 {
            let resolved = CaptchaType::Random.resolve();
            assert!(matches!(
                resolved,
                CaptchaType::Click | CaptchaType::Rotate | CaptchaType::Slider
            ));
        }
    }

    #[test]
    fn keys_are_hex_and_unique() {
        let a = generate_key();
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, generate_key());
    }

    #[test]
    fn color_interpolation_hits_both_ends() {
        assert_eq!(interpolate_color("#000000", "#FFFFFF", 0.0).unwrap(), "#000000");
        assert_eq!(interpolate_color("#000000", "#FFFFFF", 1.0).unwrap(), "#FFFFFF");
        assert_eq!(interpolate_color("#000000", "#FF0000", 0.5).unwrap(), "#7F0000");
    }

    #[test]
    fn random_colors_are_valid_hex() {
        for _ in 0..20 {
            assert!(crate::drivers::color::parse(&random_color()).is_ok());
            assert!(crate::drivers::color::parse(&random_light_color()).is_ok());
        }
        assert!(crate::drivers::color::parse(&format!("{}1E", random_color())).is_ok());
    }

    #[test]
    fn result_serializes_php_compatible_keys() {
        let result = CaptchaResult {
            key: "abc".into(),
            image: "data:image/png;base64,AA==".into(),
            captcha_type: "click".into(),
            extra: json!({"texts": [{"order": 1, "text": "云"}]}),
            url: None,
        };
        let value: Value = serde_json::to_value(&result).unwrap();
        assert_eq!(value["key"], "abc");
        assert_eq!(value["type"], "click", "PHP 载荷用 type 而非 captcha_type");
        assert!(value.get("url").is_none(), "url 为空时不应出现");
        assert_eq!(value["extra"]["texts"][0]["order"], 1);

        let with_url = CaptchaResult {
            url: Some("/captcha/abc".into()),
            ..result
        };
        let value = serde_json::to_value(&with_url).unwrap();
        assert_eq!(value["url"], "/captcha/abc");
    }
}
