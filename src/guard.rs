//! 原生 Guard（请求守卫）：不依赖任何框架的通用入口。
//!
//! 八个框架适配器（axum / actix-web / rocket / poem / salvo / warp / bee-rust / e-cat）
//! 全部产出 `Guard`；未适配的框架也可以自己从应用状态里克隆一份。
//!
//! `Guard` 只是 `Arc<CaptchaManager>` 的一层薄包装：接线期构造（`from_manager` 快速失败），
//! 请求期克隆只是两次原子计数，`Send + Sync`，适合每请求克隆。

use std::sync::Arc;

use crate::captcha::{Answer, CaptchaManager, CaptchaResult};
use crate::error::Result;
use crate::poster::PosterBuilder;

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
#[derive(Debug, Clone)]
pub struct Guard {
    manager: Arc<CaptchaManager>,
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

    /// 校验用户答案。
    pub fn verify(&self, key: &str, answer: Answer) -> Result<bool> {
        self.manager.verify(key, answer)
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
