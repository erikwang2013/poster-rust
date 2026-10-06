//! 内置素材：项目宠物 Posty、6 张验证码背景、默认中文字体。
//!
//! 路径与 PHP 版 `assets/` 一一对应：
//! - `assets/pet.png` / `assets/pet.svg` —— 宠物，`include_bytes!` 内嵌，随包分发
//! - `assets/backgrounds/*.png` —— 验证码内置背景（400×250），内嵌
//! - `assets/fonts/Alibaba-PuHuiTi-Regular.ttf` —— 默认字体（9.7MB，不内嵌，按路径暴露）

use std::path::PathBuf;

/// 项目宠物 Posty 的 PNG（600×520），用于 `add_pet()` 与缺图占位。
pub const PET_PNG: &[u8] = include_bytes!("../assets/pet.png");

/// 项目宠物 Posty 的矢量源文件（`assets/pet.svg`）。
pub const PET_SVG: &str = include_str!("../assets/pet.svg");

/// 内置验证码背景（名称, PNG 字节），对应 `assets/backgrounds/`。
pub const BACKGROUNDS: [(&str, &[u8]); 6] = [
    ("blue-purple", include_bytes!("../assets/backgrounds/blue-purple.png")),
    ("dark-elegant", include_bytes!("../assets/backgrounds/dark-elegant.png")),
    ("fresh-green", include_bytes!("../assets/backgrounds/fresh-green.png")),
    ("ocean-blue", include_bytes!("../assets/backgrounds/ocean-blue.png")),
    ("pink-pastel", include_bytes!("../assets/backgrounds/pink-pastel.png")),
    ("sunset", include_bytes!("../assets/backgrounds/sunset.png")),
];

/// 随包分发的默认字体路径（阿里巴巴普惠体），对应 PHP `src/fonts/`。
///
/// 经 `CARGO_MANIFEST_DIR` 定位：源码构建与 crates.io 安装（源码目录已解包）均可用。
pub fn default_font_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/fonts/Alibaba-PuHuiTi-Regular.ttf")
}

/// 宠物 PNG 在源码树中的路径（等价 PHP `PosterBuilder::petPath()`）。
///
/// 需要「文件路径」而非字节时使用（如作为二维码中心 Logo）；
/// 直接画进海报请用 [`PET_PNG`] 或 `PosterBuilder::add_pet()`。
pub fn pet_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/pet.png")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_pet_is_a_png() {
        assert_eq!(&PET_PNG[..8], b"\x89PNG\r\n\x1a\n");
        assert!(PET_SVG.contains("Posty"));
    }

    #[test]
    fn bundled_assets_exist_on_disk() {
        let font = default_font_path();
        assert!(font.exists(), "缺少默认字体: {}", font.display());
        assert_eq!(font.file_name().unwrap(), "Alibaba-PuHuiTi-Regular.ttf");
        assert!(pet_path().exists());
    }

    #[test]
    fn six_backgrounds_are_pngs() {
        assert_eq!(BACKGROUNDS.len(), 6);
        for (name, bytes) in BACKGROUNDS {
            assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "背景 {name} 不是 PNG");
        }
    }
}
