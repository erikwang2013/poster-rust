//! 旋转验证码示例：随机角度旋圆图 → 落盘 → 用内部答案自校验。
//!
//! ```bash
//! cargo run --example captcha_rotate
//! ```

use std::sync::Arc;

use poster::PosterConfig;
use poster::captcha::{Answer, CaptchaManager};
use poster::storage::{MemoryStorage, Storage};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::path::Path::new("examples/output");
    std::fs::create_dir_all(output)?;

    let storage = Arc::new(MemoryStorage::new());
    let manager =
        CaptchaManager::with_config_and_storage(Arc::new(PosterConfig::default()), storage.clone());

    let result = manager
        .create(Some("rotate"))?
        .set_size(240)              // 圆图直径，钳制到 60-400
        .set_angle_range(60.0, 300.0)
        .set_difficulty("hard")     // hard 的角度范围 90-330
        .generate()?;

    let image = output.join(format!("rotate-{}.png", result.key));
    std::fs::write(
        &image,
        manager.image_bytes(&result.key)?.ok_or("图片未持久化")?,
    )?;

    let payload = storage.get(&result.key)?.ok_or("答案不在存储里")?.json()?;
    let angle = payload["angle"].as_f64().ok_or("载荷缺 angle")? as f32;

    println!("key       = {}", result.key);
    println!("类型      = {}", result.captcha_type);
    println!(
        "图片      = {}（圆图 {}×{}）",
        image.display(),
        payload["orig_size"]["width"],
        payload["orig_size"]["height"]
    );
    println!("正确答案  = {angle}°（用户拖回 0°；±5° 容差，按圆周取最短边）");

    // 差 9° 超容差：拒绝，且 key 保留（还剩 2 次机会）
    println!(
        "校验(角度+9) = {}",
        manager.verify(&result.key, Answer::Rotate(angle + 9.0))?
    );
    // 差 3° 在容差内：通过，key 随即删除（一次性）
    println!(
        "校验(角度+3) = {}",
        manager.verify(&result.key, Answer::Rotate(angle + 3.0))?
    );
    println!(
        "校验(再提交) = {}（成功即删除 key）",
        manager.verify(&result.key, Answer::Rotate(angle))?
    );

    Ok(())
}
