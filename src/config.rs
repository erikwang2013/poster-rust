//! 配置：默认值 + 全局覆盖，分为 `image` / `captcha` / `poster` 三段。
//!
//! 用法：
//! ```no_run
//! use poster::{PosterConfig, config};
//!
//! let mut cfg = PosterConfig::default();
//! cfg.captcha.ttl_secs = 600;
//! config::set_global(cfg).unwrap();   // 进程内一次；之后 config::global() 生效
//! ```

use std::path::PathBuf;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::error::{PosterError, Result};

/// 背景图来源，三种取值：内置背景 / 自定义目录 / 程序化生成。
///
/// `#[non_exhaustive]`：新增来源（如网络图源）不算破坏性变更；下游穷举 `match` 需保留通配臂。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BackgroundSource {
    /// 使用随包分发的 6 张内置背景（源文件在 `assets/backgrounds/`）。
    #[default]
    Embedded,
    /// 使用自定义目录（放 png/jpg/gif/webp，随机选用）。
    Dir(PathBuf),
    /// 程序化生成（不读任何图片文件）。
    Procedural,
}

/// 缺图占位。
///
/// `#[non_exhaustive]`：新增占位不算破坏性变更；下游穷举 `match` 需保留通配臂。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Placeholder {
    /// 画项目宠物 Posty（内嵌的 `assets/pet.png`）。
    Pet,
    /// 画指定图片。
    Path(PathBuf),
}

/// `image` 段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImageOptions {
    /// `save()` / `output()` 未显式指定质量时的默认 JPEG 质量 0-100。
    pub quality: u8,
    /// 默认字体路径；`None` = 随包分发的阿里巴巴普惠体。
    pub font: Option<PathBuf>,
}

impl Default for ImageOptions {
    fn default() -> Self {
        Self {
            quality: 90,
            font: None,
        }
    }
}

/// 验证误差容忍（像素/角度）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToleranceOptions {
    /// 点击验证：像素半径。
    pub click: f32,
    /// 旋转验证：角度。
    pub rotate: f32,
    /// 滑块验证：像素。
    pub slider: f32,
}

impl Default for ToleranceOptions {
    fn default() -> Self {
        Self {
            click: 18.0,
            rotate: 5.0,
            slider: 4.0,
        }
    }
}

/// 会话/账号级窗口限流。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RateLimitOptions {
    /// 每个窗口允许的校验次数。
    pub max: u32,
    /// 窗口秒数。
    pub window_secs: u64,
}

impl Default for RateLimitOptions {
    fn default() -> Self {
        Self {
            max: 30,
            window_secs: 60,
        }
    }
}

/// 行为轨迹校验（默认关闭）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrajectoryOptions {
    pub enabled: bool,
    /// 最少采样点。
    pub min_points: usize,
    /// 最短耗时（毫秒）。
    pub min_duration_ms: u64,
    /// 最长耗时（毫秒）。
    pub max_duration_ms: u64,
    /// 线性度高于此值判为机器。
    pub max_linearity: f32,
}

impl Default for TrajectoryOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            min_points: 4,
            min_duration_ms: 300,
            max_duration_ms: 5000,
            max_linearity: 0.99,
        }
    }
}

/// `captcha` 段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptchaOptions {
    /// 默认验证码类型：`click` | `rotate` | `slider` | `random`。
    pub default_type: String,
    /// 默认难度：`easy` | `medium` | `hard`。
    pub default_difficulty: String,
    /// 滑块拼图形状：`square` | `jigsaw`。
    pub slider_shape: String,
    /// 点击验证码文字池（请只用「单一字形逐字绘制」的文字）。
    pub click_words: Vec<String>,
    /// 背景图来源。
    pub background_source: BackgroundSource,
    /// 程序化背景风格：`minimal` | `vibrant` | `natural`。
    pub background_styles: Vec<String>,
    /// 验证码有效期（秒）。
    pub ttl_secs: u64,
    /// 同一 key 最多验证次数。
    pub max_attempts: u32,
    /// 各类型容差。
    pub tolerance: ToleranceOptions,
    /// 窗口限流。
    pub rate_limit: RateLimitOptions,
    /// 轨迹校验。
    pub trajectory: TrajectoryOptions,
    /// Redis 键前缀（`redis` feature 的存储后端）。
    pub redis_prefix: String,
    /// 文件存储目录；`None` = 系统临时目录。
    pub file_path: Option<PathBuf>,
}

impl Default for CaptchaOptions {
    fn default() -> Self {
        Self {
            default_type: "random".into(),
            default_difficulty: "medium".into(),
            slider_shape: "square".into(),
            click_words: DEFAULT_CLICK_WORDS.iter().map(|s| (*s).into()).collect(),
            background_source: BackgroundSource::Embedded,
            background_styles: vec!["minimal".into(), "vibrant".into(), "natural".into()],
            ttl_secs: 300,
            max_attempts: 3,
            tolerance: ToleranceOptions::default(),
            rate_limit: RateLimitOptions::default(),
            trajectory: TrajectoryOptions::default(),
            redis_prefix: "poster:captcha:".into(),
            file_path: None,
        }
    }
}

/// 默认点击文字池（28 个单字）。
pub const DEFAULT_CLICK_WORDS: [&str; 28] = [
    "合", "家", "欢", "乐", "良", "辰", "美", "景", "千", "变", "万", "化", "心", "有", "灵", "犀",
    "五", "湖", "四", "海", "山", "川", "美", "景", "花", "好", "月", "圆",
];

/// `poster` 段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PosterOptions {
    /// 画布默认宽（px）。
    pub default_width: u32,
    /// 画布默认高（px）。
    pub default_height: u32,
    /// 默认字体路径；`None` = 随包分发的阿里巴巴普惠体。
    pub font: Option<PathBuf>,
    /// `save()` 未显式指定质量时的默认 JPEG 质量 0-100。
    pub jpeg_quality: u8,
    /// PNG 压缩级别 0-9。
    pub png_compression: u8,
    /// 缺失图片的占位图；`None` = 跳过不绘制。
    pub placeholder: Option<Placeholder>,
}

impl Default for PosterOptions {
    fn default() -> Self {
        Self {
            default_width: 750,
            default_height: 1334,
            font: None,
            jpeg_quality: 90,
            png_compression: 6,
            placeholder: None,
        }
    }
}

/// 根配置：`image` / `captcha` / `poster` 三段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PosterConfig {
    pub image: ImageOptions,
    pub captcha: CaptchaOptions,
    pub poster: PosterOptions,
}

static GLOBAL: OnceLock<PosterConfig> = OnceLock::new();

impl PosterConfig {
    /// 校验配置的合法区间，全部违规项返回 [`PosterError::Config`]（信息带具体字段名）。
    ///
    /// [`set_global`] 会先调用它；反序列化外部配置后也可单独自检。
    ///
    /// 注意 `captcha.rate_limit.max == 0` / `window_secs == 0` 是**文档化的「关闭限流」语义**，
    /// 这里不拒绝。
    pub fn validate(&self) -> Result<()> {
        let bad = |msg: String| -> Result<()> { Err(PosterError::Config(msg)) };

        if self.captcha.ttl_secs == 0 {
            return bad("captcha.ttl_secs 不能为 0：验证码会立即过期，永远无法通过校验".into());
        }
        if self.captcha.max_attempts == 0 {
            return bad("captcha.max_attempts 不能为 0：首次校验计数即超限，永远不会放行".into());
        }
        if self.captcha.redis_prefix.is_empty() {
            return bad("captcha.redis_prefix 不能为空：Redis 存储需要非空键前缀".into());
        }
        if self.image.quality > 100 {
            return bad(format!(
                "image.quality 必须在 0-100 之间，当前 {}",
                self.image.quality
            ));
        }
        if self.poster.jpeg_quality > 100 {
            return bad(format!(
                "poster.jpeg_quality 必须在 0-100 之间，当前 {}",
                self.poster.jpeg_quality
            ));
        }
        if self.poster.png_compression > 9 {
            return bad(format!(
                "poster.png_compression 必须在 0-9 之间，当前 {}",
                self.poster.png_compression
            ));
        }
        let (w, h) = (self.poster.default_width, self.poster.default_height);
        if w == 0 || h == 0 {
            return bad(format!(
                "poster.default_width / default_height 不能为 0，当前 {w}x{h}"
            ));
        }
        // 与 drivers::canvas 的像素预算保持一致（那里在构造画布时再兜一次）
        if u64::from(w) * u64::from(h) > MAX_CANVAS_PIXELS {
            return bad(format!(
                "poster 默认画布 {w}x{h} 超出像素预算 {MAX_CANVAS_PIXELS}"
            ));
        }
        Ok(())
    }
}

/// 画布像素预算，与 `drivers::canvas` 的 `MAX_PIXELS` 对齐。
const MAX_CANVAS_PIXELS: u64 = 40_000_000;

/// 设置进程级全局配置（只能设置一次）。配置非法时返回 [`PosterError::Config`]。
pub fn set_global(config: PosterConfig) -> Result<()> {
    config.validate()?;
    GLOBAL
        .set(config)
        .map_err(|_| PosterError::Config("全局配置已设置，不能重复设置".into()))
}

/// 进程级全局配置；未设置时为默认值。
pub fn global() -> &'static PosterConfig {
    GLOBAL.get_or_init(PosterConfig::default)
}

/// 默认字体路径：配置优先，否则用随包分发的阿里巴巴普惠体。
pub fn resolve_font(config: &PosterConfig, font: Option<&std::path::Path>) -> PathBuf {
    font.map(PathBuf::from)
        .or_else(|| config.image.font.clone())
        .or_else(|| config.poster.font.clone())
        .unwrap_or_else(crate::assets::default_font_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_documented_values() {
        let c = PosterConfig::default();
        assert_eq!(c.captcha.ttl_secs, 300);
        assert_eq!(c.captcha.max_attempts, 3);
        assert_eq!(c.captcha.tolerance.click, 18.0);
        assert_eq!(c.captcha.tolerance.rotate, 5.0);
        assert_eq!(c.captcha.tolerance.slider, 4.0);
        assert_eq!(c.captcha.rate_limit.max, 30);
        assert_eq!(c.captcha.rate_limit.window_secs, 60);
        assert!(!c.captcha.trajectory.enabled);
        assert_eq!(c.captcha.click_words.len(), 28);
        assert_eq!(c.poster.default_width, 750);
        assert_eq!(c.poster.default_height, 1334);
        assert_eq!(c.poster.jpeg_quality, 90);
        assert_eq!(c.poster.png_compression, 6);
        assert!(c.poster.placeholder.is_none());
        assert_eq!(c.captcha.background_source, BackgroundSource::Embedded);
    }

    #[test]
    fn config_round_trips_through_json() {
        let mut c = PosterConfig::default();
        c.captcha.background_source = BackgroundSource::Procedural;
        c.poster.placeholder = Some(Placeholder::Pet);
        let json = serde_json::to_string(&c).unwrap();
        let back: PosterConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn validate_accepts_defaults_and_documented_rate_limit_off() {
        assert!(PosterConfig::default().validate().is_ok());

        // `rate_limit` 全 0 是文档化的「关闭限流」，不是错误。
        let mut c = PosterConfig::default();
        c.captcha.rate_limit.max = 0;
        c.captcha.rate_limit.window_secs = 0;
        assert!(c.validate().is_ok());
    }

    /// 每个违规项都必须报 `PosterError::Config`，且信息里带上字段名。
    #[test]
    fn validate_rejects_each_out_of_range_field() {
        type Case = (&'static str, fn(&mut PosterConfig));
        let cases: [Case; 7] = [
            ("captcha.ttl_secs", |c| c.captcha.ttl_secs = 0),
            ("captcha.max_attempts", |c| c.captcha.max_attempts = 0),
            ("captcha.redis_prefix", |c| c.captcha.redis_prefix.clear()),
            ("image.quality", |c| c.image.quality = 101),
            ("poster.jpeg_quality", |c| c.poster.jpeg_quality = 101),
            ("poster.png_compression", |c| c.poster.png_compression = 10),
            ("poster.default_width", |c| c.poster.default_width = 0),
        ];
        for (field, break_it) in cases {
            let mut c = PosterConfig::default();
            break_it(&mut c);
            match c.validate() {
                Err(PosterError::Config(msg)) => {
                    assert!(
                        msg.contains(field),
                        "字段 {field} 的报错信息没带字段名: {msg}"
                    )
                }
                other => panic!("{field} 违规应报 Config 错误，实际 {other:?}"),
            }
        }

        // 默认高度为 0，以及宽高各自合法但乘积超出像素预算。
        let mut c = PosterConfig::default();
        c.poster.default_height = 0;
        assert!(matches!(c.validate(), Err(PosterError::Config(_))));

        let mut c = PosterConfig::default();
        c.poster.default_width = 20_000;
        c.poster.default_height = 20_000; // 4e8 > 4e7
        assert!(matches!(c.validate(), Err(PosterError::Config(_))));

        // 预算边界本身（恰好 4e7）放行。
        let mut c = PosterConfig::default();
        c.poster.default_width = 8_000;
        c.poster.default_height = 5_000; // == 4e7
        assert!(c.validate().is_ok());
    }

    #[test]
    fn resolve_font_prefers_explicit_then_config() {
        let mut c = PosterConfig::default();
        c.image.font = Some(PathBuf::from("/tmp/a.ttf"));
        assert_eq!(resolve_font(&c, None), PathBuf::from("/tmp/a.ttf"));
        assert_eq!(
            resolve_font(&c, Some(std::path::Path::new("/tmp/b.ttf"))),
            PathBuf::from("/tmp/b.ttf")
        );

        let bare = PosterConfig::default();
        assert_eq!(
            resolve_font(&bare, None),
            crate::assets::default_font_path()
        );
    }
}
