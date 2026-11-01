//! 主题:在 gpui-component 的语义 token 之上覆盖出「纸感」浅色 / 深色。
//!
//! 约定:界面里的颜色只通过 `cx.theme()` 取,不在视图里写死色值。

use gpui_kit::component::{ActiveTheme, Theme, ThemeMode};
use gpui_kit::*;

/// 内置主题。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeKind {
    /// 纸感浅色(默认)
    #[default]
    PaperLight,
    /// 纸感深色
    PaperDark,
    /// 跟随系统
    System,
}

impl From<loom_config::ThemeChoice> for ThemeKind {
    fn from(c: loom_config::ThemeChoice) -> Self {
        match c {
            loom_config::ThemeChoice::PaperLight => Self::PaperLight,
            loom_config::ThemeChoice::PaperDark => Self::PaperDark,
            loom_config::ThemeChoice::System => Self::System,
        }
    }
}

fn hex(v: u32) -> Hsla {
    rgb(v).into()
}

/// 应用主题到全局,并让 Dock 图标切到对应的浅色 / 深色版。
pub fn apply(kind: ThemeKind, window: Option<&mut Window>, cx: &mut App) {
    apply_colors(kind, window, cx);
    crate::app_icon::set_dock_icon(cx.theme().is_dark());
}

/// 覆盖主题色值。参数:`kind` 主题;`window` 当前窗口(可选);`cx` 应用上下文。
fn apply_colors(kind: ThemeKind, window: Option<&mut Window>, cx: &mut App) {
    match kind {
        ThemeKind::System => {
            Theme::sync_system_appearance(window, cx);
        }
        ThemeKind::PaperLight => {
            Theme::change(ThemeMode::Light, window, cx);
            Theme::update(cx, |t| {
                t.background = hex(0xF6F6F7);
                t.foreground = hex(0x26251E);
                t.muted = hex(0xECECEE);
                t.muted_foreground = hex(0x77756C);
                t.border = hex(0xE0DFDA);
                t.sidebar = hex(0xEFEFEF);
                t.sidebar_foreground = hex(0x26251E);
                t.sidebar_border = hex(0xE0DFDA);
                t.sidebar_accent = hex(0xE4E4E6);
                t.sidebar_accent_foreground = hex(0x26251E);
                t.popover = hex(0xFFFFFF);
                t.popover_foreground = hex(0x26251E);
                t.accent = hex(0xE8E8EA);
                t.accent_foreground = hex(0x26251E);
                t.list_hover = hex(0xEAEAEC);
                t.list_active = hex(0xE0E0E3);
                t.secondary = hex(0xECECEE);
                t.secondary_hover = hex(0xE4E4E6);
                t.primary = hex(0xF54E00);
                t.primary_hover = hex(0xD94500);
                t.primary_active = hex(0xBF3E00);
                t.primary_foreground = hex(0xFFFFFF);
                t.button_primary = hex(0xF54E00);
                t.button_primary_hover = hex(0xD94500);
                t.button_primary_active = hex(0xBF3E00);
                t.button_primary_foreground = hex(0xFFFFFF);
                t.link = hex(0xC2410C);
                t.success = hex(0x2F7D4F);
                t.warning = hex(0xB7791F);
                t.danger = hex(0xC53030);
                t.info = hex(0x2B6CB0);
            });
        }
        ThemeKind::PaperDark => {
            Theme::change(ThemeMode::Dark, window, cx);
            Theme::update(cx, |t| {
                t.background = hex(0x1D1C19);
                t.foreground = hex(0xE8E6DF);
                t.muted = hex(0x272622);
                t.muted_foreground = hex(0x9C998E);
                t.border = hex(0x35342F);
                t.sidebar = hex(0x161512);
                t.sidebar_foreground = hex(0xE8E6DF);
                t.sidebar_border = hex(0x2C2B27);
                t.sidebar_accent = hex(0x24231F);
                t.sidebar_accent_foreground = hex(0xE8E6DF);
                t.popover = hex(0x272622);
                t.popover_foreground = hex(0xE8E6DF);
                t.accent = hex(0x2B2A26);
                t.accent_foreground = hex(0xE8E6DF);
                t.list_hover = hex(0x2A2924);
                t.list_active = hex(0x34332E);
                t.secondary = hex(0x2B2A26);
                t.secondary_hover = hex(0x34332E);
                t.primary = hex(0xFF6A1F);
                t.primary_hover = hex(0xFF7F3F);
                t.primary_active = hex(0xE55A14);
                t.primary_foreground = hex(0x1D1C19);
                t.button_primary = hex(0xFF6A1F);
                t.button_primary_hover = hex(0xFF7F3F);
                t.button_primary_active = hex(0xE55A14);
                t.button_primary_foreground = hex(0x1D1C19);
                t.link = hex(0xFF9A5C);
                t.success = hex(0x5CC389);
                t.warning = hex(0xE0B04A);
                t.danger = hex(0xF07070);
                t.info = hex(0x6FA8E0);
            });
        }
    }
}

/// Diff 增删行的底色,带透明度,深浅色各一套。
pub fn diff_colors(cx: &App) -> (Hsla, Hsla) {
    let t = cx.theme();
    let mut add = t.success;
    let mut del = t.danger;
    add.a = if t.is_dark() { 0.18 } else { 0.14 };
    del.a = if t.is_dark() { 0.20 } else { 0.12 };
    (add, del)
}
