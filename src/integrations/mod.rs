//! 框架集成（feature 门控）。
//!
//! 八个适配器全部产出 [`crate::guard::Guard`]：
//! axum / actix-web / rocket / poem / salvo / warp / bee-rust / e-cat。
//! 每个适配器都提供「请求守卫提取」与「出图路由」（`GET {path}/{key} → image/png`）。

/// `POST {path}/verify` 请求体的字节上限（2 MiB，与 axum/actix/salvo 的默认体上限一致）。
///
/// 不能更小：开启轨迹校验（`captcha.track`）后 `answer` 里要带一整条鼠标轨迹，
/// 正常量级远高于 64 KiB；2 MiB 已经足够挡住「无上限读进内存」的 DoS。
/// 目前只有 warp（默认无限制）与 poem（全量读）需要显式用它；
/// 其余四家的 JSON 提取器自带同等量级的默认上限。
#[cfg(any(feature = "poem", feature = "warp"))]
pub(crate) const MAX_VERIFY_BODY: usize = 2 << 20;

/// 把可能阻塞的 Guard 调用丢进 tokio 阻塞线程池（出图 / 生成 / 校验都是 CPU + 存储 IO 重活，
/// 直接在 async handler 里跑会卡住整个 reactor）。
///
/// 任务 panic 时返回 500 侧的错误（与业务错误同样处理）。
#[cfg(any(feature = "axum", feature = "rocket", feature = "poem", feature = "salvo", feature = "warp"))]
pub(crate) async fn run_blocking<T>(
    f: impl FnOnce() -> crate::error::Result<T> + Send + 'static,
) -> crate::error::Result<T>
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .unwrap_or_else(|_| Err(crate::error::PosterError::Other("阻塞任务 panic".into())))
}

#[cfg(feature = "axum")]
pub mod axum;

#[cfg(feature = "actix")]
pub mod actix;

#[cfg(feature = "rocket")]
pub mod rocket;

#[cfg(feature = "poem")]
pub mod poem;

#[cfg(feature = "salvo")]
pub mod salvo;

#[cfg(feature = "warp")]
pub mod warp;

#[cfg(feature = "bee-rust")]
pub mod bee_rust;

#[cfg(feature = "ecat")]
pub mod ecat;
