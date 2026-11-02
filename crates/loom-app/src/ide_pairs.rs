//! 括号配对着色与配对竖线(对标 VS Code 的 Bracket Pair Colorization / Bracket Pair Guides)。
//!
//! - **配对扫描**:按语言的注释 / 字符串规则跳过它们里面的括号,用栈配出 `()` `[]` `{}`,每对记嵌套深度。
//! - **着色**:编辑器库只支持给字符范围加底色 / 边框(不能按范围改字色),所以每个括号铺一块按深度循环取色
//!   (金 / 紫 / 蓝)的半透明底色。只铺视口前后各 150 行,滚出范围或内容变化后再刷新。
//! - **竖线**:跨行的配对在开括号所在行的缩进列上画一条竖线(从开括号的下一行到闭括号的上一行,
//!   闭括号需独占行首),颜色同深度色;包住光标的最内层一对更亮。
//!
//! 扫描结果按「文本长度 + 编辑序号」缓存,滚动与移动光标不重扫。超过 512 KB 的文件不处理。
//!
//! @author ixxxxoooo

use crate::ide::IdeDoc;
use gpui_kit::component::input::{EditorState, RangeDecoration, RangeDecorationCollection, RangeDecorationStyle};
use gpui_kit::component::RopeExt as _;
use gpui_kit::*;
use std::cell::RefCell;
use std::sync::Arc;

/// 超过这个字节数的文件不做配对(扫描与装饰都会变慢)。
const MAX_BYTES: usize = 512 * 1024;
/// 着色范围:视口前后各多铺的行数。
const MARGIN_LINES: usize = 150;
/// 一帧最多画的竖线数,嵌套极深时兜底。
const MAX_GUIDES: usize = 120;
/// 一次最多铺的括号底色块数。
const MAX_MARKS: usize = 6000;

/// 一对括号:开括号与闭括号的字节偏移,以及嵌套深度(最外层为 0)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pair {
    pub open: usize,
    pub close: usize,
    pub depth: usize,
}

/// 语言的词法规则:哪些内容里的括号不算数。
#[derive(Debug, Clone, Copy)]
pub struct Rules {
    line_comments: &'static [&'static str],
    block_comment: Option<(&'static str, &'static str)>,
    /// 字符串引号;默认到行尾为止,`multiline` 里的引号可以跨行
    quotes: &'static [u8],
    multiline: &'static [u8],
    /// 是否支持三引号(Python)
    triple: bool,
}

const C_LIKE: Rules = Rules { line_comments: &["//"], block_comment: Some(("/*", "*/")), quotes: b"\"'", multiline: b"", triple: false };
const HASH: Rules = Rules { line_comments: &["#"], block_comment: None, quotes: b"\"'", multiline: b"", triple: false };

/// 按 `file_lang::language_for` 的语言名取规则;不认识的语言(Markdown、纯文本、HTML…)返回 `None`,不做配对。
pub fn rules_for(lang: &str) -> Option<Rules> {
    Some(match lang {
        // Rust 的 `'` 常是生命周期,不当引号
        "rust" => Rules { quotes: b"\"", ..C_LIKE },
        "python" => Rules { triple: true, ..HASH },
        "javascript" | "typescript" | "tsx" | "go" => Rules { multiline: b"`", quotes: b"\"'`", ..C_LIKE },
        "c" | "cpp" | "csharp" | "java" | "kotlin" | "swift" | "scala" | "php" | "zig" => C_LIKE,
        "bash" | "yaml" | "toml" | "ruby" | "make" | "cmake" => HASH,
        "sql" => Rules { line_comments: &["--"], ..C_LIKE },
        "lua" => Rules { line_comments: &["--"], block_comment: None, ..C_LIKE },
        "css" => Rules { line_comments: &[], block_comment: Some(("/*", "*/")), ..C_LIKE },
        "json" => Rules { line_comments: &[], block_comment: None, quotes: b"\"", ..C_LIKE },
        "graphql" => HASH,
        "proto" => C_LIKE,
        _ => return None,
    })
}

fn kind_of(b: u8) -> Option<(u8, bool)> {
    Some(match b {
        b'(' => (0, true),
        b')' => (0, false),
        b'[' => (1, true),
        b']' => (1, false),
        b'{' => (2, true),
        b'}' => (2, false),
        _ => return None,
    })
}

/// 扫描全文,返回按开括号位置排序的配对。注释与字符串里的括号、没有配上的括号都忽略。
pub fn scan(text: &str, rules: &Rules) -> Vec<Pair> {
    let bytes = text.as_bytes();
    let n = bytes.len();
    let mut pairs = Vec::new();
    // (括号种类, 开括号位置)
    let mut stack: Vec<(u8, usize)> = Vec::new();
    let mut i = 0;
    while i < n {
        let rest = &bytes[i..];
        if rules.line_comments.iter().any(|p| rest.starts_with(p.as_bytes())) {
            i += rest.iter().position(|&c| c == b'\n').unwrap_or(rest.len());
            continue;
        }
        if let Some((open, close)) = rules.block_comment {
            if rest.starts_with(open.as_bytes()) {
                let from = open.len();
                i += rest[from..].windows(close.len()).position(|w| w == close.as_bytes()).map_or(rest.len(), |p| from + p + close.len());
                continue;
            }
        }
        let b = bytes[i];
        if rules.quotes.contains(&b) {
            if rules.triple && rest.len() >= 3 && rest[1] == b && rest[2] == b {
                let tri = [b; 3];
                i += rest[3..].windows(3).position(|w| w == tri).map_or(rest.len(), |p| 3 + p + 3);
                continue;
            }
            let multi = rules.multiline.contains(&b);
            i += 1;
            while i < n {
                match bytes[i] {
                    b'\\' => i += 2,
                    c if c == b => {
                        i += 1;
                        break;
                    }
                    b'\n' if !multi => break,
                    _ => i += 1,
                }
            }
            continue;
        }
        if let Some((kind, is_open)) = kind_of(b) {
            if is_open {
                stack.push((kind, i));
            } else if stack.last().is_some_and(|&(k, _)| k == kind) {
                let (_, open) = stack.pop().unwrap();
                pairs.push(Pair { open, close: i, depth: stack.len() });
            }
        }
        i += 1;
    }
    pairs.sort_by_key(|p| p.open);
    pairs
}

/// 深度对应的颜色(金 / 紫 / 蓝循环,取 VS Code 的默认三色)。
pub fn depth_color(depth: usize) -> Hsla {
    [hsla(51. / 360., 1., 0.5, 1.), hsla(302. / 360., 0.59, 0.65, 1.), hsla(205. / 360., 1., 0.55, 1.)][depth % 3]
}

/// 文档的括号配对状态:扫描缓存与已铺的底色块。
pub struct PairState {
    rules: Option<Rules>,
    /// 编辑序号,每次内容变化加一(和文本长度一起作缓存键)
    rev: u64,
    cache: RefCell<Option<((usize, u64), Arc<Vec<Pair>>)>>,
    marks: Option<RangeDecorationCollection>,
    /// 底色块对应的 (文本长度, 编辑序号, 起始行, 结束行)
    marks_key: Option<(usize, u64, usize, usize)>,
}

impl PairState {
    /// 新建;`lang` 是 `language_for` 给的语言名。
    pub fn new(lang: &str) -> Self {
        Self { rules: rules_for(lang), rev: 0, cache: RefCell::new(None), marks: None, marks_key: None }
    }

    /// 内容变了:让缓存与底色块失效。
    pub fn bump(&mut self) {
        self.rev += 1;
    }

    /// 当前内容的配对(带缓存);语言不支持或文件过大时为 `None`。
    pub fn pairs(&self, ed: &EditorState) -> Option<Arc<Vec<Pair>>> {
        let rules = self.rules.as_ref()?;
        let len = ed.text().len();
        if len > MAX_BYTES {
            return None;
        }
        let key = (len, self.rev);
        if let Some((k, p)) = self.cache.borrow().as_ref() {
            if *k == key {
                return Some(p.clone());
            }
        }
        let p = Arc::new(scan(&ed.value(), rules));
        *self.cache.borrow_mut() = Some((key, p.clone()));
        Some(p)
    }
}

/// 视口前后各 `MARGIN_LINES` 行的范围。
fn window_of(vis: &std::ops::Range<usize>, lines: usize) -> (usize, usize) {
    (vis.start.saturating_sub(MARGIN_LINES), (vis.end + MARGIN_LINES).min(lines))
}

/// 底色块是否需要刷新:开关变了、内容变了或视口走出了已铺的范围。渲染时用,很便宜。
pub fn stale(doc: &IdeDoc, enabled: bool, cx: &App) -> bool {
    let st = &doc.pairs;
    let ed = doc.editor.read(cx);
    let len = ed.text().len();
    if !enabled || st.rules.is_none() || len > MAX_BYTES {
        return st.marks.is_some();
    }
    let Some(vis) = ed.visible_row_range() else { return false };
    match st.marks_key {
        Some((l, g, s, e)) => (l, g) != (len, st.rev) || vis.start < s || vis.end.min(ed.text().lines_len()) > e,
        None => true,
    }
}

/// 刷新括号底色块:`enabled` 为假时清掉。
pub fn refresh_colors(doc: &mut IdeDoc, enabled: bool, cx: &mut App) {
    let editor = doc.editor.clone();
    let st = &mut doc.pairs;
    let (decos, key) = {
        let ed = editor.read(cx);
        let pairs = enabled.then(|| st.pairs(ed)).flatten();
        let Some(pairs) = pairs else {
            if let Some(m) = st.marks.take() {
                m.dispose(cx);
            }
            st.marks_key = None;
            return;
        };
        let Some(vis) = ed.visible_row_range() else { return };
        let text = ed.text();
        let (s, e) = window_of(&vis, text.lines_len());
        let from = text.line_start_offset(s);
        let to = if e >= text.lines_len() { text.len() } else { text.line_start_offset(e) };
        let mut decos = Vec::new();
        'all: for p in pairs.iter().filter(|p| p.open < to && p.close >= from) {
            for at in [p.open, p.close] {
                if (from..to).contains(&at) {
                    if decos.len() >= MAX_MARKS {
                        break 'all;
                    }
                    decos.push(RangeDecoration::new(at..at + 1).with_style(RangeDecorationStyle::Fill).with_color(depth_color(p.depth).opacity(0.3)));
                }
            }
        }
        (decos, (text.len(), st.rev, s, e))
    };
    st.marks_key = Some(key);
    match &st.marks {
        Some(c) => c.set(decos, cx),
        None => st.marks = Some(editor.update(cx, |e, cx| e.create_range_decorations_collection(decos, cx))),
    }
}

/// 配对竖线叠层(一个盖住编辑区的透明容器,里面每条线一个绝对定位的 1px 元素)。
/// 光标行不在视口、没有跨行配对或功能关闭时返回 `None`。
pub fn guides(doc: &IdeDoc, cx: &App) -> Option<AnyElement> {
    let ed = doc.editor.read(cx);
    let pairs = doc.pairs.pairs(ed)?;
    let vis = ed.visible_row_range()?;
    let tb = ed.text_bounds()?;
    let text = ed.text();
    let lines = text.lines_len();
    let (from, to) = (text.line_start_offset(vis.start), if vis.end >= lines { text.len() } else { text.line_start_offset(vis.end) });
    let cursor = ed.cursor();
    // 包住光标的最内层一对
    let active = pairs.iter().filter(|p| p.open < cursor && cursor <= p.close).max_by_key(|p| p.open).copied();
    let line_of = |o: usize| text.offset_to_position(o).line as usize;
    let mut lines_out: Vec<AnyElement> = Vec::new();
    for p in pairs.iter().filter(|p| p.open < to && p.close >= from) {
        if lines_out.len() >= MAX_GUIDES {
            break;
        }
        let (ol, cl) = (line_of(p.open), line_of(p.close));
        // 闭括号必须独占行首(只有缩进在它前面),否则不画
        let close_line_start = text.line_start_offset(cl);
        if cl <= ol + 1 || !text.slice(close_line_start..p.close).chars().all(char::is_whitespace) {
            continue;
        }
        let (first, last) = ((ol + 1).max(vis.start), (cl - 1).min(vis.end.saturating_sub(1)));
        if first > last {
            continue;
        }
        // 竖线所在列:开括号那一行的缩进末端
        let open_line = text.slice_line(ol);
        let indent_bytes: usize = open_line.chars().take_while(|c| c.is_whitespace()).map(char::len_utf8).sum();
        let at = text.line_start_offset(ol) + indent_bytes;
        // 开括号所在行可能已滚出视口,x 改用块内第一条可见行的同列位置量
        let x_ref = text.line_start_offset(first) + indent_bytes.min(text.slice_line(first).len().saturating_sub(1));
        let Some(x) = ed.range_to_bounds(&(at..at)).or_else(|| ed.range_to_bounds(&(x_ref..x_ref))).map(|b| b.origin.x) else { continue };
        let (Some(top), Some(bottom)) = (ed.range_to_bounds(&{
            let o = text.line_start_offset(first);
            o..o
        }), ed.range_to_bounds(&{
            let o = text.line_end_offset(last);
            o..o
        })) else {
            continue;
        };
        let (y0, y1) = (top.origin.y.max(tb.top()), (bottom.origin.y + bottom.size.height).min(tb.bottom()));
        if x < tb.left() || y1 <= y0 {
            continue;
        }
        let alpha = if active == Some(*p) { 0.95 } else { 0.4 };
        lines_out.push(div().absolute().left(x - tb.left() - px(0.5)).top(y0 - tb.top()).w(px(1.)).h(y1 - y0).bg(depth_color(p.depth).opacity(alpha)).into_any_element());
    }
    if lines_out.is_empty() {
        return None;
    }
    Some(deferred(anchored().position(tb.origin).anchor(Anchor::TopLeft).child(div().relative().w(tb.size.width).h(tb.size.height).overflow_hidden().children(lines_out))).into_any_element())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(lang: &str, src: &str) -> Vec<(usize, usize, usize)> {
        scan(src, &rules_for(lang).unwrap()).iter().map(|p| (p.open, p.close, p.depth)).collect()
    }

    #[::core::prelude::v1::test]
    fn 嵌套括号带深度() {
        // f(a[0], {b})
        assert_eq!(pairs("rust", "f(a[0], {b})"), vec![(1, 11, 0), (3, 5, 1), (8, 10, 1)]);
    }

    #[::core::prelude::v1::test]
    fn 字符串与注释里的括号不算() {
        assert_eq!(pairs("python", "x = f(\")\")  # (\n"), vec![(5, 9, 0)]);
        assert_eq!(pairs("rust", "a(/* ) */ b) // ("), vec![(1, 11, 0)]);
        // 三引号可跨行
        assert_eq!(pairs("python", "f('''\n)\n''')"), vec![(1, 11, 0)]);
    }

    #[::core::prelude::v1::test]
    fn 转义引号不提前结束字符串() {
        assert_eq!(pairs("python", r#"f("a\")b")"#), vec![(1, 9, 0)]);
    }

    #[::core::prelude::v1::test]
    fn 未闭合的单行字符串到行尾为止() {
        // 第一行引号没闭合,不能吞掉后面的括号
        assert_eq!(pairs("javascript", "a = \"x\n(b)"), vec![(7, 9, 0)]);
    }

    #[::core::prelude::v1::test]
    fn 错配与未配的括号被忽略() {
        assert_eq!(pairs("rust", "(]"), vec![]);
        assert_eq!(pairs("rust", "((a)"), vec![(1, 3, 1)]);
    }

    #[::core::prelude::v1::test]
    fn 生命周期的单引号不当引号() {
        assert_eq!(pairs("rust", "fn f<'a>(x: &'a str) {}"), vec![(8, 19, 0), (21, 22, 0)]);
    }

    #[::core::prelude::v1::test]
    fn 不认识的语言不做配对() {
        assert!(rules_for("markdown").is_none());
        assert!(rules_for("text").is_none());
    }

    #[::core::prelude::v1::test]
    fn 深度循环取三种颜色() {
        assert_eq!(depth_color(0), depth_color(3));
        assert_ne!(depth_color(0), depth_color(1));
    }
}
