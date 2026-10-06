//! 验证码管理器，对应 PHP `CaptchaManager`：创建 → 生成 → 校验的入口。
//!
//! 校验顺序与 PHP 一致：
//! 1. 跨 key 的窗口限流先行——单 key 计数挡不住「每次换新 key 再猜一次」；
//! 2. 取载荷，缺失（不存在 / 已过期 / 损坏）即失败；
//! 3. **先自增再判定**，以存储的原子自增返回值为本次尝试序号：读写之间即使有并发插入，
//!    也不会放大放行次数；
//! 4. 超出 `captcha.max_attempts` → 删 key（连图片一起），失败；
//! 5. 通过 → 删 key（一次性），失败 → 保留剩余次数。
//!
//! 类型从存储的载荷里取，`verify()` 不必再传 type；答案变体与载荷类型不符时返回
//! `Ok(false)`。存储故障以 `Err` 上抛（PHP 一律吞成 false，Rust 侧让部署问题可见）。

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use crate::config::PosterConfig;
use crate::error::Result;
use crate::storage::{FileStorage, MemoryStorage, Storage};

use super::{Answer, CaptchaBuilder, CaptchaFactory, Ctx, image_key, rate_limiter, trajectory};

/// 验证码管理器。`Send + Sync`，无全局可变状态，可 `Arc` 共享给各 Web 框架的请求守卫。
pub struct CaptchaManager {
    ctx: Arc<Ctx>,
    /// 限流身份解析器：登录用户可用 uid，默认由框架层注入客户端 IP 等。
    identity: Option<Arc<dyn Fn() -> String + Send + Sync>>,
}

impl Default for CaptchaManager {
    fn default() -> Self {
        Self::with_config(Arc::new(PosterConfig::default()))
    }
}

impl CaptchaManager {
    /// 按全局配置构造：`captcha.file_path` 配了就用文件存储（会建目录，故可能失败），
    /// 否则进程内存储。
    ///
    /// 需要分布式共享时自行构造 `RedisStorage`（feature = `redis`）
    /// 并走 [`with_storage`](Self::with_storage)。
    pub fn new() -> Result<Self> {
        let config = Arc::new(crate::config::global().clone());
        let storage: Arc<dyn Storage> = match config.captcha.file_path {
            Some(_) => Arc::new(FileStorage::from_config(&config)?),
            None => Arc::new(MemoryStorage::new()),
        };
        Ok(Self::with_config_and_storage(config, storage))
    }

    /// 指定配置（存储为进程内存储，忽略 `captcha.file_path`）。
    pub fn with_config(config: Arc<PosterConfig>) -> Self {
        Self::with_config_and_storage(config, Arc::new(MemoryStorage::new()))
    }

    /// 指定存储（配置取默认值）。
    pub fn with_storage(storage: Arc<dyn Storage>) -> Self {
        Self::with_config_and_storage(Arc::new(PosterConfig::default()), storage)
    }

    /// 指定配置与存储。
    pub fn with_config_and_storage(config: Arc<PosterConfig>, storage: Arc<dyn Storage>) -> Self {
        Self {
            ctx: Arc::new(Ctx { config, storage }),
            identity: None,
        }
    }

    /// 注入限流身份解析器（PHP 构造函数里的 `$identityResolver`）。
    pub fn with_identity_resolver(
        mut self,
        resolver: Arc<dyn Fn() -> String + Send + Sync>,
    ) -> Self {
        self.identity = Some(resolver);
        self
    }

    /// 存储写探针：接线期（应用启动）调用，让后端不可用在第一个请求前就暴露。
    pub fn probe(&self) -> Result<()> {
        let key = format!("probe:{}", super::generate_key());
        let ttl = Duration::from_secs(5);
        self.ctx.storage.set(&key, b"1", ttl)?;
        self.ctx.storage.get(&key)?;
        self.ctx.storage.delete(&key)?;
        Ok(())
    }

    /// 创建验证码 Builder。`None` = 配置默认类型；`random` 解析成三种之一。
    pub fn create(&self, captcha_type: Option<&str>) -> Result<CaptchaBuilder> {
        CaptchaFactory::create(Arc::clone(&self.ctx), captcha_type)
    }

    /// 校验（限流身份取自身份解析器，未注入时为 `"default"`）。
    pub fn verify(&self, key: &str, answer: Answer) -> Result<bool> {
        let identity = match &self.identity {
            Some(resolver) => resolver(),
            None => "default".to_string(),
        };
        self.verify_as(key, answer, &identity)
    }

    /// 校验，显式指定限流身份（多用户服务应走这个入口：IP / uid / session）。
    pub fn verify_as(&self, key: &str, answer: Answer, identity: &str) -> Result<bool> {
        let config = &self.ctx.config;
        if !rate_limiter::allow(
            &config.captcha.rate_limit,
            self.ctx.storage.as_ref(),
            identity,
        )? {
            return Ok(false);
        }

        let Some(entry) = self.ctx.storage.get(key)? else {
            return Ok(false);
        };
        // 载荷损坏按「不存在」处理（PHP 各存储实现同样以 null/0 表达）
        let Ok(payload) = entry.json() else {
            return Ok(false);
        };

        let Some(attempts) = self.ctx.storage.increment(key)? else {
            // 自增不可用：拿不到可信序号，失败关闭
            return Ok(false);
        };
        if attempts > config.captcha.max_attempts as u64 {
            self.drop_keys(key)?;
            return Ok(false);
        }

        let passed = check(config, &payload, &answer);
        if passed {
            self.drop_keys(key)?;
        }
        Ok(passed)
    }

    /// 取生成时持久化的图片 PNG 字节，供框架层的 `GET {path}/{key}` 端点直出
    /// （等价 PHP 的 `captcha.route` / `CaptchaImage::png()`）。key 不存在或已过期返回 `None`。
    pub fn image_bytes(&self, key: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .ctx
            .storage
            .get(&image_key(key))?
            .map(|entry| entry.value))
    }

    /// 删除答案与图片（一次性语义）。
    fn drop_keys(&self, key: &str) -> Result<()> {
        self.ctx.storage.delete(key)?;
        self.ctx.storage.delete(&image_key(key))?;
        Ok(())
    }
}

/// 类型 + 答案比对，容差与轨迹口径见模块文档。
fn check(config: &PosterConfig, payload: &Value, answer: &Answer) -> bool {
    let kind = payload.get("type").and_then(Value::as_str).unwrap_or("");
    let tolerance = &config.captcha.tolerance;

    match (kind, answer) {
        ("click", Answer::Click(points)) => check_click(payload, points, tolerance.click),
        ("rotate", Answer::Rotate(angle)) | ("rotate", Answer::RotateWithTrail { angle, .. }) => {
            trajectory::verify(&config.captcha.trajectory, answer.trajectory().as_ref())
                && check_rotate(payload, *angle, tolerance.rotate)
        }
        ("slider", Answer::Slider(x)) | ("slider", Answer::SliderWithTrail { x, .. }) => {
            trajectory::verify(&config.captcha.trajectory, answer.trajectory().as_ref())
                && check_slider(payload, *x, tolerance.slider)
        }
        _ => false,
    }
}

/// 点击：坐标顺序 + 容差半径（PHP `checkClick`）。
fn check_click(payload: &Value, points: &[(f32, f32)], tolerance: f32) -> bool {
    let Some(targets) = payload.get("targets").and_then(Value::as_array) else {
        return false;
    };
    if targets.is_empty() || points.len() != targets.len() {
        return false;
    }
    for (target, (x, y)) in targets.iter().zip(points.iter()) {
        let (Some(tx), Some(ty)) = (
            target.get("x").and_then(Value::as_f64),
            target.get("y").and_then(Value::as_f64),
        ) else {
            return false;
        };
        let (dx, dy) = (*x as f64 - tx, *y as f64 - ty);
        if (dx * dx + dy * dy).sqrt() > tolerance as f64 {
            return false;
        }
    }
    true
}

/// 旋转：按圆周差取最短边（PHP `checkRotate`）。
fn check_rotate(payload: &Value, angle: f32, tolerance: f32) -> bool {
    let Some(actual) = payload.get("angle").and_then(Value::as_f64) else {
        return false;
    };
    let mut user = (angle % 360.0) as f64;
    if user < 0.0 {
        user += 360.0;
    }
    let mut diff = (user - actual).abs();
    if diff > 180.0 {
        diff = 360.0 - diff;
    }
    diff <= tolerance as f64
}

/// 滑块：x 容差（PHP `checkSlider`）。
fn check_slider(payload: &Value, x: f32, tolerance: f32) -> bool {
    let Some(actual) = payload.get("x").and_then(Value::as_f64) else {
        return false;
    };
    (x as f64 - actual).abs() <= tolerance as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rotate_wraps_around_the_circle() {
        let payload = json!({"angle": 359.0});
        assert!(check_rotate(&payload, 1.0, 5.0), "359° 与 1° 相差 2°");
        assert!(check_rotate(&payload, -1.0, 5.0), "负角度先归一到 359°");
        assert!(!check_rotate(&payload, 10.0, 5.0));

        let payload = json!({"angle": 10.0});
        assert!(check_rotate(&payload, 370.0, 5.0), "370° ≡ 10°");
        assert!(!check_rotate(&json!({}), 10.0, 5.0), "载荷缺 angle");
    }

    #[test]
    fn slider_uses_absolute_tolerance() {
        let payload = json!({"x": 100.0});
        assert!(check_slider(&payload, 96.0, 4.0));
        assert!(check_slider(&payload, 104.0, 4.0));
        assert!(!check_slider(&payload, 95.9, 4.0));
        assert!(!check_slider(&json!({"x": "100"}), 100.0, 4.0), "非数值不算数");
    }

    #[test]
    fn click_requires_order_and_count() {
        let payload = json!({"targets": [
            {"x": 50, "y": 50},
            {"x": 120, "y": 90},
        ]});
        assert!(check_click(&payload, &[(50.0, 50.0), (120.0, 90.0)], 18.0));
        assert!(
            !check_click(&payload, &[(120.0, 90.0), (50.0, 50.0)], 18.0),
            "顺序相反应失败"
        );
        assert!(!check_click(&payload, &[(50.0, 50.0)], 18.0), "数量不符");
        assert!(!check_click(&json!({"targets": []}), &[], 18.0), "空目标");
    }

    #[test]
    fn trajectory_gate_applies_only_when_enabled() {
        let mut config = PosterConfig::default();
        let payload = json!({"type": "slider", "x": 100.0});

        // 默认关闭：裸数值答案直接按 x 判定
        assert!(check(&config, &payload, &Answer::Slider(100.0)));

        // 开启后裸数值一律失败（旧前端无法证明是人在操作）
        config.captcha.trajectory.enabled = true;
        assert!(!check(&config, &payload, &Answer::Slider(100.0)));
        assert!(check(
            &config,
            &payload,
            &Answer::SliderWithTrail {
                x: 100.0,
                trail: vec![(0.0, 0.0, 0.0), (5.0, 0.0, 150.0), (6.0, 1.0, 300.0), (30.0, 0.0, 450.0)],
                duration_ms: 500,
            }
        ));
        assert!(!check(
            &config,
            &payload,
            &Answer::SliderWithTrail {
                x: 100.0,
                trail: vec![(0.0, 0.0, 0.0), (30.0, 0.0, 500.0)],
                duration_ms: 500,
            }
        ), "点数不足");
    }

    #[test]
    fn type_mismatch_and_unknown_type_fail() {
        let config = PosterConfig::default();
        assert!(!check(
            &config,
            &json!({"type": "click", "targets": []}),
            &Answer::Slider(1.0)
        ));
        assert!(!check(&config, &json!({"type": "unknown"}), &Answer::Slider(1.0)));
        assert!(!check(&config, &json!({}), &Answer::Click(vec![])));
    }
}
