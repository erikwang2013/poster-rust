//! 滑块验证码示例：生成凹凸拼图 → 落盘 → 用内部答案自校验。
//!
//! ```bash
//! cargo run --example captcha_slider
//! ```
//!
//! 真实场景里答案只留在服务端（`CaptchaManager::verify`），
//! 这里为了自校验顺手把存储里的载荷读出来演示。

use std::sync::Arc;

use poster::PosterConfig;
use poster::captcha::{Answer, CaptchaManager};
use poster::storage::{MemoryStorage, Storage};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::path::Path::new("examples/output");
    std::fs::create_dir_all(output)?;

    // 存储句柄留给示例自己读答案；生产代码不需要它
    let storage = Arc::new(MemoryStorage::new());
    let manager =
        CaptchaManager::with_config_and_storage(Arc::new(PosterConfig::default()), storage.clone());

    let result = manager
        .create(Some("slider"))?
        .set_shape("jigsaw")
        .generate()?;

    let background = output.join(format!("slider-{}.png", result.key));
    std::fs::write(
        &background,
        manager.image_bytes(&result.key)?.ok_or("图片未持久化")?,
    )?;
    let puzzle = output.join(format!("slider-{}-puzzle.png", result.key));
    std::fs::write(&puzzle, decode_data_uri(result.extra["puzzle"].as_str().ok_or("缺 puzzle")?)?)?;

    let payload = storage.get(&result.key)?.ok_or("答案不在存储里")?.json()?;
    let x = payload["x"].as_f64().ok_or("载荷缺 x")? as f32;

    println!("key        = {}", result.key);
    println!("类型       = {}", result.captcha_type);
    println!(
        "拼图本体   = {}×{}（jigsaw 的拼图块 PNG 每边再外扩一个凸起半径）",
        result.extra["puzzle_w"], result.extra["puzzle_h"]
    );
    println!("正确答案 x = {x}（拼图块 PNG 左上角，容差 ±4px）");
    println!("背景       = {}", background.display());
    println!("拼图块     = {}", puzzle.display());
    println!("校验(正确) = {}", manager.verify(&result.key, Answer::Slider(x))?);
    // 一次性：上面那次通过后 key 已删除
    println!("再校验     = {}", manager.verify(&result.key, Answer::Slider(x))?);

    Ok(())
}

/// `data:image/png;base64,…` → PNG 字节。
fn decode_data_uri(uri: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use base64::Engine;
    let b64 = uri.strip_prefix("data:image/png;base64,").ok_or("不是 PNG data URI")?;
    Ok(base64::engine::general_purpose::STANDARD.decode(b64)?)
}
