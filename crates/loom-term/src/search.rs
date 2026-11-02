//! 终端内搜索:在回看历史与当前画面里找文本。
//!
//! @author ixxxxoooo

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::{Term, cell::Flags};

/// 一处匹配。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchMatch {
    /// 网格行号:0 起是当前画面,负数是回看历史
    pub line: i32,
    /// 起始列
    pub col: usize,
    /// 占几格
    pub cells: usize,
}

/// 匹配数量上限,防止在超大回看里卡住界面。
const MAX_MATCHES: usize = 5000;

/// 在整个网格(含历史)里找 `query`,按从上到下、从左到右返回。
///
/// 参数:`case_sensitive` 为假时忽略大小写;空查询返回空。
pub(crate) fn find<T: EventListener>(term: &Term<T>, query: &str, case_sensitive: bool) -> Vec<SearchMatch> {
    let fold = |c: char| if case_sensitive { c } else { c.to_lowercase().next().unwrap_or(c) };
    let needle: Vec<char> = query.chars().map(fold).collect();
    if needle.is_empty() {
        return Vec::new();
    }
    let (top, bottom, cols) = (term.topmost_line().0, term.bottommost_line().0, term.columns());
    let mut out = Vec::new();
    for l in top..=bottom {
        // 一行里的字符与它们所在的列、占格数(宽字符的占位格跳过)
        let mut chars: Vec<(char, usize, usize)> = Vec::new();
        for c in 0..cols {
            let cell = &term.grid()[Line(l)][Column(c)];
            if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                continue;
            }
            chars.push((fold(if cell.c == '\0' { ' ' } else { cell.c }), c, if cell.flags.contains(Flags::WIDE_CHAR) { 2 } else { 1 }));
        }
        let mut i = 0;
        while i + needle.len() <= chars.len() {
            if chars[i..i + needle.len()].iter().map(|x| x.0).eq(needle.iter().copied()) {
                let last = &chars[i + needle.len() - 1];
                out.push(SearchMatch { line: l, col: chars[i].1, cells: last.1 + last.2 - chars[i].1 });
                if out.len() >= MAX_MATCHES {
                    return out;
                }
                i += needle.len();
            } else {
                i += 1;
            }
        }
    }
    out
}
