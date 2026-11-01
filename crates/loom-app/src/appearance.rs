//! 外观偏好的运行时落地:滚动条显示方式、等待动画文案。
//!
//! @author ixxxxoooo

use crate::UiPrefs;
use gpui_kit::component::Theme;
use gpui_kit::component::scroll::ScrollbarMode;
use gpui_kit::*;
use std::time::Duration;
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

/// 各风格的动画字符帧(与 OhMyAgent 一致)。
pub fn glyph_frames(style: SpinnerStyle) -> &'static [&'static str] {
    match style {
        SpinnerStyle::Classic | SpinnerStyle::Fun | SpinnerStyle::ClaudeCode => &["·", "✢", "✳", "✶", "✻", "✽", "✻", "✶", "✳", "✢"],
        SpinnerStyle::Poetic => &["◜", "◠", "◝", "◞", "◡", "◟"],
        SpinnerStyle::Geek => &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
        SpinnerStyle::Minimal => &["●", "◉", "○", "◉"],
    }
}

/// 静态展示用的代表字符(设置页每一行左侧)。
pub fn glyph_static(style: SpinnerStyle) -> &'static str {
    match style {
        SpinnerStyle::Classic | SpinnerStyle::Fun | SpinnerStyle::ClaudeCode => "✻",
        SpinnerStyle::Poetic => "◠",
        SpinnerStyle::Geek => "⠿",
        SpinnerStyle::Minimal => "●",
    }
}

/// 当前风格的动画字符:所有帧叠在同一格里,按时间轮流显示一帧(重复播放)。
///
/// 参数:`id` 元素 id 前缀;`color` 字符颜色;`cx` 应用上下文(读取当前风格)。
pub fn glyph_spinner(id: &str, color: Hsla, cx: &App) -> AnyElement {
    let style = cx.try_global::<UiPrefs>().map_or(SpinnerStyle::Classic, |p| p.spinner);
    let frames = glyph_frames(style);
    let n = frames.len();
    div().relative().size(px(16.)).flex_none().children(frames.iter().enumerate().map(|(i, g)| {
        div().absolute().size_full().flex().items_center().justify_center().text_size(px(14.)).text_color(color).child(g.to_string())
            .with_animation(SharedString::from(format!("{id}-g{i}")), Animation::new(Duration::from_millis(110 * n as u64)).repeat(),
                move |d, t| d.opacity(if ((t * n as f32) as usize).min(n - 1) == i { 1. } else { 0. }))
    })).into_any_element()
}
