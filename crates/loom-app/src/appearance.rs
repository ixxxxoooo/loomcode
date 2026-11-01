//! 外观偏好的运行时落地:滚动条显示方式、等待动画文案。
//!
//! @author ixxxxoooo

use crate::UiPrefs;
use gpui_kit::component::Theme;
use gpui_kit::component::scroll::ScrollbarMode;
use gpui_kit::*;
use std::time::Duration;
use loom_config::{ScrollbarStyle, SpinnerStyle};

/// 一种等待动画风格:名称、动词表(与 OhMyAgent 一致)。
pub struct SpinnerDef {
    pub style: SpinnerStyle,
    pub name: &'static str,
    pub verbs: &'static [&'static str],
}

/// 全部风格。
pub const SPINNERS: &[SpinnerDef] = &[
    SpinnerDef { style: SpinnerStyle::Classic, name: "经典", verbs: &["思考中", "琢磨中", "推敲中", "构思中", "斟酌中", "酝酿中", "梳理中", "盘算中"] },
    SpinnerDef { style: SpinnerStyle::Fun, name: "趣味", verbs: &["摸鱼中", "炼丹中", "薅头发中", "掐指一算中", "憋大招中", "搬砖中", "喂猫中", "冲咖啡中"] },
    SpinnerDef { style: SpinnerStyle::ClaudeCode, name: "Claude Code", verbs: &["沉思中", "踱步中", "冥想中", "酿造中", "蹦跶中", "琢磨透中", "神游中", "凝思中"] },
    SpinnerDef { style: SpinnerStyle::Poetic, name: "诗意", verbs: &["凝神中", "遐思中", "斟字酌句中", "静待花开中", "拾掇思绪中", "望月沉吟中", "研墨中", "听风中"] },
    SpinnerDef { style: SpinnerStyle::Geek, name: "极客", verbs: &["编译宇宙中", "梳理依赖中", "调优中", "遍历语法树中", "回收内存中", "预热缓存中", "握手中", "解析意图中"] },
    SpinnerDef { style: SpinnerStyle::Minimal, name: "极简", verbs: &["工作中"] },
];

/// 设置页每行的示例:前三个动词。
pub fn sample(def: &SpinnerDef) -> String {
    let v: Vec<&str> = def.verbs.iter().take(3).copied().collect();
    if v.len() < 3 { format!("{}", v.join("、")) } else { format!("{}…", v.join("、")) }
}

/// 当前风格。
fn current(cx: &App) -> &'static SpinnerDef {
    let cur = cx.try_global::<UiPrefs>().map_or(SpinnerStyle::Classic, |p| p.spinner);
    SPINNERS.iter().find(|d| d.style == cur).unwrap_or(&SPINNERS[0])
}

/// 稳定地挑一个动词并加省略号:同一个 `seed`(会话 / 条目 id)永远得到同一个词,不会乱跳。
pub fn verb_text(cx: &App, seed: &str) -> String {
    let def = current(cx);
    let h = seed.bytes().fold(0usize, |a, b| a.wrapping_mul(31).wrapping_add(usize::from(b)));
    format!("{}…", def.verbs[h % def.verbs.len()])
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

/// 动画字符:所有帧叠在同一格里,每 150ms 显示下一帧(与 OhMyAgent 一致),循环播放。
///
/// 参数:`id` 元素 id 前缀;`style` 风格;`color` 字符颜色;`size` 字号。
pub fn glyph_anim(id: &str, style: SpinnerStyle, color: Hsla, size: f32) -> AnyElement {
    let frames = glyph_frames(style);
    let n = frames.len();
    div().relative().size(px(size + 4.)).flex_none().children(frames.iter().enumerate().map(|(i, g)| {
        div().absolute().size_full().flex().items_center().justify_center().text_size(px(size)).text_color(color).child(g.to_string())
            .with_animation(SharedString::from(format!("{id}-g{i}")), Animation::new(Duration::from_millis(150 * n as u64)).repeat(),
                move |d, t| d.opacity(if ((t * n as f32) as usize).min(n - 1) == i { 1. } else { 0. }))
    })).into_any_element()
}

/// 当前风格的动画字符(会话里的处理中 / 思考中提示用)。
pub fn glyph_spinner(id: &str, color: Hsla, cx: &App) -> AnyElement {
    glyph_anim(id, current(cx).style, color, 14.)
}
