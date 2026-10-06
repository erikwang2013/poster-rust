//! 项目宠物「Posty」。
//!
//! 图形本体见 `assets/pet.svg` / `assets/pet.png`（左手举二维码卡片、右手举滑块拼图，
//! 身体是一张海报）；终端里没有 SVG，因此这里保留一份等价的 ASCII 版本。
//!
//! 相关：`assets::PET_PNG`（内嵌 PNG）、`PosterBuilder::add_pet()`（画进海报）。

/// 宠物名。
pub const NAME: &str = "Posty";

/// 一句话标语。
pub const TAGLINE: &str = "海报本体 + 二维码卡片 + 滑块拼图 —— 出图与验证";

/// 头顶天线、左手二维码、右手滑块拼图的海报小宠物（无尾随换行）。
///
/// 左列 `▣` 是二维码卡片，右列 `▩` 是滑块拼图，中间圆角框是海报本体。
pub fn art() -> &'static str {
    concat!(
        "        ●        \n",
        "        │        \n",
        " ▣▣▣  ╭──────╮   \n",
        " ▣▣▣  │ ▬▬▬▬ │ ▩▩\n",
        " ▣ ▣  │ ◉  ◉ │ ▩▩\n",
        "      │  ‿   │   \n",
        "      │ ▭▭▭  │   \n",
        "      ╰──┬───╯   \n",
        "        ╰┴╯      ",
    )
}

/// 可直接打印 / 写日志的完整问候，末尾带换行。
pub fn greet() -> String {
    format!("{}\n{} · {}\n", art(), NAME, TAGLINE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn art_is_rectangular() {
        let lines: Vec<&str> = art().lines().collect();
        assert_eq!(lines.len(), 9);
        // 所有行等宽，保证 ASCII 版不漏形。
        let width = lines[0].chars().count();
        assert!(lines.iter().all(|l| l.chars().count() == width));
        assert!(!art().ends_with('\n'));
    }

    #[test]
    fn greet_appends_name_and_tagline() {
        assert_eq!(greet(), format!("{}\nPosty · {}\n", art(), TAGLINE));
    }
}
