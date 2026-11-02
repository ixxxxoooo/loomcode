//! 终端主题:前景 / 背景色与 256 色调色板。
//!
//! 命令行程序(如 opencode、vim、delta)靠 OSC 10 / 11 / 4 查询终端的前景、背景与调色板色,
//! 或读环境变量 `COLORFGBG`,来决定自己用深色还是浅色主题;终端不应答它们就只能按深色处理。
//!
//! @author ixxxxoooo

use alacritty_terminal::vte::ansi::Rgb;
use std::sync::{Arc, Mutex};

/// 调色板查询函数:序号 0..=255 → (r, g, b)。
pub type PaletteFn = Arc<dyn Fn(u8) -> (u8, u8, u8) + Send + Sync>;

/// 终端当前使用的颜色。
#[derive(Clone)]
pub struct TermTheme {
    /// 默认前景色
    pub fg: (u8, u8, u8),
    /// 默认背景色
    pub bg: (u8, u8, u8),
    /// 是否深色主题
    pub dark: bool,
    /// 调色板
    pub palette: PaletteFn,
}

impl std::fmt::Debug for TermTheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TermTheme {{ fg: {:?}, bg: {:?}, dark: {} }}", self.fg, self.bg, self.dark)
    }
}

impl TermTheme {
    /// 环境变量 `COLORFGBG` 的值:`前景;背景`,用 ANSI 序号表示(深色 `15;0`,浅色 `0;15`)。
    pub fn colorfgbg(&self) -> &'static str {
        if self.dark { "15;0" } else { "0;15" }
    }

    /// 颜色是否相同(调色板函数无法比较,深浅色一致即视为同一套)。
    pub fn same(&self, other: &Self) -> bool {
        self.fg == other.fg && self.bg == other.bg && self.dark == other.dark
    }
}

/// 与读线程、界面共享的主题槽;没设置过时为 `None`(此时不应答颜色查询)。
pub(crate) type ThemeSlot = Arc<Mutex<Option<TermTheme>>>;

/// 按序号取颜色:0..=255 走调色板,256 前景,257 背景,258 光标(取前景);其余返回 `None`。
pub(crate) fn lookup(theme: &TermTheme, index: usize) -> Option<Rgb> {
    let (r, g, b) = match index {
        0..=255 => (theme.palette)(index as u8),
        256 | 258 => theme.fg,
        257 => theme.bg,
        _ => return None,
    };
    Some(Rgb { r, g, b })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(dark: bool) -> TermTheme {
        TermTheme { fg: (1, 2, 3), bg: (250, 251, 252), dark, palette: Arc::new(|i| (i, i, i)) }
    }

    #[test]
    fn 按序号取前景背景与调色板() {
        let t = sample(false);
        assert_eq!(lookup(&t, 256), Some(Rgb { r: 1, g: 2, b: 3 }));
        assert_eq!(lookup(&t, 257), Some(Rgb { r: 250, g: 251, b: 252 }));
        assert_eq!(lookup(&t, 9), Some(Rgb { r: 9, g: 9, b: 9 }));
        assert_eq!(lookup(&t, 300), None);
    }

    #[test]
    fn colorfgbg_随深浅色() {
        assert_eq!(sample(true).colorfgbg(), "15;0");
        assert_eq!(sample(false).colorfgbg(), "0;15");
    }
}
