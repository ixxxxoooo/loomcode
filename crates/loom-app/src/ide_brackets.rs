//! 括号配对高亮:光标紧挨括号时,给它和与之配对的括号画上边框。
//! 只做简单的深度计数(不识别字符串 / 注释里的括号),扫描范围有上限以免大文件卡顿。
//!
//! @author ixxxxoooo

use gpui_kit::component::input::{EditorState, RangeDecoration, RangeDecorationCollection, RangeDecorationStyle};
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;

/// 单次最多向前 / 向后扫描的字节数。
const MAX_SCAN: usize = 60_000;

/// 找出光标处括号的配对位置。
///
/// 参数:`text` 全文;`cursor` 光标字节偏移。优先看光标左侧的字符,再看右侧。
/// 返回:(括号位置, 配对位置) 的字节偏移。
pub fn find_match(text: &str, cursor: usize) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let candidates = [cursor.checked_sub(1), Some(cursor)];
    for at in candidates.into_iter().flatten() {
        let Some(&b) = bytes.get(at) else { continue };
        let (open, close, forward) = match b {
            b'(' => (b'(', b')', true),
            b'[' => (b'[', b']', true),
            b'{' => (b'{', b'}', true),
            b')' => (b'(', b')', false),
            b']' => (b'[', b']', false),
            b'}' => (b'{', b'}', false),
            _ => continue,
        };
        let mut depth = 0i32;
        if forward {
            for (i, &c) in bytes.iter().enumerate().skip(at).take(MAX_SCAN) {
                depth += i32::from(c == open) - i32::from(c == close);
                if depth == 0 {
                    return Some((at, i));
                }
            }
        } else {
            for i in (at.saturating_sub(MAX_SCAN)..=at).rev() {
                let c = bytes[i];
                depth += i32::from(c == close) - i32::from(c == open);
                if depth == 0 {
                    return Some((at, i));
                }
            }
        }
    }
    None
}

/// 按光标位置刷新括号高亮;`slot` 是该文档持有的装饰集合,`last` 记录上次的配对(没变就不重画)。
pub fn refresh(editor: &Entity<EditorState>, slot: &mut Option<RangeDecorationCollection>, last: &mut Option<(usize, usize)>, cx: &mut App) {
    let (text, cursor) = {
        let e = editor.read(cx);
        (e.value().to_string(), e.cursor())
    };
    let found = find_match(&text, cursor);
    if found == *last {
        return;
    }
    *last = found;
    let color = cx.theme().primary;
    let decos: Vec<RangeDecoration> = found
        .into_iter()
        .flat_map(|(a, b)| [a, b])
        .map(|i| RangeDecoration::new(i..i + 1).with_style(RangeDecorationStyle::Frame).with_color(color))
        .collect();
    match slot {
        Some(c) => c.set(decos, cx),
        None => *slot = Some(editor.update(cx, |e, cx| e.create_range_decorations_collection(decos, cx))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 向前与向后配对() {
        let t = "fn a(x: [i32; 2]) { y }";
        assert_eq!(find_match(t, 5), Some((4, 16)));
        assert_eq!(find_match(t, 17), Some((16, 4)));
        assert_eq!(find_match(t, 19), Some((18, 22)));
    }

    #[::core::prelude::v1::test]
    fn 嵌套与无配对() {
        assert_eq!(find_match("((a))", 1), Some((0, 4)));
        assert_eq!(find_match("(a", 1), None);
        assert_eq!(find_match("((a)", 1), Some((1, 3)));
        assert_eq!(find_match("abc", 1), None);
    }
}
