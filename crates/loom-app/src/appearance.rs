//! 外观偏好的运行时落地:滚动条显示方式、等待动画文案。
//!
//! @author ixxxxoooo

use crate::UiPrefs;
use gpui_kit::component::Theme;
use gpui_kit::component::scroll::ScrollbarMode;
use gpui_kit::*;
use loom_config::{ScrollbarStyle, SpinnerStyle};

/// 各风格的文案:(风格名, 说明用示例, 处理中, 思考中)。
pub const SPINNERS: &[(SpinnerStyle, &str, &str, &str, &str)] = &[
    (SpinnerStyle::Classic, "经典", "思考中、琢磨中、推敲中…", "处理中…", "思考中…"),
    (SpinnerStyle::Fun, "趣味", "摸鱼中、炼丹中、薅头发中…", "炼丹中…", "摸鱼中…"),
    (SpinnerStyle::ClaudeCode, "Claude Code", "沉思中、踱步中、冥想中…", "踱步中…", "沉思中…"),
    (SpinnerStyle::Poetic, "诗意", "凝神中、遐思中、斟字酌句中…", "斟字酌句中…", "凝神中…"),
    (SpinnerStyle::Geek, "极客", "编译宇宙中、梳理依赖中、调优中…", "梳理依赖中…", "编译宇宙中…"),
    (SpinnerStyle::Minimal, "极简", "工作中", "工作中", "工作中"),
];

/// 当前风格的「处理中」文案。
pub fn working_text(cx: &App) -> &'static str {
    pick(cx, |s| s.3)
}

/// 当前风格的「思考中」文案。
pub fn thinking_text(cx: &App) -> &'static str {
    pick(cx, |s| s.4)
}

/// 取当前风格的某一项文案。
fn pick(cx: &App, f: impl Fn(&(SpinnerStyle, &'static str, &'static str, &'static str, &'static str)) -> &'static str) -> &'static str {
    let cur = cx.try_global::<UiPrefs>().map_or(SpinnerStyle::Classic, |p| p.spinner);
    SPINNERS.iter().find(|s| s.0 == cur).or(SPINNERS.first()).map_or("处理中…", f)
}

/// 应用滚动条显示方式:跟随系统 / 滚动时出现 / 常驻。
pub fn apply_scrollbar(style: ScrollbarStyle, cx: &mut App) {
    match style {
        ScrollbarStyle::System => Theme::sync_scrollbar_appearance(cx),
        ScrollbarStyle::Auto => Theme::set_scrollbar_mode(ScrollbarMode::Scrolling, cx),
        ScrollbarStyle::Always => Theme::set_scrollbar_mode(ScrollbarMode::Always, cx),
    }
}
