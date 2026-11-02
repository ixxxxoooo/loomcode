//! 当前缩进块的高亮参考线(对标 VS Code 的 active indent guide):
//! 编辑器库自带淡色缩进线,这里在光标所在块上叠一条更亮的竖线,一眼看出当前代码块的起止。
//!
//! 块的判定只看缩进(与语言无关):光标在块头行(下一行缩进更深)时取它的子块,否则取光标行自己所在的块;
//! 竖线画在块缩进的上一级列上,从块头的下一行到块尾的上一行(头尾两行不在块内)。
//!
//! @author ixxxxoooo

use crate::ide::IdeDoc;
use gpui_kit::component::input::TabSize;
use gpui_kit::component::{ActiveTheme, RopeExt as _};
use gpui_kit::*;

/// 单向最多扫描的行数,超长文件里块很大时也不拖慢渲染。
const MAX_SCAN: usize = 2000;

/// 一条高亮线:画在第 `col` 列,覆盖 `start..=end` 行(缓冲区行号)。
#[derive(Debug, PartialEq, Eq)]
pub struct Guide {
    pub col: usize,
    pub start: usize,
    pub end: usize,
}

/// 求光标所在块的高亮线。
///
/// 参数:`lines` 总行数;`cursor` 光标行;`tab` 缩进宽度;`indent` 返回某行的缩进列数(空白行为 `None`)。
/// 返回:没有可高亮的块(光标在顶层、文件没有缩进)时为 `None`。
pub fn active_guide(lines: usize, cursor: usize, tab: usize, indent: impl Fn(usize) -> Option<usize>) -> Option<Guide> {
    let tab = tab.max(1);
    if cursor >= lines {
        return None;
    }
    // 向上 / 向下找最近的非空白行
    let neighbor = |from: usize, up: bool| -> Option<(usize, usize)> {
        (1..=MAX_SCAN).map_while(|d| if up { from.checked_sub(d) } else { Some(from + d).filter(|&l| l < lines) }).find_map(|l| indent(l).map(|i| (l, i)))
    };
    let own = indent(cursor);
    let next = neighbor(cursor, false);
    let ind = match own {
        // 块头行:取子块的缩进
        Some(i) => next.map_or(i, |(_, n)| if n > i { n } else { i }),
        // 空白行:夹在两段代码之间时取较浅的一边
        None => match (neighbor(cursor, true), next) {
            (Some((_, a)), Some((_, b))) => a.min(b),
            (Some((_, a)), None) | (None, Some((_, a))) => a,
            (None, None) => return None,
        },
    };
    if ind == 0 {
        return None;
    }
    let col = (ind - 1) / tab * tab;
    // 起点:光标行本身在块内就从它出发,否则(块头行)从子块的第一行出发
    let anchor = match own {
        Some(i) if i <= col => next?.0,
        _ => cursor,
    };
    let (mut start, mut end) = (anchor, anchor);
    for d in 1..=MAX_SCAN {
        let Some(l) = anchor.checked_sub(d) else { break };
        match indent(l) {
            Some(i) if i > col => start = l,
            Some(_) => break,
            None => {}
        }
    }
    for d in 1..=MAX_SCAN {
        let l = anchor + d;
        if l >= lines {
            break;
        }
        match indent(l) {
            Some(i) if i > col => end = l,
            Some(_) => break,
            None => {}
        }
    }
    Some(Guide { col, start, end })
}

/// 光标所在块的高亮线元素。光标行不在视口、没有可高亮的块或横向滚动让该列移出可视区时返回 `None`。
pub fn overlay(doc: &IdeDoc, cx: &App) -> Option<AnyElement> {
    let tab = usize::from(crate::prefs(cx).editor.tab_size.clamp(1, 16));
    let ts = TabSize { tab_size: tab, hard_tabs: false };
    let ed = doc.editor.read(cx);
    let text = ed.text();
    let visible = ed.visible_row_range()?;
    let tb = ed.text_bounds()?;
    let cursor = text.offset_to_position(ed.cursor()).line as usize;
    let line_indent = |l: usize| {
        let line = text.slice_line(l);
        (!line.chars().all(char::is_whitespace)).then(|| ts.indent_count(&line))
    };
    let g = active_guide(text.lines_len(), cursor, tab, line_indent)?;
    let (s, e) = (g.start.max(visible.start), g.end.min(visible.end.saturating_sub(1)));
    if s > e {
        return None;
    }
    // 量 x 用块内一条真正缩进到该列的行
    let ref_line = (s..=e).find(|&l| line_indent(l).is_some_and(|i| i > g.col))?;
    let x = {
        let line = text.slice_line(ref_line);
        let (mut width, mut bytes) = (0, 0);
        for ch in line.chars() {
            if width >= g.col {
                break;
            }
            width += if ch == '\t' { tab } else { 1 };
            bytes += ch.len_utf8();
        }
        let at = text.line_start_offset(ref_line) + bytes;
        ed.range_to_bounds(&(at..at))?.origin.x
    };
    let top = ed.range_to_bounds(&{
        let o = text.line_start_offset(s);
        o..o
    })?;
    let bottom = ed.range_to_bounds(&{
        let o = text.line_end_offset(e);
        o..o
    })?;
    let (top_y, bottom_y) = (top.origin.y.max(tb.top()), (bottom.origin.y + bottom.size.height).min(tb.bottom()));
    if x < tb.left() || bottom_y <= top_y {
        return None;
    }
    let color = cx.theme().muted_foreground.opacity(0.7);
    Some(deferred(anchored().position(point(x - px(0.5), top_y)).anchor(Anchor::TopLeft).child(div().w(px(1.)).h(bottom_y - top_y).bg(color))).into_any_element())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 把代码文本按行转成缩进表(空白行为 None)。
    fn indents(src: &str) -> Vec<Option<usize>> {
        src.lines().map(|l| (!l.trim().is_empty()).then(|| l.len() - l.trim_start().len())).collect()
    }

    fn guide(src: &str, cursor: usize) -> Option<Guide> {
        let v = indents(src);
        active_guide(v.len(), cursor, 4, |l| v[l])
    }

    const PY: &str = "def f(item):\n    a = 1\n    return {\n        \"x\": 1,\n        \"y\": 2,\n    }\n\nclass A:\n    pass\n";

    #[::core::prelude::v1::test]
    fn 光标在块内取所在块() {
        // 光标在 "y" 行:块缩进 8,线画在第 4 列,覆盖两行字典项(头尾两行不含)
        assert_eq!(guide(PY, 4), Some(Guide { col: 4, start: 3, end: 4 }));
    }

    #[::core::prelude::v1::test]
    fn 光标在块头取子块() {
        // 光标在 `return {` 行:取它的子块
        assert_eq!(guide(PY, 2), Some(Guide { col: 4, start: 3, end: 4 }));
        // 光标在 def 行:取函数体(缩进 4,线在第 0 列),覆盖到闭括号那一行
        assert_eq!(guide(PY, 0), Some(Guide { col: 0, start: 1, end: 5 }));
    }

    #[::core::prelude::v1::test]
    fn 顶层行没有高亮线() {
        assert_eq!(guide("a = 1\nb = 2\n", 1), None);
        // 空白行夹在顶层代码之间
        assert_eq!(guide("a = 1\n\nb = 2\n", 1), None);
    }

    #[::core::prelude::v1::test]
    fn 块内空白行不断开也不拖长() {
        let src = "if x:\n    a = 1\n\n    b = 2\n\n\nc = 3\n";
        // 光标在 a 行:块跨过中间的空白行,末尾空白行不计入
        assert_eq!(guide(src, 1), Some(Guide { col: 0, start: 1, end: 3 }));
        // 光标在块内的空白行也能取到
        assert_eq!(guide(src, 2), Some(Guide { col: 0, start: 1, end: 3 }));
    }

    #[::core::prelude::v1::test]
    fn 缩进列按缩进宽度取整() {
        // 缩进 6(不规整):线画在第 4 列
        let src = "if x:\n    if y:\n      z = 1\n";
        assert_eq!(guide(src, 2), Some(Guide { col: 4, start: 2, end: 2 }));
    }
}
