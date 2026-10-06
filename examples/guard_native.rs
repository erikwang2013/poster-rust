//! 原生 Guard 示例：不依赖任何 Web 框架。
//!
//! ```bash
//! cargo run --example guard_native
//! ```

use std::sync::Arc;

use poster::captcha::{Answer, CaptchaManager};
use poster::Guard;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 接线期构造：存储探针失败会在这里报错，而不是在第一个请求
    let guard = Guard::from_manager(Arc::new(CaptchaManager::new()?))?;

    // 生成一张滑块验证码
    let result = guard.create(Some("slider"))?.generate()?;
    println!("key    = {}", result.key);
    println!("type   = {}", result.captcha_type);
    println!("image  = {}…（{} 字节 data URI）", &result.image[..40.min(result.image.len())], result.image.len());
    println!("extra  = {}", result.extra);

    // 出图路由的等价物：拿到 PNG 字节
    let png = guard.image(&result.key)?;
    println!("png    = {} 字节", png.map(|i| i.bytes.len()).unwrap_or(0));

    // 故意用错误的答案校验（真实场景里答案来自前端）
    let pass = guard.verify(&result.key, Answer::Slider(0.0))?;
    println!("verify(错误答案) = {pass}");

    Ok(())
}
