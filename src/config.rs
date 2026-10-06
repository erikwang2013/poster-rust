//! 配置：默认值 + 全局覆盖，键名与 PHP 版 `config/poster.php` 一一对应。
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

/// 背景图来源，对应 PHP `captcha.background_dir` 的三级优先级。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundSource {
    /// 使用随包分发的 6 张内置背景（PHP 默认指向 `assets/backgrounds/`）。
    #[default]
    Embedded,
    /// 使用自定义目录（放 png/jpg/gif/webp，随机选用）。
    Dir(PathBuf),
    /// 程序化生成（PHP 里 `background_dir = null` 的语义）。
    Procedural,
}

/// 缺图占位，对应 PHP `poster.placeholder`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Placeholder {
    /// 画项目宠物 Posty（PHP 里把 placeholder 指向 `assets/pet.png` 的语义）。
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

/// 验证误差容忍（像素/角度），对应 PHP `captcha.tolerance`。
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

/// 会话/账号级窗口限流，对应 PHP `captcha.rate_limit`。
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

/// 行为轨迹校验，对应 PHP `captcha.trajectory`（默认关闭）。
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

/// 默认点击文字池，与 PHP `captcha.click_words` 逐字一致。
pub const DEFAULT_CLICK_WORDS: [&str; 28] = [
    "合", "家", "欢", "乐", "良", "辰", "美", "景", "千", "变", "万", "化", "心", "有", "灵",
    "犀", "五", "湖", "四", "海", "山", "川", "美", "景", "花", "好", "月", "圆",
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

/// 根配置，键名与 PHP `config/poster.php` 对齐。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PosterConfig {
    pub image: ImageOptions,
    pub captcha: CaptchaOptions,
    pub poster: PosterOptions,
}

static GLOBAL: OnceLock<PosterConfig> = OnceLock::new();

/// 设置进程级全局配置（只能设置一次）。
pub fn set_global(config: PosterConfig) -> Result<()> {
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
    fn defaults_match_php_config() {
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
    fn resolve_font_prefers_explicit_then_config() {
        let mut c = PosterConfig::default();
        c.image.font = Some(PathBuf::from("/tmp/a.ttf"));
        assert_eq!(resolve_font(&c, None), PathBuf::from("/tmp/a.ttf"));
        assert_eq!(resolve_font(&c, Some(std::path::Path::new("/tmp/b.ttf"))), PathBuf::from("/tmp/b.ttf"));

        let bare = PosterConfig::default();
        assert_eq!(resolve_font(&bare, None), crate::assets::default_font_path());
    }
}
