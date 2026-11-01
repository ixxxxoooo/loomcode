//! 主题:在 gpui-component 的语义 token 之上覆盖出四套配色(纸感浅 / 深、One 浅 / 深)与对应的语法高亮。
//!
//! 约定:界面里的颜色只通过 `cx.theme()` 取,不在视图里写死色值;圆角用下面的三档令牌。

use gpui_kit::component::highlighter::HighlightTheme;
use gpui_kit::component::{ActiveTheme, Colorize as _, Theme, ThemeMode};
use gpui_kit::*;
use std::sync::Arc;

/// 设计令牌:圆角三档(小控件 / 输入与分段 / 卡片与面板),界面里的圆角统一从这里取。
pub const R_SM: f32 = 6.;
pub const R_MD: f32 = 8.;
pub const R_LG: f32 = 12.;

/// 内置主题。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeKind {
    /// 纸感浅色(默认)
    #[default]
    PaperLight,
    /// 纸感深色
    PaperDark,
    /// One 浅色(Zed One 风格的中性灰蓝)
    OneLight,
    /// One 深色
    OneDark,
    /// 跟随系统
    System,
}

impl From<loom_config::ThemeChoice> for ThemeKind {
    fn from(c: loom_config::ThemeChoice) -> Self {
        use loom_config::ThemeChoice as C;
        match c {
            C::PaperLight => Self::PaperLight,
            C::PaperDark => Self::PaperDark,
            C::OneLight => Self::OneLight,
            C::OneDark => Self::OneDark,
            C::System => Self::System,
        }
    }
}

/// 外观页的主题预览:(选项, 名称, 背景, 前景, 强调色)。
pub const PREVIEWS: [(loom_config::ThemeChoice, &str, u32, u32, u32); 5] = [
    (loom_config::ThemeChoice::PaperLight, "纸感浅色", 0xF6F6F7, 0x26251E, 0xF54E00),
    (loom_config::ThemeChoice::PaperDark, "纸感深色", 0x1D1C19, 0xE8E6DF, 0xFF6A1F),
    (loom_config::ThemeChoice::OneLight, "One 浅色", 0xFAFAFA, 0x242529, 0x5C78E2),
    (loom_config::ThemeChoice::OneDark, "One 深色", 0x282C34, 0xDCE0E5, 0x74ADE8),
    (loom_config::ThemeChoice::System, "跟随系统", 0xE8E8EA, 0x333333, 0x888888),
];

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

/// 应用主题到全局(含强调色),并让 Dock 图标切到对应的浅色 / 深色版。
pub fn apply(kind: ThemeKind, window: Option<&mut Window>, cx: &mut App) {
    apply_colors(kind, window, cx);
    if let Some(c) = cx.try_global::<Accent>().and_then(|a| a.0) {
        let base = hex(c);
        Theme::update(cx, |t| {
            t.primary = base;
            t.primary_hover = base.darken(0.08);
            t.primary_active = base.darken(0.16);
            t.button_primary = base;
            t.button_primary_hover = base.darken(0.08);
            t.button_primary_active = base.darken(0.16);
            t.link = base;
        });
    }
    crate::md_theme::install(cx);
    crate::app_icon::set_dock_icon(cx.theme().is_dark());
}

/// 一套界面配色(语义 token)。
struct Palette {
    background: u32,
    foreground: u32,
    muted: u32,
    muted_foreground: u32,
    border: u32,
    sidebar: u32,
    sidebar_border: u32,
    sidebar_accent: u32,
    popover: u32,
    accent: u32,
    list_hover: u32,
    list_active: u32,
    secondary_hover: u32,
    primary: u32,
    primary_hover: u32,
    primary_active: u32,
    primary_foreground: u32,
    link: u32,
    success: u32,
    warning: u32,
    danger: u32,
    info: u32,
}

/// 语法高亮配色。
struct Syntax {
    comment: u32,
    keyword: u32,
    string: u32,
    number: u32,
    function: u32,
    type_: u32,
    constant: u32,
    property: u32,
    tag: u32,
    operator: u32,
    punctuation: u32,
    variable: u32,
}

const PAPER_LIGHT: Palette = Palette {
    background: 0xF6F6F7, foreground: 0x26251E, muted: 0xECECEE, muted_foreground: 0x77756C, border: 0xE0DFDA,
    sidebar: 0xEFEFEF, sidebar_border: 0xE0DFDA, sidebar_accent: 0xE4E4E6, popover: 0xFFFFFF, accent: 0xE8E8EA,
    list_hover: 0xEAEAEC, list_active: 0xE0E0E3, secondary_hover: 0xE4E4E6,
    primary: 0xF54E00, primary_hover: 0xD94500, primary_active: 0xBF3E00, primary_foreground: 0xFFFFFF, link: 0xC2410C,
    success: 0x2F7D4F, warning: 0xB7791F, danger: 0xC53030, info: 0x2B6CB0,
};

const PAPER_DARK: Palette = Palette {
    background: 0x1D1C19, foreground: 0xE8E6DF, muted: 0x272622, muted_foreground: 0x9C998E, border: 0x35342F,
    sidebar: 0x161512, sidebar_border: 0x2C2B27, sidebar_accent: 0x24231F, popover: 0x272622, accent: 0x2B2A26,
    list_hover: 0x2A2924, list_active: 0x34332E, secondary_hover: 0x34332E,
    primary: 0xFF6A1F, primary_hover: 0xFF7F3F, primary_active: 0xE55A14, primary_foreground: 0x1D1C19, link: 0xFF9A5C,
    success: 0x5CC389, warning: 0xE0B04A, danger: 0xF07070, info: 0x6FA8E0,
};

const ONE_LIGHT: Palette = Palette {
    background: 0xFAFAFA, foreground: 0x242529, muted: 0xEBEBEC, muted_foreground: 0x6A6B70, border: 0xDCDCDE,
    sidebar: 0xF1F1F2, sidebar_border: 0xDCDCDE, sidebar_accent: 0xE5E5E7, popover: 0xFFFFFF, accent: 0xE8E8EA,
    list_hover: 0xEBEBED, list_active: 0xE2E2E5, secondary_hover: 0xE2E2E5,
    primary: 0x5C78E2, primary_hover: 0x4A66D0, primary_active: 0x3C55B8, primary_foreground: 0xFFFFFF, link: 0x4A66D0,
    success: 0x3F9E5F, warning: 0xC18401, danger: 0xD3604F, info: 0x3882B7,
};

const ONE_DARK: Palette = Palette {
    background: 0x282C34, foreground: 0xDCE0E5, muted: 0x2F343E, muted_foreground: 0xA9AFBC, border: 0x3A404C,
    sidebar: 0x21252B, sidebar_border: 0x363C46, sidebar_accent: 0x2C313A, popover: 0x2F343E, accent: 0x363C46,
    list_hover: 0x2C313A, list_active: 0x363C46, secondary_hover: 0x3E4452,
    primary: 0x74ADE8, primary_hover: 0x8DBCEE, primary_active: 0x5C9AD8, primary_foreground: 0x1D2026, link: 0x8DBCEE,
    success: 0xA1C181, warning: 0xDFC184, danger: 0xD07277, info: 0x73ADE9,
};

const SYNTAX_PAPER_LIGHT: Syntax = Syntax {
    comment: 0x9A978C, keyword: 0xB4470F, string: 0x4D7C3A, number: 0x9A6A00, function: 0x2F6DA3, type_: 0x8A4F9E,
    constant: 0x9A6A00, property: 0x4F6A99, tag: 0xB4470F, operator: 0x6B685E, punctuation: 0x6B685E, variable: 0x26251E,
};
const SYNTAX_PAPER_DARK: Syntax = Syntax {
    comment: 0x7D7A70, keyword: 0xFF8A4C, string: 0x8FBF7F, number: 0xE0B04A, function: 0x7FB2E0, type_: 0xC79AE0,
    constant: 0xE0B04A, property: 0x9DB4D6, tag: 0xFF8A4C, operator: 0xA8A59A, punctuation: 0xA8A59A, variable: 0xE8E6DF,
};
const SYNTAX_ONE_LIGHT: Syntax = Syntax {
    comment: 0xA2A3A7, keyword: 0xA449AB, string: 0x50A14F, number: 0xAD6E25, function: 0x5B79E3, type_: 0x3882B7,
    constant: 0xC18401, property: 0xD3604F, tag: 0x5B79E3, operator: 0x3882B7, punctuation: 0x4D4F52, variable: 0x242529,
};
const SYNTAX_ONE_DARK: Syntax = Syntax {
    comment: 0x5D636F, keyword: 0xB477CF, string: 0xA1C181, number: 0xBF956A, function: 0x73ADE9, type_: 0x6EB4BF,
    constant: 0xDFC184, property: 0xD07277, tag: 0x74ADE8, operator: 0x6EB4BF, punctuation: 0xACB2BE, variable: 0xDCE0E5,
};

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

/// 覆盖主题色值。参数:`kind` 主题;`window` 当前窗口(可选);`cx` 应用上下文。
fn apply_colors(kind: ThemeKind, window: Option<&mut Window>, cx: &mut App) {
    match kind {
        ThemeKind::System => {
            // 先让库同步系统明暗,再按结果套纸感配色
            Theme::sync_system_appearance(window, cx);
            let dark = cx.theme().is_dark();
            if dark {
                paint(true, &PAPER_DARK, &SYNTAX_PAPER_DARK, "Paper Dark", None, cx);
            } else {
                paint(false, &PAPER_LIGHT, &SYNTAX_PAPER_LIGHT, "Paper Light", None, cx);
            }
        }
        ThemeKind::PaperLight => paint(false, &PAPER_LIGHT, &SYNTAX_PAPER_LIGHT, "Paper Light", window, cx),
        ThemeKind::PaperDark => paint(true, &PAPER_DARK, &SYNTAX_PAPER_DARK, "Paper Dark", window, cx),
        ThemeKind::OneLight => paint(false, &ONE_LIGHT, &SYNTAX_ONE_LIGHT, "One Light", window, cx),
        ThemeKind::OneDark => paint(true, &ONE_DARK, &SYNTAX_ONE_DARK, "One Dark", window, cx),
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
    fn 语法主题能解析() {
        for (dark, s) in [(false, &SYNTAX_PAPER_LIGHT), (true, &SYNTAX_PAPER_DARK), (false, &SYNTAX_ONE_LIGHT), (true, &SYNTAX_ONE_DARK)] {
            let t = highlight_theme("t", dark, s);
            // 解析失败会退回默认主题,默认主题的关键字色与我们的不同
            let kw = t.keyword.as_ref().expect("有关键字样式");
            let _ = kw;
            assert_eq!(t.name, "t");
        }
    }

    #[::core::prelude::v1::test]
    fn 十六进制解析() {
        assert_eq!(parse_hex("#ff6a1f"), Some(0xFF6A1F));
        assert_eq!(parse_hex("xyz"), None);
    }
}
