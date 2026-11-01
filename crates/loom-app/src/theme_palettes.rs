//! 各主题的界面配色(语义 token)与语法高亮配色。纯数据。
//!
//! @author ixxxxoooo

/// 一套界面配色(语义 token)。
pub struct Palette {
    pub background: u32,
    pub foreground: u32,
    pub muted: u32,
    pub muted_foreground: u32,
    pub border: u32,
    pub sidebar: u32,
    pub sidebar_border: u32,
    pub sidebar_accent: u32,
    pub popover: u32,
    pub accent: u32,
    pub list_hover: u32,
    pub list_active: u32,
    pub secondary_hover: u32,
    pub primary: u32,
    pub primary_hover: u32,
    pub primary_active: u32,
    pub primary_foreground: u32,
    pub link: u32,
    pub success: u32,
    pub warning: u32,
    pub danger: u32,
    pub info: u32,
}

/// 语法高亮配色。
pub struct Syntax {
    pub comment: u32,
    pub keyword: u32,
    pub string: u32,
    pub number: u32,
    pub function: u32,
    pub type_: u32,
    pub constant: u32,
    pub property: u32,
    pub tag: u32,
    pub operator: u32,
    pub punctuation: u32,
    pub variable: u32,
}

pub const PAPER_LIGHT: Palette = Palette {
    background: 0xF6F6F7, foreground: 0x26251E, muted: 0xECECEE, muted_foreground: 0x77756C, border: 0xE0DFDA,
    sidebar: 0xEFEFEF, sidebar_border: 0xE0DFDA, sidebar_accent: 0xE4E4E6, popover: 0xFFFFFF, accent: 0xE8E8EA,
    list_hover: 0xEAEAEC, list_active: 0xE0E0E3, secondary_hover: 0xE4E4E6,
    primary: 0xF54E00, primary_hover: 0xD94500, primary_active: 0xBF3E00, primary_foreground: 0xFFFFFF, link: 0xC2410C,
    success: 0x2F7D4F, warning: 0xB7791F, danger: 0xC53030, info: 0x2B6CB0,
};
pub const PAPER_DARK: Palette = Palette {
    background: 0x1D1C19, foreground: 0xE8E6DF, muted: 0x272622, muted_foreground: 0x9C998E, border: 0x35342F,
    sidebar: 0x161512, sidebar_border: 0x2C2B27, sidebar_accent: 0x24231F, popover: 0x272622, accent: 0x2B2A26,
    list_hover: 0x2A2924, list_active: 0x34332E, secondary_hover: 0x34332E,
    primary: 0xFF6A1F, primary_hover: 0xFF7F3F, primary_active: 0xE55A14, primary_foreground: 0x1D1C19, link: 0xFF9A5C,
    success: 0x5CC389, warning: 0xE0B04A, danger: 0xF07070, info: 0x6FA8E0,
};
pub const ONE_LIGHT: Palette = Palette {
    background: 0xFAFAFA, foreground: 0x242529, muted: 0xEBEBEC, muted_foreground: 0x6A6B70, border: 0xDCDCDE,
    sidebar: 0xF1F1F2, sidebar_border: 0xDCDCDE, sidebar_accent: 0xE5E5E7, popover: 0xFFFFFF, accent: 0xE8E8EA,
    list_hover: 0xEBEBED, list_active: 0xE2E2E5, secondary_hover: 0xE2E2E5,
    primary: 0x5C78E2, primary_hover: 0x4A66D0, primary_active: 0x3C55B8, primary_foreground: 0xFFFFFF, link: 0x4A66D0,
    success: 0x3F9E5F, warning: 0xC18401, danger: 0xD3604F, info: 0x3882B7,
};
pub const ONE_DARK: Palette = Palette {
    background: 0x282C34, foreground: 0xDCE0E5, muted: 0x2F343E, muted_foreground: 0xA9AFBC, border: 0x3A404C,
    sidebar: 0x21252B, sidebar_border: 0x363C46, sidebar_accent: 0x2C313A, popover: 0x2F343E, accent: 0x363C46,
    list_hover: 0x2C313A, list_active: 0x363C46, secondary_hover: 0x3E4452,
    primary: 0x74ADE8, primary_hover: 0x8DBCEE, primary_active: 0x5C9AD8, primary_foreground: 0x1D2026, link: 0x8DBCEE,
    success: 0xA1C181, warning: 0xDFC184, danger: 0xD07277, info: 0x73ADE9,
};
/// Claude 风格浅色:奶油底、暖灰文字、赤陶橙强调。
pub const CLAUDE_LIGHT: Palette = Palette {
    background: 0xFAF9F5, foreground: 0x1F1E1D, muted: 0xF0EEE6, muted_foreground: 0x73726C, border: 0xE8E6DC,
    sidebar: 0xF5F4ED, sidebar_border: 0xE8E6DC, sidebar_accent: 0xEAE8DF, popover: 0xFFFFFF, accent: 0xEFEDE4,
    list_hover: 0xEFEDE4, list_active: 0xE8E5DA, secondary_hover: 0xE8E5DA,
    primary: 0xC15F3C, primary_hover: 0xAE5630, primary_active: 0x9B4B2A, primary_foreground: 0xFFFFFF, link: 0xAE5630,
    success: 0x3F7D4F, warning: 0xB7791F, danger: 0xBF3B2E, info: 0x3B6EA5,
};
/// Claude 风格深色:暖炭灰底、奶油文字、珊瑚橙强调。
pub const CLAUDE_DARK: Palette = Palette {
    background: 0x262624, foreground: 0xF0EEE6, muted: 0x30302E, muted_foreground: 0xB0AEA5, border: 0x3E3E3A,
    sidebar: 0x1F1E1D, sidebar_border: 0x33322F, sidebar_accent: 0x2B2A28, popover: 0x30302E, accent: 0x353532,
    list_hover: 0x2F2E2C, list_active: 0x3A3936, secondary_hover: 0x3A3936,
    primary: 0xD97757, primary_hover: 0xE58A6C, primary_active: 0xC4623F, primary_foreground: 0x1F1E1D, link: 0xE58A6C,
    success: 0x7DBE8F, warning: 0xE0B04A, danger: 0xEF7B6C, info: 0x7FA8D9,
};
pub const NORD: Palette = Palette {
    background: 0x2E3440, foreground: 0xD8DEE9, muted: 0x3B4252, muted_foreground: 0x9AA5B8, border: 0x434C5E,
    sidebar: 0x272C36, sidebar_border: 0x3B4252, sidebar_accent: 0x343B49, popover: 0x3B4252, accent: 0x3E4658,
    list_hover: 0x353C4A, list_active: 0x434C5E, secondary_hover: 0x434C5E,
    primary: 0x88C0D0, primary_hover: 0x9ACBDA, primary_active: 0x6FAFC2, primary_foreground: 0x2E3440, link: 0x88C0D0,
    success: 0xA3BE8C, warning: 0xEBCB8B, danger: 0xBF616A, info: 0x81A1C1,
};
pub const SOLARIZED_LIGHT: Palette = Palette {
    background: 0xFDF6E3, foreground: 0x586E75, muted: 0xEEE8D5, muted_foreground: 0x6F8085, border: 0xDDD6C1,
    sidebar: 0xF5EFDC, sidebar_border: 0xDDD6C1, sidebar_accent: 0xEAE3CE, popover: 0xFFFBEB, accent: 0xEEE8D5,
    list_hover: 0xEEE8D5, list_active: 0xE6DFC8, secondary_hover: 0xE6DFC8,
    primary: 0x1F7FC4, primary_hover: 0x1A6DAA, primary_active: 0x155B8F, primary_foreground: 0xFFFFFF, link: 0x1A6DAA,
    success: 0x6F8A00, warning: 0xB58900, danger: 0xDC322F, info: 0x2AA198,
};
pub const SOLARIZED_DARK: Palette = Palette {
    background: 0x002B36, foreground: 0x93A1A1, muted: 0x073642, muted_foreground: 0x7F9396, border: 0x12414D,
    sidebar: 0x00222B, sidebar_border: 0x0E3A46, sidebar_accent: 0x0A323D, popover: 0x073642, accent: 0x0C3C48,
    list_hover: 0x0A343F, list_active: 0x12414D, secondary_hover: 0x12414D,
    primary: 0x268BD2, primary_hover: 0x3D9BDD, primary_active: 0x1F78B4, primary_foreground: 0x002B36, link: 0x4AA3E0,
    success: 0x859900, warning: 0xB58900, danger: 0xDC322F, info: 0x2AA198,
};
pub const DRACULA: Palette = Palette {
    background: 0x282A36, foreground: 0xF8F8F2, muted: 0x303341, muted_foreground: 0xA3A8C3, border: 0x44475A,
    sidebar: 0x21222C, sidebar_border: 0x343746, sidebar_accent: 0x2D2F3D, popover: 0x303341, accent: 0x3A3D4F,
    list_hover: 0x2F3240, list_active: 0x44475A, secondary_hover: 0x44475A,
    primary: 0xBD93F9, primary_hover: 0xCCA8FB, primary_active: 0xA87BF0, primary_foreground: 0x21222C, link: 0xCCA8FB,
    success: 0x50FA7B, warning: 0xFFB86C, danger: 0xFF5555, info: 0x8BE9FD,
};
pub const GRUVBOX_DARK: Palette = Palette {
    background: 0x282828, foreground: 0xEBDBB2, muted: 0x32302F, muted_foreground: 0xA89984, border: 0x45403D,
    sidebar: 0x1D2021, sidebar_border: 0x3C3836, sidebar_accent: 0x2A2827, popover: 0x32302F, accent: 0x3C3836,
    list_hover: 0x32302F, list_active: 0x3C3836, secondary_hover: 0x45403D,
    primary: 0xFE8019, primary_hover: 0xFF9433, primary_active: 0xD96A0D, primary_foreground: 0x1D2021, link: 0xFABD2F,
    success: 0xB8BB26, warning: 0xFABD2F, danger: 0xFB4934, info: 0x83A598,
};

pub const SYNTAX_PAPER_LIGHT: Syntax = Syntax {
    comment: 0x9A978C, keyword: 0xB4470F, string: 0x4D7C3A, number: 0x9A6A00, function: 0x2F6DA3, type_: 0x8A4F9E,
    constant: 0x9A6A00, property: 0x4F6A99, tag: 0xB4470F, operator: 0x6B685E, punctuation: 0x6B685E, variable: 0x26251E,
};
pub const SYNTAX_PAPER_DARK: Syntax = Syntax {
    comment: 0x7D7A70, keyword: 0xFF8A4C, string: 0x8FBF7F, number: 0xE0B04A, function: 0x7FB2E0, type_: 0xC79AE0,
    constant: 0xE0B04A, property: 0x9DB4D6, tag: 0xFF8A4C, operator: 0xA8A59A, punctuation: 0xA8A59A, variable: 0xE8E6DF,
};
pub const SYNTAX_ONE_LIGHT: Syntax = Syntax {
    comment: 0xA2A3A7, keyword: 0xA449AB, string: 0x50A14F, number: 0xAD6E25, function: 0x5B79E3, type_: 0x3882B7,
    constant: 0xC18401, property: 0xD3604F, tag: 0x5B79E3, operator: 0x3882B7, punctuation: 0x4D4F52, variable: 0x242529,
};
pub const SYNTAX_ONE_DARK: Syntax = Syntax {
    comment: 0x5D636F, keyword: 0xB477CF, string: 0xA1C181, number: 0xBF956A, function: 0x73ADE9, type_: 0x6EB4BF,
    constant: 0xDFC184, property: 0xD07277, tag: 0x74ADE8, operator: 0x6EB4BF, punctuation: 0xACB2BE, variable: 0xDCE0E5,
};
pub const SYNTAX_CLAUDE_LIGHT: Syntax = Syntax {
    comment: 0x9A978C, keyword: 0xAE5630, string: 0x5E7F3A, number: 0x9A6A00, function: 0x3B6EA5, type_: 0x7A5A9E,
    constant: 0x9A6A00, property: 0x5A6F8F, tag: 0xAE5630, operator: 0x73726C, punctuation: 0x73726C, variable: 0x1F1E1D,
};
pub const SYNTAX_CLAUDE_DARK: Syntax = Syntax {
    comment: 0x7D7B72, keyword: 0xE58A6C, string: 0x9CC184, number: 0xE0B04A, function: 0x7FA8D9, type_: 0xC3A0DE,
    constant: 0xE0B04A, property: 0xA3B7D6, tag: 0xE58A6C, operator: 0xB0AEA5, punctuation: 0xB0AEA5, variable: 0xF0EEE6,
};
pub const SYNTAX_NORD: Syntax = Syntax {
    comment: 0x616E88, keyword: 0x81A1C1, string: 0xA3BE8C, number: 0xB48EAD, function: 0x88C0D0, type_: 0x8FBCBB,
    constant: 0xB48EAD, property: 0xD08770, tag: 0x81A1C1, operator: 0x81A1C1, punctuation: 0x9AA5B8, variable: 0xD8DEE9,
};
pub const SYNTAX_SOLARIZED_LIGHT: Syntax = Syntax {
    comment: 0x93A1A1, keyword: 0x859900, string: 0x2AA198, number: 0xD33682, function: 0x268BD2, type_: 0xB58900,
    constant: 0xCB4B16, property: 0x6C71C4, tag: 0x268BD2, operator: 0x859900, punctuation: 0x657B83, variable: 0x586E75,
};
pub const SYNTAX_SOLARIZED_DARK: Syntax = Syntax {
    comment: 0x586E75, keyword: 0x859900, string: 0x2AA198, number: 0xD33682, function: 0x268BD2, type_: 0xB58900,
    constant: 0xCB4B16, property: 0x6C71C4, tag: 0x268BD2, operator: 0x859900, punctuation: 0x93A1A1, variable: 0x93A1A1,
};
pub const SYNTAX_DRACULA: Syntax = Syntax {
    comment: 0x6272A4, keyword: 0xFF79C6, string: 0xF1FA8C, number: 0xBD93F9, function: 0x50FA7B, type_: 0x8BE9FD,
    constant: 0xBD93F9, property: 0xFFB86C, tag: 0xFF79C6, operator: 0xFF79C6, punctuation: 0xF8F8F2, variable: 0xF8F8F2,
};
pub const SYNTAX_GRUVBOX_DARK: Syntax = Syntax {
    comment: 0x928374, keyword: 0xFB4934, string: 0xB8BB26, number: 0xD3869B, function: 0xFABD2F, type_: 0x83A598,
    constant: 0xD3869B, property: 0x83A598, tag: 0x8EC07C, operator: 0x8EC07C, punctuation: 0xA89984, variable: 0xEBDBB2,
};
