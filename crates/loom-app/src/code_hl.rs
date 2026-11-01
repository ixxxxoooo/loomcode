//! 代码文本的语法高亮:把一段文本按语言算出各段样式,供 `StyledText::with_highlights` 使用。
//!
//! @author ixxxxoooo

use gpui_kit::component::highlighter::SyntaxHighlighter;
use gpui_kit::component::{ActiveTheme, Rope};
use gpui_kit::*;
use std::ops::Range;

/// 高亮文本过大时不再着色,避免卡顿。
const MAX_BYTES: usize = 200_000;

/// 计算语法高亮样式。
///
/// 参数:`text` 文本;`lang` 语言名(`text` 或空表示不着色);`cx` 取语法主题。
/// 返回:按位置排序的 (字节区间, 样式);不支持的语言或文本过大返回空。
pub fn styles(text: &str, lang: &str, cx: &App) -> Vec<(Range<usize>, HighlightStyle)> {
    if lang.is_empty() || lang == "text" || text.is_empty() || text.len() > MAX_BYTES {
        return Vec::new();
    }
    let mut hl = SyntaxHighlighter::new(lang);
    hl.update(None, &Rope::from(text), Some(std::time::Duration::from_millis(80)));
    hl.styles(&(0..text.len()), &*cx.theme().highlight_theme)
}
