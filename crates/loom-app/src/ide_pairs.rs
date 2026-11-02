//! 括号配对扫描与配对竖线(对标 VS Code 的 `editor.guides.bracketPairs`;没有现成的第三方库,Zed 也没有)。
//!
//! - **配对扫描**:按语言的注释 / 字符串规则跳过它们里面的括号,用栈配出 `()` `[]` `{}`,每对记嵌套深度。
//!   括号字符着色在 `ide_rainbow.rs`(包在语法高亮外面,直接改字色)。
//! - **竖线**:跨行的配对在开括号所在行的缩进列上画一条竖线(从开括号的下一行到闭括号的上一行,
//!   闭括号需独占行首),颜色与括号字色同深度同色;包住光标的最内层一对更亮(VS Code 的 `active` 效果)。
//!
//! 扫描结果按「文本长度 + 编辑序号」缓存,滚动与移动光标不重扫。超过 512 KB 的文件不处理。
//!
//! @author ixxxxoooo


use crate::ide::IdeDoc;
use crate::pair_scan::{rules_for, scan, Pair, Rules, MAX_BYTES};
use gpui_kit::component::input::EditorState;
use gpui_kit::component::{ActiveTheme as _, RopeExt as _};
use gpui_kit::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

/// 一帧最多画的竖线数,嵌套极深时兜底。
const MAX_GUIDES: usize = 120;


/// 文档的括号配对状态:扫描缓存。可克隆(内部共享),绘制阶段的画布闭包持有一份。
#[derive(Clone)]
pub struct PairState {
    rules: Option<Rules>,
    /// 编辑序号,每次内容变化加一(和文本长度一起作缓存键)
    rev: Rc<Cell<u64>>,
    cache: Rc<RefCell<Option<((usize, u64), Arc<Vec<Pair>>)>>>,
}

impl PairState {
    /// 新建;`lang` 是 `language_for` 给的语言名。
    pub fn new(lang: &str) -> Self {
        Self { rules: rules_for(lang), rev: Rc::new(Cell::new(0)), cache: Rc::new(RefCell::new(None)) }
    }

    /// 内容变了:让缓存失效。
    pub fn bump(&self) {
        self.rev.set(self.rev.get() + 1);
    }

    /// 当前内容的配对(带缓存);语言不支持或文件过大时为 `None`。
    pub fn pairs(&self, ed: &EditorState) -> Option<Arc<Vec<Pair>>> {
        let rules = self.rules.as_ref()?;
        let len = ed.text().len();
        if len > MAX_BYTES {
            return None;
        }
        let key = (len, self.rev.get());
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

/// 所有配对竖线的矩形与颜色(窗口坐标)。
/// 位置取自编辑器**最近一次绘制**的布局,必须在绘制阶段调用(见 `guides`)。
/// 参数:`ed` 编辑器状态;`st` 配对状态;`dark` 是否深色主题;`canvas` 画布范围。返回:没有跨行配对或不在视口时为空。
fn guide_rects(ed: &EditorState, st: &PairState, dark: bool, canvas: Bounds<Pixels>) -> Vec<(Bounds<Pixels>, Hsla)> {
    let Some(pairs) = st.pairs(ed) else { return Vec::new() };
    let (Some(vis), Some(tb)) = (ed.visible_row_range(), crate::ide_guides::text_view(ed, canvas)) else { return Vec::new() };
    let text = ed.text();
    let lines = text.lines_len();
    let (from, to) = (text.line_start_offset(vis.start), if vis.end >= lines { text.len() } else { text.line_start_offset(vis.end) });
    let cursor = ed.cursor();
    // 包住光标的最内层一对
    let active = pairs.iter().filter(|p| p.open < cursor && cursor <= p.close).max_by_key(|p| p.open).copied();
    let line_of = |o: usize| text.offset_to_position(o).line as usize;
    let mut out: Vec<(Bounds<Pixels>, Hsla)> = Vec::new();
    for p in pairs.iter().filter(|p| p.open < to && p.close >= from) {
        if out.len() >= MAX_GUIDES {
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
        out.push((Bounds::new(point(x - px(0.5), y0), size(px(1.), y1 - y0)), crate::ide_rainbow::depth_color(p.depth, dark).opacity(alpha)));
    }
    out
}

/// 配对竖线叠层:一个盖住编辑区的画布,**在绘制阶段**按编辑器刚画完的布局定位。
/// 不能在渲染阶段算位置:编辑器的布局在它自己的绘制里才更新,渲染里读到的是上一帧的,滚动停下后竖线会停在原地。
pub fn guides(doc: &IdeDoc) -> AnyElement {
    let (editor, st) = (doc.editor.clone(), doc.pairs.clone());
    canvas(
        |_, _, _| (),
        move |canvas, _, window, cx| {
            let dark = cx.theme().is_dark();
            for (r, c) in guide_rects(editor.read(cx), &st, dark, canvas) {
                window.paint_quad(fill(r, c));
            }
        },
    )
    .absolute()
    .size_full()
    .into_any_element()
}
