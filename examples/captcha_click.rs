//! 点击验证码示例：文字目标 / 矢量图标两种形态 → 落盘 → 用内部答案自校验。
//!
//! ```bash
//! cargo run --example captcha_click
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

    // 图标形态：11 种程序化矢量图形，提示里带 base64 缩略图
    let result = manager
        .create(Some("click"))?
        .set_difficulty("hard")
        .set_target_type("icon")
        .generate()?;

    let image = output.join(format!("click-{}.png", result.key));
    std::fs::write(
        &image,
        manager.image_bytes(&result.key)?.ok_or("图片未持久化")?,
    )?;

    let payload = storage.get(&result.key)?.ok_or("答案不在存储里")?.json()?;
    let targets = payload["targets"].as_array().ok_or("载荷缺 targets")?;
    let answer: Vec<(f32, f32)> = targets
        .iter()
        .map(|target| {
            Ok((
                target["x"].as_f64().ok_or("缺 x")? as f32,
                target["y"].as_f64().ok_or("缺 y")? as f32,
            ))
        })
        .collect::<Result<_, Box<dyn std::error::Error>>>()?;

    println!("key    = {}", result.key);
    println!("类型   = {}（hard = 4 个目标）", result.captcha_type);
    println!("图片   = {}", image.display());
    for (index, item) in result.extra["texts"].as_array().ok_or("缺 texts")?.iter().enumerate() {
        let thumb = item["thumb"].as_str().map(|_| "（含缩略图）").unwrap_or("");
        println!("提示{}  = {}{}", index + 1, item["text"], thumb);
    }
    println!("目标   = {answer:?}（顺序须一致，容差半径 18px）");

    // 故意打乱顺序：顺序错即失败，且消耗一次尝试次数
    let mut reversed = answer.clone();
    reversed.reverse();
    println!("校验(乱序) = {}", manager.verify(&result.key, Answer::Click(reversed))?);
    println!("校验(正确) = {}", manager.verify(&result.key, Answer::Click(answer))?);

    Ok(())
}
