//! 主题:在 gpui-component 的语义 token 之上覆盖出多套配色(纸感 / One / Claude / Nord / Solarized / Dracula / Gruvbox)与对应的语法高亮。
//!
//! 约定:界面里的颜色只通过 `cx.theme()` 取,不在视图里写死色值;圆角用下面的三档令牌。
//! 强调色按主题各记一个,并按明暗自动调整明度;悬停 / 选中 / 焦点 / 选区等都从最终主色派生,随主题与强调色联动。
//!
//! @author ixxxxoooo

use crate::theme_palettes::*;
use gpui_kit::component::highlighter::HighlightTheme;
use gpui_kit::component::{ActiveTheme, Colorize as _, Theme, ThemeMode};
use gpui_kit::*;
use loom_config::ThemeChoice;
use std::sync::Arc;

/// 设计令牌:圆角三档(小控件 / 输入与分段 / 卡片与面板),界面里的圆角统一从这里取。
pub const R_SM: f32 = 6.;
pub const R_MD: f32 = 8.;
pub const R_LG: f32 = 12.;

/// 内置主题(与设置里的 `ThemeChoice` 同一类型)。
pub type ThemeKind = ThemeChoice;

/// 主题的展示信息:(名称, 背景, 前景, 主题色),用于设置页预览卡片。
pub fn preview(c: ThemeChoice) -> (&'static str, u32, u32, u32) {
    let (name, p) = match c {
        ThemeChoice::System => return ("跟随系统", 0xE8E8EA, 0x333333, 0x888888),
        ThemeChoice::PaperLight => ("纸感浅色", &PAPER_LIGHT),
        ThemeChoice::PaperDark => ("纸感深色", &PAPER_DARK),
        ThemeChoice::OneLight => ("One 浅色", &ONE_LIGHT),
        ThemeChoice::OneDark => ("One 深色", &ONE_DARK),
        ThemeChoice::ClaudeLight => ("Claude 浅色", &CLAUDE_LIGHT),
        ThemeChoice::ClaudeDark => ("Claude 深色", &CLAUDE_DARK),
        ThemeChoice::Nord => ("Nord", &NORD),
        ThemeChoice::SolarizedLight => ("Solarized 浅色", &SOLARIZED_LIGHT),
        ThemeChoice::SolarizedDark => ("Solarized 深色", &SOLARIZED_DARK),
        ThemeChoice::Dracula => ("Dracula", &DRACULA),
        ThemeChoice::GruvboxDark => ("Gruvbox 深色", &GRUVBOX_DARK),
    };
    (name, p.background, p.foreground, p.primary)
}

/// 主题是否为深色(`System` 返回 `None`,取决于系统)。
pub fn is_dark_theme(c: ThemeChoice) -> Option<bool> {
    use ThemeChoice as C;
    match c {
        C::System => None,
        C::PaperLight | C::OneLight | C::ClaudeLight | C::SolarizedLight => Some(false),
        _ => Some(true),
    }
}

fn hex(v: u32) -> Hsla {
    rgb(v).into()
}

/// 用户自选的强调色(设置 → 外观);`None` 用主题默认色。
#[derive(Clone, Copy, Default)]
pub struct Accent(pub Option<u32>);
impl Global for Accent {}

/// 解析 `RRGGBB`(可带 `#`)。
pub fn parse_hex(text: &str) -> Option<u32> {
    let t = text.trim().trim_start_matches('#');
    (t.len() == 6).then(|| u32::from_str_radix(t, 16).ok()).flatten()
}

/// 把颜色 `a` 向 `b` 混合 `t`(0–1),在 RGB 空间线性插值,保持 `a` 的透明度。
pub fn mix(a: Hsla, b: Hsla, t: f32) -> Hsla {
    let (x, y): (Rgba, Rgba) = (a.into(), b.into());
    let l = |p: f32, q: f32| p + (q - p) * t;
    Rgba { r: l(x.r, y.r), g: l(x.g, y.g), b: l(x.b, y.b), a: x.a }.into()
}

/// 相对亮度(0–1),用来决定主色上的文字用黑还是白。
fn luma(c: Hsla) -> f32 {
    let r: Rgba = c.into();
    0.2126 * r.r + 0.7152 * r.g + 0.0722 * r.b
}

/// 让强调色适配明暗:深色主题提亮到不低于 0.62,浅色主题压到不高于 0.48,保证与背景的对比度。
pub fn adapt_accent(base: Hsla, dark: bool) -> Hsla {
    let mut c = base;
    c.l = if dark { c.l.max(0.62) } else { c.l.min(0.48) };
    c
}

/// 应用主题到全局(含强调色),并让 Dock 图标切到对应的浅色 / 深色版。
pub fn apply(kind: ThemeKind, window: Option<&mut Window>, cx: &mut App) {
    apply_colors(kind, window, cx);
    let (dark, bg) = (cx.theme().is_dark(), cx.theme().background);
    if let Some(c) = cx.try_global::<Accent>().and_then(|a| a.0) {
        let base = adapt_accent(hex(c), dark);
        let step = |c: Hsla, v: f32| if dark { c.lighten(v) } else { c.darken(v) };
        let fg = if luma(base) > 0.6 { hex(0x1A1A1A) } else { hex(0xFFFFFF) };
        Theme::update(cx, |t| {
            t.primary = base;
            t.primary_hover = step(base, 0.08);
            t.primary_active = step(base, 0.16);
            t.primary_foreground = fg;
            t.button_primary = base;
            t.button_primary_hover = step(base, 0.08);
            t.button_primary_active = step(base, 0.16);
            t.button_primary_foreground = fg;
            t.link = step(base, 0.04);
        });
    }
    derive_from_primary(dark, bg, cx);
    crate::md_theme::install(cx);
    crate::app_icon::set_dock_icon(cx.theme().is_dark());
}

/// 从最终主色派生悬停 / 选中 / 焦点 / 选区等,让整套界面随主题与强调色一起变。
fn derive_from_primary(dark: bool, bg: Hsla, cx: &mut App) {
    let k = if dark { 1.3 } else { 1.0 };
    Theme::update(cx, |t| {
        let p = t.primary;
        t.accent = mix(bg, p, 0.08 * k);
        t.list_hover = mix(bg, p, 0.06 * k);
        t.list_active = mix(bg, p, 0.13 * k);
        t.secondary_hover = mix(bg, p, 0.12 * k);
        t.sidebar_accent = mix(t.sidebar, p, 0.10 * k);
        t.list_active_border = p;
        t.ring = p;
        t.caret = p;
        t.selection = p.opacity(if dark { 0.32 } else { 0.24 });
        t.drop_target = p.opacity(0.2);
        t.link_hover = t.link;
        t.link_active = t.link;
    });
}

/// 把语法配色编成 Zed 格式的高亮主题(gpui-component 直接按 JSON 读取)。
fn highlight_theme(name: &str, dark: bool, s: &Syntax) -> Arc<HighlightTheme> {
    let c = |v: u32, style: Option<&str>| {
        let mut o = serde_json::json!({ "color": format!("#{v:06X}") });
        if let Some(st) = style {
            o["font_style"] = st.into();
        }
        o
    };
    let json = serde_json::json!({
        "name": name,
        "appearance": if dark { "dark" } else { "light" },
        "style": {
            "syntax": {
                "comment": c(s.comment, Some("italic")), "comment.doc": c(s.comment, Some("italic")),
                "keyword": c(s.keyword, None), "preproc": c(s.keyword, None), "label": c(s.keyword, None),
                "string": c(s.string, None), "string.escape": c(s.number, None), "string.regex": c(s.number, None),
                "string.special": c(s.number, None), "string.special.symbol": c(s.number, None), "text.literal": c(s.string, None),
                "number": c(s.number, None), "boolean": c(s.number, None), "constant": c(s.constant, None),
                "function": c(s.function, None), "constructor": c(s.function, None),
                "type": c(s.type_, None), "enum": c(s.type_, None), "variant": c(s.type_, None),
                "property": c(s.property, None), "attribute": c(s.property, None),
                "tag": c(s.tag, None), "tag.doctype": c(s.comment, None),
                "operator": c(s.operator, None),
                "punctuation": c(s.punctuation, None), "punctuation.bracket": c(s.punctuation, None),
                "punctuation.delimiter": c(s.punctuation, None), "punctuation.special": c(s.operator, None),
                "punctuation.list_marker": c(s.keyword, None),
                "variable": c(s.variable, None), "variable.special": c(s.constant, None), "primary": c(s.variable, None),
                "title": c(s.keyword, None), "link_text": c(s.function, None), "link_uri": c(s.string, None),
                "text.code.span": c(s.string, None), "embedded": c(s.variable, None),
                "emphasis": c(s.variable, Some("italic")), "emphasis.strong": c(s.variable, None),
                "hint": c(s.comment, None), "predictive": c(s.comment, None)
            }
        }
    });
    serde_json::from_value::<HighlightTheme>(json).map(Arc::new).unwrap_or_else(|_| if dark { HighlightTheme::default_dark() } else { HighlightTheme::default_light() })
}

/// 把配色写进全局主题。
fn paint(dark: bool, p: &Palette, syn: &Syntax, name: &str, window: Option<&mut Window>, cx: &mut App) {
    Theme::change(if dark { ThemeMode::Dark } else { ThemeMode::Light }, window, cx);
    let hl = highlight_theme(name, dark, syn);
    Theme::update(cx, |t| {
        t.background = hex(p.background);
        t.foreground = hex(p.foreground);
        t.muted = hex(p.muted);
        t.muted_foreground = hex(p.muted_foreground);
        t.border = hex(p.border);
        t.sidebar = hex(p.sidebar);
        t.sidebar_foreground = hex(p.foreground);
        t.sidebar_border = hex(p.sidebar_border);
        t.sidebar_accent = hex(p.sidebar_accent);
        t.sidebar_accent_foreground = hex(p.foreground);
        t.popover = hex(p.popover);
        t.popover_foreground = hex(p.foreground);
        t.accent = hex(p.accent);
        t.accent_foreground = hex(p.foreground);
        t.list_hover = hex(p.list_hover);
        t.list_active = hex(p.list_active);
        t.secondary = hex(p.muted);
        t.secondary_hover = hex(p.secondary_hover);
        t.primary = hex(p.primary);
        t.primary_hover = hex(p.primary_hover);
        t.primary_active = hex(p.primary_active);
        t.primary_foreground = hex(p.primary_foreground);
        t.button_primary = hex(p.primary);
        t.button_primary_hover = hex(p.primary_hover);
        t.button_primary_active = hex(p.primary_active);
        t.button_primary_foreground = hex(p.primary_foreground);
        t.link = hex(p.link);
        t.success = hex(p.success);
        t.warning = hex(p.warning);
        t.danger = hex(p.danger);
        t.info = hex(p.info);
        t.highlight_theme = hl;
    });
}

/// 一个主题的规格:(是否深色, 界面配色, 语法配色, 高亮主题名)。
fn spec(kind: ThemeChoice) -> (bool, &'static Palette, &'static Syntax, &'static str) {
    use ThemeChoice as C;
    match kind {
        C::PaperLight | C::System => (false, &PAPER_LIGHT, &SYNTAX_PAPER_LIGHT, "Paper Light"),
        C::PaperDark => (true, &PAPER_DARK, &SYNTAX_PAPER_DARK, "Paper Dark"),
        C::OneLight => (false, &ONE_LIGHT, &SYNTAX_ONE_LIGHT, "One Light"),
        C::OneDark => (true, &ONE_DARK, &SYNTAX_ONE_DARK, "One Dark"),
        C::ClaudeLight => (false, &CLAUDE_LIGHT, &SYNTAX_CLAUDE_LIGHT, "Claude Light"),
        C::ClaudeDark => (true, &CLAUDE_DARK, &SYNTAX_CLAUDE_DARK, "Claude Dark"),
        C::Nord => (true, &NORD, &SYNTAX_NORD, "Nord"),
        C::SolarizedLight => (false, &SOLARIZED_LIGHT, &SYNTAX_SOLARIZED_LIGHT, "Solarized Light"),
        C::SolarizedDark => (true, &SOLARIZED_DARK, &SYNTAX_SOLARIZED_DARK, "Solarized Dark"),
        C::Dracula => (true, &DRACULA, &SYNTAX_DRACULA, "Dracula"),
        C::GruvboxDark => (true, &GRUVBOX_DARK, &SYNTAX_GRUVBOX_DARK, "Gruvbox Dark"),
    }
}

/// 覆盖主题色值。参数:`kind` 主题;`window` 当前窗口(可选);`cx` 应用上下文。
fn apply_colors(kind: ThemeKind, window: Option<&mut Window>, cx: &mut App) {
    if kind == ThemeChoice::System {
        // 先让库同步系统明暗,再按结果套纸感配色
        Theme::sync_system_appearance(window, cx);
        let pick = if cx.theme().is_dark() { ThemeChoice::PaperDark } else { ThemeChoice::PaperLight };
        let (dark, p, syn, name) = spec(pick);
        paint(dark, p, syn, name, None, cx);
    } else {
        let (dark, p, syn, name) = spec(kind);
        paint(dark, p, syn, name, window, cx);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 全部主题的语法主题能解析() {
        for c in ThemeChoice::ALL.into_iter().filter(|c| *c != ThemeChoice::System) {
            let (dark, _, s, _) = spec(c);
            let t = highlight_theme("t", dark, s);
            assert!(t.keyword.is_some(), "{c:?} 解析失败");
            assert_eq!(t.name, "t");
        }
    }

    #[::core::prelude::v1::test]
    fn 十六进制解析() {
        assert_eq!(parse_hex("#ff6a1f"), Some(0xFF6A1F));
        assert_eq!(parse_hex("xyz"), None);
    }

    #[::core::prelude::v1::test]
    fn 强调色按明暗调整明度() {
        let dark_blue = adapt_accent(hex(0x1D4ED8), true);
        assert!(dark_blue.l >= 0.62);
        let light_yellow = adapt_accent(hex(0xFACC15), false);
        assert!(light_yellow.l <= 0.48);
    }

    #[::core::prelude::v1::test]
    fn 混合端点正确() {
        let (a, b) = (hex(0x000000), hex(0xFFFFFF));
        assert!((luma(mix(a, b, 0.)) - 0.).abs() < 0.01);
        assert!((luma(mix(a, b, 1.)) - 1.).abs() < 0.01);
    }

    #[::core::prelude::v1::test]
    fn 主题标识互相可逆() {
        for c in ThemeChoice::ALL {
            assert_eq!(ThemeChoice::from_key(c.key()), Some(c));
        }
    }
}
