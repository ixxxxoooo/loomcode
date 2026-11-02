//! 括号着色(对标 Zed 的 `colorize_brackets` 与 VS Code 的 Bracket Pair Colorization)。
//!
//! 做法与业界一致:只改括号**字符本身**的字色,不铺底色。包在 tree-sitter 语法高亮外面(`RainbowHighlighter`),
//! 高亮器给出的样式段在括号处被切开,括号字符按嵌套深度循环取色;其余样式原样保留。
//! 括号位置由 `pair_scan::scan` 配对得到(跳过注释与字符串),按「编辑序号」缓存,滚动不重扫。
//!
//! 开关与深浅色存在线程局部状态里(界面线程独占),变化时由 `sync` 报告,调用方据此重装高亮工厂以重新排版。
//!
//! @author ixxxxoooo

use crate::pair_scan::{rules_for, scan, Rules, MAX_BYTES};
use gpui_kit::component::input::{EditorState, FoldRange, HighlightStyleResolver, InputEdit, InputHighlighter, InputHighlighterFactory};
use gpui_kit::*;
use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;

thread_local! {
    static ENABLED: Cell<bool> = const { Cell::new(true) };
    static DARK: Cell<bool> = const { Cell::new(true) };
}

/// 同步开关与深浅色;任一变化返回 `true`(调用方需要重装高亮工厂,让已排版的文字重新取色)。
///
/// 参数:`enabled` 是否着色;`dark` 当前是否深色主题。
pub fn sync(enabled: bool, dark: bool) -> bool {
    let changed = ENABLED.with(|c| c.replace(enabled)) != enabled || DARK.with(|c| c.replace(dark)) != dark;
    changed
}

/// 深度对应的括号字色:深色与浅色各一套,按深度循环(金 / 紫 / 蓝 / 青)。
///
/// 参数:`depth` 嵌套深度(最外层 0);`dark` 是否深色主题。返回:颜色。
pub fn depth_color(depth: usize, dark: bool) -> Hsla {
    let (dk, lt) = ([0xe5c07bu32, 0xc678dd, 0x61afef, 0x56b6c2], [0xa67c00u32, 0xa626a4, 0x0b6bcb, 0x0e7c86]);
    rgb(if dark { dk[depth % 4] } else { lt[depth % 4] }).into()
}

/// 把高亮样式段在括号处切开并给括号着色。
///
/// 参数:`base` 有序且不重叠的样式段;`tokens` 按位置排序的 (括号偏移, 深度);`dark` 深浅色。
/// 返回:新的样式段,仍然有序且完整覆盖原范围。
pub fn split_runs(base: Vec<(Range<usize>, HighlightStyle)>, tokens: &[(usize, usize)], dark: bool) -> Vec<(Range<usize>, HighlightStyle)> {
    let mut out = Vec::with_capacity(base.len() + 8);
    for (r, st) in base {
        let from = tokens.partition_point(|t| t.0 < r.start);
        let mut cur = r.start;
        for &(at, depth) in tokens[from..].iter().take_while(|t| t.0 < r.end) {
            if at > cur {
                out.push((cur..at, st));
            }
            out.push((at..at + 1, HighlightStyle { color: Some(depth_color(depth, dark)), ..st }));
            cur = at + 1;
        }
        if cur < r.end {
            out.push((cur..r.end, st));
        }
    }
    out
}

/// 包住原高亮器的括号着色器。
struct RainbowHighlighter {
    inner: Box<dyn InputHighlighter>,
    rules: Option<Rules>,
    text: Option<ropey::Rope>,
    rev: u64,
    /// (编辑序号, 括号 (偏移, 深度) 列表)
    cache: RefCell<Option<(u64, Rc<Vec<(usize, usize)>>)>>,
}

impl RainbowHighlighter {
    /// 当前文本的括号列表(带缓存);语言不支持或文件过大时为 `None`。
    fn tokens(&self) -> Option<Rc<Vec<(usize, usize)>>> {
        let (rules, text) = (self.rules.as_ref()?, self.text.as_ref()?);
        if text.len() > MAX_BYTES {
            return None;
        }
        if let Some((rev, t)) = self.cache.borrow().as_ref() {
            if *rev == self.rev {
                return Some(t.clone());
            }
        }
        let mut toks: Vec<(usize, usize)> = scan(&text.to_string(), rules).iter().flat_map(|p| [(p.open, p.depth), (p.close, p.depth)]).collect();
        toks.sort_unstable();
        let toks = Rc::new(toks);
        *self.cache.borrow_mut() = Some((self.rev, toks.clone()));
        Some(toks)
    }
}

impl InputHighlighter for RainbowHighlighter {
    fn language(&self) -> SharedString {
        self.inner.language()
    }

    fn update(&mut self, edit: Option<InputEdit>, text: &ropey::Rope, folding: bool, window: &mut Window, cx: &mut Context<EditorState>) {
        self.inner.update(edit, text, folding, window, cx);
        self.text = Some(text.clone());
        self.rev += 1;
    }

    fn update_batch(&mut self, edits: &[(InputEdit, ropey::Rope)], folding: bool, window: &mut Window, cx: &mut Context<EditorState>) {
        self.inner.update_batch(edits, folding, window, cx);
        if let Some((_, text)) = edits.last() {
            self.text = Some(text.clone());
            self.rev += 1;
        }
    }

    fn styles(&self, range: &Range<usize>, resolver: &dyn HighlightStyleResolver) -> Vec<(Range<usize>, HighlightStyle)> {
        let base = self.inner.styles(range, resolver);
        if !ENABLED.with(Cell::get) {
            return base;
        }
        match self.tokens() {
            Some(t) if !t.is_empty() => split_runs(base, &t, DARK.with(Cell::get)),
            _ => base,
        }
    }

    fn fold_ranges(&self, text: &ropey::Rope) -> Vec<FoldRange> {
        self.inner.fold_ranges(text)
    }

    fn fold_ranges_for_edit(&self, range: Range<usize>, text: &ropey::Rope) -> Vec<FoldRange> {
        self.inner.fold_ranges_for_edit(range, text)
    }
}

/// 高亮工厂:tree-sitter 高亮器外面套括号着色器;语言没有解析器时与原来一样返回 `None`。
pub fn factory() -> InputHighlighterFactory {
    let inner = gpui_kit::component::highlighter::input_highlighter_factory();
    Rc::new(move |lang| {
        let h = inner(lang)?;
        Some(Box::new(RainbowHighlighter { inner: h, rules: rules_for(lang), text: None, rev: 0, cache: RefCell::new(None) }) as Box<dyn InputHighlighter>)
    })
}

/// 给一个编辑器(重新)装上括号着色的高亮工厂,并让它重新排版。
pub fn install(ed: &mut EditorState, cx: &mut Context<EditorState>) {
    ed.set_highlighter_factory(factory(), cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(r: Range<usize>) -> (Range<usize>, HighlightStyle) {
        (r, HighlightStyle::default())
    }

    #[::core::prelude::v1::test]
    fn 括号处切开且完整覆盖() {
        let out = split_runs(vec![plain(0..10)], &[(2, 0), (5, 1)], true);
        let spans: Vec<_> = out.iter().map(|(r, _)| r.clone()).collect();
        assert_eq!(spans, vec![0..2, 2..3, 3..5, 5..6, 6..10]);
        assert_eq!(out[1].1.color, Some(depth_color(0, true)));
        assert_eq!(out[3].1.color, Some(depth_color(1, true)));
        assert_eq!(out[0].1.color, None);
    }

    #[::core::prelude::v1::test]
    fn 括号在段边界与范围外() {
        // 括号恰在段首、段尾前一字节;范围外的括号不影响
        let out = split_runs(vec![plain(4..8), plain(8..12)], &[(1, 0), (4, 0), (7, 1), (12, 0)], false);
        let spans: Vec<_> = out.iter().map(|(r, _)| r.clone()).collect();
        assert_eq!(spans, vec![4..5, 5..7, 7..8, 8..12]);
    }

    #[::core::prelude::v1::test]
    fn 保留原有样式只改颜色() {
        let mut st = HighlightStyle::default();
        st.font_weight = Some(FontWeight::BOLD);
        let out = split_runs(vec![(0..3, st)], &[(1, 2)], true);
        assert_eq!(out[1].1.font_weight, Some(FontWeight::BOLD));
        assert_eq!(out[1].1.color, Some(depth_color(2, true)));
    }

    #[::core::prelude::v1::test]
    fn 深度循环取四种颜色且深浅色不同() {
        assert_eq!(depth_color(0, true), depth_color(4, true));
        assert_ne!(depth_color(0, true), depth_color(1, true));
        assert_ne!(depth_color(0, true), depth_color(0, false));
    }

    #[::core::prelude::v1::test]
    fn 同步报告变化() {
        sync(true, true);
        assert!(!sync(true, true));
        assert!(sync(false, true));
        assert!(sync(false, false));
        sync(true, true);
    }
}
