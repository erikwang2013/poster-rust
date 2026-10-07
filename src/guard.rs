//! 原生 Guard（请求守卫）：不依赖任何框架的通用入口。
//!
//! 八个框架适配器（axum / actix-web / rocket / poem / salvo / warp / bee-rust / e-cat）
//! 全部产出 `Guard`；未适配的框架也可以自己从应用状态里克隆一份。
//!
//! `Guard` 只是 `Arc<CaptchaManager>` 的一层薄包装：接线期构造（`from_manager` 快速失败），
//! 请求期克隆只是两次原子计数，`Send + Sync`，适合每请求克隆。

use std::net::IpAddr;
use std::sync::Arc;

use crate::captcha::{Answer, CaptchaManager, CaptchaResult};
use crate::error::Result;
use crate::poster::PosterBuilder;

/// `X-Forwarded-For` 派生的身份长度上限（限流计数键只取哈希，截断只是防御性的边界约束）。
const MAX_IDENTITY_LEN: usize = 64;

/// 由请求头与对端地址派生限流身份，供各框架适配器调用。
///
/// 优先级：
/// 1. `X-Forwarded-For` 第一段（`"1.2.3.4, 5.6.7.8"` → `"1.2.3.4"`），trim 后为空/全空白则忽略，
///    超长截断到 64 字符；
/// 2. 对端 IP（`peer`）；
/// 3. 都没有则 `"unknown"`（宁可所有人共用一个桶，也不放行无限量猜测）。
///
/// **代理场景注意**：`X-Forwarded-For` 是客户端可伪造的请求头。直接暴露公网时
/// 应传 `None`（或先在可信代理层剥离/覆写该头），否则攻击者每次换一个伪造 IP
/// 即可绕开限流；只有置于可信反向代理之后才应采信它。
pub fn client_identity(forwarded_for: Option<&str>, peer: Option<IpAddr>) -> String {
    let forwarded = forwarded_for
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|first| !first.is_empty());
    if let Some(first) = forwarded {
        return first.chars().take(MAX_IDENTITY_LEN).collect();
    }
    match peer {
        Some(ip) => ip.to_string(),
        None => "unknown".to_string(),
    }
}

/// 出图响应的通用描述（各框架适配器自行转成框架的 Response）。
#[derive(Debug, Clone)]
pub struct CaptchaImage {
    /// PNG 字节。
    pub bytes: Vec<u8>,
    /// 固定 `image/png`。
    pub content_type: &'static str,
    /// 固定 `no-store`（验证码图片不应被缓存）。
    pub cache_control: &'static str,
}

/// 请求守卫。
#[derive(Clone)]
pub struct Guard {
    manager: Arc<CaptchaManager>,
}

impl std::fmt::Debug for Guard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Guard").finish_non_exhaustive()
    }
}

impl Guard {
    /// 直接包装管理器（不做探针）。
    pub fn new(manager: Arc<CaptchaManager>) -> Self {
        Self { manager }
    }

    /// 接线期构造：对存储做一次写探针，后端不可用立即失败（而不是等第一个请求）。
    pub fn from_manager(manager: Arc<CaptchaManager>) -> Result<Self> {
        manager.probe()?;
        Ok(Self { manager })
    }

    /// 底层管理器。
    pub fn manager(&self) -> &Arc<CaptchaManager> {
        &self.manager
    }

    /// 生成一张验证码（`kind = None` 取配置默认类型）。
    pub fn create(&self, kind: Option<&str>) -> Result<crate::captcha::CaptchaBuilder> {
        self.manager.create(kind)
    }

    /// 校验用户答案，限流身份取管理器注入的解析器，未注入时为常量 `"default"`。
    ///
    /// **多用户服务请勿使用**：这会让所有请求共用同一个限流窗口，用户量一大就互相误杀
    /// （且攻击者能靠拖垮公共桶拒绝他人）。改用 [`Guard::verify_as`]，身份由
    /// [`client_identity`]（或 session / uid）按请求派生。
    pub fn verify(&self, key: &str, answer: Answer) -> Result<bool> {
        self.manager.verify(key, answer)
    }

    /// 校验用户答案，显式指定限流身份（客户端 IP / session / uid）。
    ///
    /// 身份只用于限流计数分桶（内部取哈希），不会写进验证码载荷。
    pub fn verify_as(&self, key: &str, answer: Answer, identity: &str) -> Result<bool> {
        self.manager.verify_as(key, answer, identity)
    }

    /// 取已生成验证码的 PNG 字节（供 `GET {path}/{key}` 出图路由使用）。
    pub fn image(&self, key: &str) -> Result<Option<CaptchaImage>> {
        Ok(self.manager.image_bytes(key)?.map(|bytes| CaptchaImage {
            bytes,
            content_type: "image/png",
            cache_control: "no-store",
        }))
    }

    /// 便捷入口：生成一张海报 Builder（默认尺寸取配置）。
    pub fn poster(&self, width: Option<u32>, height: Option<u32>) -> Result<PosterBuilder> {
        let mut builder = PosterBuilder::new()?;
        if let Some(w) = width {
            builder.width(w);
        }
        if let Some(h) = height {
            builder.height(h);
        }
        Ok(builder)
    }

    /// 生成 + 序列化一步到位（框架 handler 直接返回 JSON 用）。
    pub fn create_json(&self, kind: Option<&str>) -> Result<serde_json::Value> {
        let result: CaptchaResult = self.create(kind)?.generate()?;
        serde_json::to_value(result).map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn xff_first_segment_wins_over_peer() {
        assert_eq!(
            client_identity(Some("1.2.3.4, 5.6.7.8"), Some(ip("9.9.9.9"))),
            "1.2.3.4"
        );
    }

    #[test]
    fn xff_is_trimmed_before_use() {
        assert_eq!(client_identity(Some("  1.2.3.4 ,5.6.7.8"), None), "1.2.3.4");
        assert_eq!(client_identity(Some("1.2.3.4,"), None), "1.2.3.4");
    }

    #[test]
    fn blank_xff_falls_back_to_peer() {
        assert_eq!(client_identity(Some(""), Some(ip("9.9.9.9"))), "9.9.9.9");
        assert_eq!(client_identity(Some("   "), Some(ip("9.9.9.9"))), "9.9.9.9");
    }

    #[test]
    fn missing_everything_is_unknown() {
        assert_eq!(client_identity(None, None), "unknown");
    }

    #[test]
    fn ipv6_peer_is_used_verbatim() {
        assert_eq!(
            client_identity(None, Some(ip("2001:db8::1"))),
            "2001:db8::1"
        );
    }

    #[test]
    fn long_xff_is_truncated_to_64_chars() {
        let long = "a".repeat(200);
        assert_eq!(client_identity(Some(&long), None).chars().count(), MAX_IDENTITY_LEN);
        // 多字节字符按字符截断，不破坏 UTF-8 边界
        let cjk = "测".repeat(100);
        assert_eq!(client_identity(Some(&cjk), None).chars().count(), MAX_IDENTITY_LEN);
    }
}
