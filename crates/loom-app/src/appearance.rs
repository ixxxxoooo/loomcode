//! 外观偏好的运行时落地:滚动条显示方式、等待动画文案。
//!
//! @author ixxxxoooo

use crate::UiPrefs;
use gpui_kit::component::{ActiveTheme as _, Theme};
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

/// 字符动画每一帧停留的时间(与 OhMyAgent 一致)。
const GLYPH_FRAME_MS: u64 = 150;
/// 字符动画的刷新帧率上限:比换帧频率(1000 / 150 ≈ 6.7)略高,避免换帧时刻被量化得忽快忽慢。
const GLYPH_FPS: f32 = 10.;

/// 字符动画下标:`t` 是一个循环内的进度(0..1),`n` 是帧数。返回:当前该显示的帧下标。
pub fn glyph_index(t: f32, n: usize) -> usize {
    ((t * n as f32) as usize).min(n.saturating_sub(1))
}

/// 动画字符:每 150ms 换下一帧,循环播放。
///
/// 只有一个元素、一个动画,并限频到 10 帧 / 秒(以前每个字形各叠一个元素、每个都按 60 帧重绘,
/// 空闲时也让工作台每秒重建 60 次)。
///
/// 参数:`id` 元素 id 前缀;`style` 风格;`color` 字符颜色;`size` 字号。
pub fn glyph_anim(id: &str, style: SpinnerStyle, color: Hsla, size: f32) -> AnyElement {
    let frames = glyph_frames(style);
    let n = frames.len();
    div().id(SharedString::from(format!("{id}-glyph"))).relative().size(px(size + 4.)).flex_none().flex().items_center().justify_center().text_size(px(size)).text_color(color)
        .with_animation(
            SharedString::from(format!("{id}-g")),
            Animation::new(Duration::from_millis(GLYPH_FRAME_MS * n as u64)).repeat_synced().with_max_fps(GLYPH_FPS),
            move |d, t| d.child(frames[glyph_index(t, n)].to_string()),
        )
        .into_any_element()
}

/// 当前风格的动画字符(会话里的处理中 / 思考中提示用)。
pub fn glyph_spinner(id: &str, color: Hsla, cx: &App) -> AnyElement {
    glyph_anim(id, current(cx).style, color, 14.)
}

/// 会话流行内的小号转圈(工具行、工具组、子 Agent),与思考 / 处理中同一种字符动画,随外观偏好。
///
/// 参数:`id` 元素 id 前缀(同一时刻唯一)。
pub fn row_spinner(id: &str, cx: &App) -> AnyElement {
    glyph_anim(id, current(cx).style, cx.theme().primary, 11.)
}

/// 耗时文字:`12s` / `1m 05s` / `1h 02m`。
pub fn elapsed_text(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m {:02}s", secs / 60, secs % 60),
        _ => format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60),
    }
}

/// 整行等待文案:`思考中… (12s · 按 Esc 中断)`。
///
/// 参数:`verb` 带省略号的动词;`started` 回合开始时间(没有则不显示耗时与提示)。
pub fn status_line(verb: String, started: Option<std::time::Instant>) -> String {
    match started {
        Some(t) => format!("{verb} ({} · 按 Esc 中断)", elapsed_text(t.elapsed().as_secs())),
        None => verb,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn formats_elapsed() {
        assert_eq!(elapsed_text(5), "5s");
        assert_eq!(elapsed_text(65), "1m 05s");
        assert_eq!(elapsed_text(3720), "1h 02m");
    }

    #[::core::prelude::v1::test]
    fn 字符动画下标覆盖全部帧且不越界() {
        let n = 10;
        let seen: std::collections::BTreeSet<usize> = (0..=100).map(|i| glyph_index(i as f32 / 100., n)).collect();
        assert_eq!(seen.len(), n, "一个循环里每一帧都应出现");
        assert_eq!(glyph_index(1.0, n), n - 1, "进度恰为 1 时不越界");
        assert_eq!(glyph_index(0.0, n), 0);
        assert_eq!(glyph_index(0.5, 0), 0, "没有帧时不 panic");
    }

    #[::core::prelude::v1::test]
    fn 所有风格都有帧() {
        use loom_config::SpinnerStyle::*;
        for s in [Classic, Fun, ClaudeCode, Poetic, Geek, Minimal] {
            assert!(glyph_frames(s).len() >= 4);
        }
    }
}
