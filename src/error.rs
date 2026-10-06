//! 统一错误类型。
//!
//! 对应 PHP 版的异常抛出点：驱动操作、配置、模板、验证码、存储。

/// poster-rust 统一错误类型。
#[derive(Debug, thiserror::Error)]
pub enum PosterError {
    /// 图像解码 / 编码 / 尺寸错误。
    #[error("图像错误: {0}")]
    Image(#[from] image::ImageError),

    /// 文件读写错误。
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    /// 模板 / 配置 JSON 错误。
    #[error("JSON 错误: {0}")]
    Json(#[from] serde_json::Error),

    /// 字体加载或渲染错误。
    #[error("字体错误: {0}")]
    Font(String),

    /// 颜色解析错误（如 `#GGGGGG`）。
    #[error("颜色错误: {0}")]
    Color(String),

    /// 验证码错误：类型未知、画布过小、key 不存在等。
    #[error("验证码错误: {0}")]
    Captcha(String),

    /// 配置错误。
    #[error("配置错误: {0}")]
    Config(String),

    /// 模板错误：未知元素类型、变量缺失等。
    #[error("模板错误: {0}")]
    Template(String),

    /// 存储后端错误（文件损坏、Redis 不可达等）。
    #[error("存储错误: {0}")]
    Storage(String),

    /// Redis 存储错误（`redis` feature）。
    #[cfg(feature = "redis")]
    #[error("Redis 错误: {0}")]
    Redis(#[from] redis::RedisError),

    /// 其他错误。
    #[error("{0}")]
    Other(String),
}

/// 便捷 `Result` 别名。
pub type Result<T> = std::result::Result<T, PosterError>;
