//! 差异标签里的代码区:自绘的**可选中、可复制**的 Diff 视图。
//!
//! 原先的差异行是一堆 `StyledText` 元素,不支持选中。这里整个差异正文由一个画布元素自己绘制
//! (背景、行号、语法着色文字、选区),并处理鼠标:拖动选择(限制在按下时所在的一栏:左 / 右 / 统一)、
//! 双击选词、三击选行、⌘C 复制、⌘A 全选。只绘制视口内的行,长 Diff 也不卡。
//!
//! 布局三种:左右对比、统一视图,以及**纯新增 / 纯删除**(新文件 / 整个文件被删):左右对比下只剩有内容的
//! 那一栏,占满整行,不再留一个空的左栏。
//!
//! @author ixxxxoooo

use crate::diff::{DiffLine, line_styles};
use crate::diff_split::pair_rows;
use crate::ide_diff::Hunk;
use crate::theme::diff_colors;
use crate::ui::MONO;
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;
use similar::ChangeTag;
use std::cell::{Cell as StdCell, RefCell};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::rc::Rc;

actions!(diff_code, [DiffCopy, DiffSelectAll]);

/// 注册快捷键(应用启动时调用一次)。
pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("cmd-c", DiffCopy, Some("DiffCode")), KeyBinding::new("cmd-a", DiffSelectAll, Some("DiffCode"))]);
    // 内联 Diff 没有焦点,⌘C 靠全局按键观察:焦点不在输入框 / 终端 / 其它可选中文本时,复制它的选区
    cx.observe_keystrokes(|ev, _, cx| {
        let k = &ev.keystroke;
        if k.key != "c" || !k.modifiers.platform || k.modifiers.shift || k.modifiers.alt {
            return;
        }
        if ev.context_stack.iter().any(|c| ["Input", "TextView", "DiffCode", "Terminal", "IdeView"].iter().any(|n| c.contains(n))) {
            return;
        }
        if let Some(text) = inline_selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    })
    .detach();
}

/// 行高 / 字号 / 行号栏宽 / 统一视图的 +− 标记栏宽 / 文字左右内边距(像素)。
const ROW_H: f32 = 18.;
const FONT: f32 = 12.;
const GUTTER: f32 = 44.;
const MARK_W: f32 = 14.;
const PAD: f32 = 6.;

/// 一栏里的一行。
#[derive(Clone)]
pub struct Cell {
    text: SharedString,
    no: Option<usize>,
    tag: ChangeTag,
    styles: Vec<(Range<usize>, HighlightStyle)>,
}

/// 画布上的一行。
#[derive(Clone)]
pub enum DRow {
    /// 分隔条(「已折叠 N 行未更改」「还有 N 行未显示」)的文字
    Sep(String),
    /// 左右对比:(旧, 新),一侧没有内容时为 `None`
    Pair(Option<Cell>, Option<Cell>),
    /// 统一视图或只剩一栏时的一行
    One(Cell),
}

/// 整份差异的布局。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Layout {
    Split,
    Unified,
    /// 只剩一栏(0 = 旧 / 左,1 = 新 / 右):纯新增 → `Only(1)`,纯删除 → `Only(0)`
    Only(u8),
}

/// 栏的编号:左 0、右 1、统一视图 2。
const UNIFIED: u8 = 2;

impl Layout {
    /// 由内容与偏好决定布局:左右对比下,整份差异都是新增(或都是删除)就只留一栏。
    pub fn decide(hunks: &[Hunk], split: bool) -> Self {
        let all = |tag: ChangeTag| !hunks.is_empty() && hunks.iter().all(|h| h.lines.iter().all(|l| l.tag == tag));
        match (split, all(ChangeTag::Insert), all(ChangeTag::Delete)) {
            (false, _, _) => Layout::Unified,
            (true, true, _) => Layout::Only(1),
            (true, _, true) => Layout::Only(0),
            _ => Layout::Split,
        }
    }
}

/// 一栏的几何(相对画布左上角):栏编号、起点、宽度、文字起点。
#[derive(Clone, Copy, Debug, PartialEq)]
struct Col {
    side: u8,
    x0: f32,
    w: f32,
    tx: f32,
}

fn columns(layout: Layout, width: f32) -> Vec<Col> {
    match layout {
        Layout::Split => {
            let w = ((width - 1.) / 2.).floor().max(0.);
            vec![Col { side: 0, x0: 0., w, tx: GUTTER + PAD }, Col { side: 1, x0: w + 1., w: width - w - 1., tx: w + 1. + GUTTER + PAD }]
        }
        Layout::Unified => vec![Col { side: UNIFIED, x0: 0., w: width, tx: GUTTER + MARK_W + PAD }],
        Layout::Only(s) => vec![Col { side: s, x0: 0., w: width, tx: GUTTER + PAD }],
    }
}

/// 选区:在哪一栏,从 anchor 到 head(行号, 字节偏移),head 可以在 anchor 之前。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sel {
    side: u8,
    anchor: (usize, usize),
    head: (usize, usize),
}

impl Sel {
    /// 由前到后排好的两端。
    fn ordered(&self) -> ((usize, usize), (usize, usize)) {
        if self.anchor <= self.head { (self.anchor, self.head) } else { (self.head, self.anchor) }
    }
}

/// 上一次绘制留下的几何,供鼠标命中测试用。
#[derive(Default)]
struct Geom {
    origin: Point<Pixels>,
    cols: Vec<Col>,
    /// 视口内各行的排版结果:(行, 栏) → 排版
    shaped: HashMap<(usize, u8), ShapedLine>,
}

/// 选择的状态机(按下 / 拖动 / 松开):实体版(`DiffCode`)与无状态的内联版共用。
#[derive(Default, Clone, Copy)]
struct SelState {
    sel: Option<Sel>,
    dragging: bool,
}

impl SelState {
    /// 按下:单击落点,双击选词,三击选行。
    fn press(&mut self, rows: &[DRow], geom: &Geom, pos: Point<Pixels>, clicks: usize) {
        let Some((side, row, byte)) = hit_test(rows, geom, pos, None) else { return };
        let len = cell_at(rows, row, side).map_or(0, |c| c.text.len());
        self.dragging = false;
        self.sel = Some(match clicks {
            1 => {
                self.dragging = true;
                Sel { side, anchor: (row, byte), head: (row, byte) }
            }
            2 => {
                let (a, b) = cell_at(rows, row, side).map_or((0, 0), |c| word_bounds(&c.text, byte));
                Sel { side, anchor: (row, a), head: (row, b) }
            }
            _ => Sel { side, anchor: (row, 0), head: (row, len) },
        });
    }

    /// 拖动:终点跟着鼠标,固定在按下时的那一栏。返回选区是否变了。
    fn drag(&mut self, rows: &[DRow], geom: &Geom, pos: Point<Pixels>) -> bool {
        if !self.dragging {
            return false;
        }
        let Some(side) = self.sel.map(|s| s.side) else { return false };
        let Some((_, row, byte)) = hit_test(rows, geom, pos, Some(side)) else { return false };
        if let Some(s) = self.sel.as_mut() {
            s.head = (row, byte);
        }
        true
    }

    /// 松开:没拖动(起点 = 终点)就清掉选区,点一下取消选择。
    fn release(&mut self) {
        if !self.dragging {
            return;
        }
        self.dragging = false;
        if self.sel.is_some_and(|s| s.anchor == s.head) {
            self.sel = None;
        }
    }
}

/// 窗口坐标 → (栏, 行, 字节偏移)。`force_side` 拖动时固定在按下时的那一栏。
fn hit_test(rows: &[DRow], g: &Geom, pos: Point<Pixels>, force_side: Option<u8>) -> Option<(u8, usize, usize)> {
    if g.cols.is_empty() || rows.is_empty() {
        return None;
    }
    let (lx, ly) = (f32::from(pos.x - g.origin.x), f32::from(pos.y - g.origin.y));
    let row = ((ly / ROW_H).floor().max(0.) as usize).min(rows.len() - 1);
    let col = match force_side {
        Some(s) => *g.cols.iter().find(|c| c.side == s)?,
        None => *g.cols.iter().find(|c| lx < c.x0 + c.w).or(g.cols.last())?,
    };
    let Some(cell) = cell_at(rows, row, col.side) else { return Some((col.side, row, 0)) };
    let tx = lx - col.tx;
    let byte = match g.shaped.get(&(row, col.side)) {
        Some(sh) => sh.closest_index_for_x(px(tx.max(0.))),
        None if tx <= 0. => 0,
        None => cell.text.len(),
    };
    Some((col.side, row, byte.min(cell.text.len())))
}

pub struct DiffCode {
    hunks: Vec<Hunk>,
    hidden_tail: usize,
    lang: String,
    rows: Rc<Vec<DRow>>,
    layout: Layout,
    dark: bool,
    st: SelState,
    focus: FocusHandle,
    geom: Rc<RefCell<Geom>>,
}

fn cell_of(line: &DiffLine, no: Option<usize>, styles: Vec<(Range<usize>, HighlightStyle)>) -> Cell {
    Cell { text: line.text.clone().into(), no, tag: line.tag, styles }
}

/// 把改动块转成画布行。
fn build_rows(hunks: &[Hunk], hidden_tail: usize, layout: Layout, lang: &str, cx: &App) -> Vec<DRow> {
    let mut rows = Vec::new();
    for h in hunks {
        if h.hidden_before > 0 {
            rows.push(DRow::Sep(format!("⋯ 已折叠 {} 行未更改", h.hidden_before)));
        }
        if layout == Layout::Split {
            let pairs = pair_rows(&h.lines);
            let left: Vec<DiffLine> = pairs.iter().filter_map(|r| r.left.clone()).collect();
            let right: Vec<DiffLine> = pairs.iter().filter_map(|r| r.right.clone()).collect();
            let (mut ls, mut rs) = (line_styles(&left, lang, cx).into_iter(), line_styles(&right, lang, cx).into_iter());
            for r in &pairs {
                let l = r.left.as_ref().map(|l| cell_of(l, l.old_no, ls.next().unwrap_or_default()));
                let n = r.right.as_ref().map(|l| cell_of(l, l.new_no, rs.next().unwrap_or_default()));
                rows.push(DRow::Pair(l, n));
            }
        } else {
            let styles = line_styles(&h.lines, lang, cx);
            for (l, st) in h.lines.iter().zip(styles) {
                // 统一视图:删除行显示旧行号,其余显示新行号;只剩一栏时用那一栏自己的行号
                let no = match layout {
                    Layout::Only(0) => l.old_no,
                    Layout::Only(_) => l.new_no,
                    _ => l.new_no.or(l.old_no),
                };
                rows.push(DRow::One(cell_of(l, no, st)));
            }
        }
    }
    if hidden_tail > 0 {
        rows.push(DRow::Sep(format!("⋯ 已折叠 {hidden_tail} 行未更改")));
    }
    rows
}

impl DiffCode {
    pub fn new(hunks: Vec<Hunk>, hidden_tail: usize, lang: String, split: bool, cx: &mut Context<Self>) -> Self {
        let layout = Layout::decide(&hunks, split);
        let rows = Rc::new(build_rows(&hunks, hidden_tail, layout, &lang, cx));
        let mut this = Self { hunks, hidden_tail, lang, rows, layout, dark: cx.theme().is_dark(), st: SelState::default(), focus: cx.focus_handle(), geom: Rc::default() };
        // 调试用:LOOM_DIFF_SELECT=1 打开差异标签即全选(截图验证选区绘制)
        if std::env::var_os("LOOM_DIFF_SELECT").is_some() {
            let side = this.default_side();
            let last = this.rows.len().saturating_sub(1);
            let end = this.cell(last, side).map_or(0, |c| c.text.len());
            this.st.sel = Some(Sel { side, anchor: (0, 0), head: (last, end) });
        }
        this
    }

    /// 偏好(左右 / 统一)或深浅色变了就重建行(语法着色依赖主题);没变什么都不做。
    pub fn sync(&mut self, split: bool, cx: &mut Context<Self>) {
        let layout = Layout::decide(&self.hunks, split);
        let dark = cx.theme().is_dark();
        if layout == self.layout && dark == self.dark {
            return;
        }
        self.layout = layout;
        self.dark = dark;
        self.rows = Rc::new(build_rows(&self.hunks, self.hidden_tail, layout, &self.lang, cx));
        self.st.sel = None;
        cx.notify();
    }

    fn cell(&self, row: usize, side: u8) -> Option<&Cell> {
        cell_at(&self.rows, row, side)
    }

    /// 选中的文字(一栏内按行拼接,没有内容的行跳过)。
    pub fn selected_text(&self) -> Option<String> {
        selected_text(&self.rows, self.st.sel.as_ref()?)
    }

    fn default_side(&self) -> u8 {
        match self.layout {
            Layout::Split => 1,
            Layout::Unified => UNIFIED,
            Layout::Only(s) => s,
        }
    }
}

fn cell_at(rows: &[DRow], row: usize, side: u8) -> Option<&Cell> {
    match rows.get(row)? {
        DRow::Sep(_) => None,
        DRow::One(c) => Some(c),
        DRow::Pair(l, r) => if side == 0 { l.as_ref() } else { r.as_ref() },
    }
}

/// 选区里的文字。
fn selected_text(rows: &[DRow], sel: &Sel) -> Option<String> {
    let ((r0, c0), (r1, c1)) = sel.ordered();
    let mut out = Vec::new();
    for r in r0..=r1.min(rows.len().saturating_sub(1)) {
        let Some(cell) = cell_at(rows, r, sel.side) else { continue };
        let (a, b) = (if r == r0 { c0 } else { 0 }, if r == r1 { c1 } else { cell.text.len() });
        let (a, b) = (a.min(cell.text.len()), b.min(cell.text.len()));
        out.push(cell.text.get(a..b).unwrap_or("").to_string());
    }
    let text = out.join("\n");
    (!text.is_empty()).then_some(text)
}

/// 字节偏移 `at` 所在的「词」(字母数字下划线连成一段;其余字符各自成段)的范围。
/// 光标正好在词尾时取前面那个词。
fn word_bounds(text: &str, at: usize) -> (usize, usize) {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let (after, before) = (text[at..].chars().next(), text[..at].chars().next_back());
    let pick = match (after, before) {
        (Some(c), _) if is_word(c) => Some((at, c)),
        (_, Some(c)) if is_word(c) => Some((at - c.len_utf8(), c)),
        (Some(c), _) => Some((at, c)),
        (None, Some(c)) => Some((at - c.len_utf8(), c)),
        (None, None) => None,
    };
    let Some((i, c)) = pick else { return (at, at) };
    if !is_word(c) {
        return (i, i + c.len_utf8());
    }
    let start = text[..i].char_indices().rev().take_while(|(_, ch)| is_word(*ch)).last().map_or(i, |(j, _)| j);
    let end = i + text[i..].char_indices().take_while(|(_, ch)| is_word(*ch)).last().map_or(0, |(j, ch)| j + ch.len_utf8());
    (start, end)
}

/// 把一行文字按语法着色区间切成若干段:(范围, 样式);没有样式的段为 `None`。
fn segments(len: usize, styles: &[(Range<usize>, HighlightStyle)]) -> Vec<(Range<usize>, Option<HighlightStyle>)> {
    let mut out = Vec::new();
    let mut pos = 0;
    for (r, st) in styles {
        let (a, b) = (r.start.max(pos), r.end.min(len));
        if a >= b {
            continue;
        }
        if a > pos {
            out.push((pos..a, None));
        }
        out.push((a..b, Some(*st)));
        pos = b;
    }
    if pos < len {
        out.push((pos..len, None));
    }
    out
}

/// 绘制用到的颜色(渲染时按主题取好)。
#[derive(Clone, Copy)]
struct Palette {
    bg: Hsla,
    fg: Hsla,
    muted: Hsla,
    border: Hsla,
    add_bg: Hsla,
    del_bg: Hsla,
    add_fg: Hsla,
    del_fg: Hsla,
    blank: Hsla,
    sep: Hsla,
    selection: Hsla,
}

fn shape(text: &str, size: f32, segs: &[(Range<usize>, Option<HighlightStyle>)], fg: Hsla, window: &Window) -> ShapedLine {
    let mut font = window.text_style().font();
    font.family = SharedString::from(MONO);
    let runs: Vec<TextRun> = if text.is_empty() {
        Vec::new()
    } else {
        segs.iter().map(|(r, st)| {
            let mut f = font.clone();
            if let Some(w) = st.and_then(|s| s.font_weight) { f.weight = w; }
            if let Some(s) = st.and_then(|s| s.font_style) { f.style = s; }
            TextRun { len: r.len(), font: f, color: st.and_then(|s| s.color).unwrap_or(fg), background_color: None, underline: None, strikethrough: None }
        }).collect()
    };
    window.text_system().shape_line(SharedString::from(text.to_string()), px(size), &runs, None)
}

#[allow(clippy::too_many_arguments)]
fn paint_all(rows: &[DRow], layout: Layout, sel: Option<Sel>, pal: Palette, geom: &Rc<RefCell<Geom>>, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let width = f32::from(bounds.size.width);
    let cols = columns(layout, width);
    {
        let mut g = geom.borrow_mut();
        g.origin = bounds.origin;
        g.cols = cols.clone();
        g.shaped.clear();
    }
    let clip = window.content_mask().bounds;
    let first = (f32::from(clip.top() - bounds.top()) / ROW_H).floor().max(0.) as usize;
    let last = ((f32::from(clip.bottom() - bounds.top()) / ROW_H).ceil().max(0.) as usize).min(rows.len());
    let sel = sel.map(|s| (s, s.ordered()));
    // 左右对比的中缝:先画,折叠分隔条的不透明底会盖住它,文字不被竖线切开
    if layout == Layout::Split {
        let x = bounds.left() + px(cols[0].w);
        window.paint_quad(fill(Bounds::new(point(x, bounds.top()), size(px(1.), bounds.size.height)), pal.border));
    }
    for r in first..last {
        let top = bounds.top() + px(r as f32 * ROW_H);
        match &rows[r] {
            DRow::Sep(label) => {
                // 先铺一层不透明底盖住左右对比的中缝竖线,再铺分隔条的淡色
                let band = Bounds::new(point(bounds.left(), top), size(bounds.size.width, px(ROW_H)));
                window.paint_quad(fill(band, pal.bg));
                window.paint_quad(fill(band, pal.sep));
                let sh = shape(label, 11., &[(0..label.len(), None)], pal.muted, window);
                let x = bounds.left() + (bounds.size.width - sh.width) / 2.;
                let _ = sh.paint(point(x, top), px(ROW_H), TextAlign::Left, None, window, cx);
            }
            row => {
                for col in &cols {
                    let area = Bounds::new(point(bounds.left() + px(col.x0), top), size(px(col.w), px(ROW_H)));
                    let Some(cell) = cell_at(std::slice::from_ref(row), 0, col.side) else {
                        window.paint_quad(fill(area, pal.blank));
                        continue;
                    };
                    window.with_content_mask(Some(ContentMask { bounds: area }), |window| {
                        match cell.tag {
                            ChangeTag::Insert => window.paint_quad(fill(area, pal.add_bg)),
                            ChangeTag::Delete => window.paint_quad(fill(area, pal.del_bg)),
                            ChangeTag::Equal => {}
                        }
                        // 行号(右对齐)
                        if let Some(n) = cell.no {
                            let s = n.to_string();
                            let sh = shape(&s, FONT, &[(0..s.len(), None)], pal.muted, window);
                            let x = area.left() + px(GUTTER - PAD) - sh.width;
                            let _ = sh.paint(point(x, top), px(ROW_H), TextAlign::Left, None, window, cx);
                        }
                        // 统一视图的 +/− 标记
                        if layout == Layout::Unified && cell.tag != ChangeTag::Equal {
                            let (m, c) = if cell.tag == ChangeTag::Insert { ("+", pal.add_fg) } else { ("−", pal.del_fg) };
                            let sh = shape(m, FONT, &[(0..m.len(), None)], c, window);
                            let _ = sh.paint(point(area.left() + px(GUTTER), top), px(ROW_H), TextAlign::Left, None, window, cx);
                        }
                        let segs = segments(cell.text.len(), &cell.styles);
                        let sh = shape(&cell.text, FONT, &segs, pal.fg, window);
                        let tx = area.left() + px(col.tx - col.x0);
                        // 选区底色(先画,文字盖在上面);跨行选区的非末行多画一小截表示换行也被选中
                        if let Some((s, ((r0, c0), (r1, c1)))) = sel.filter(|(s, _)| s.side == col.side) {
                            let _ = s;
                            if r >= r0 && r <= r1 {
                                let a = if r == r0 { c0 } else { 0 }.min(cell.text.len());
                                let b = if r == r1 { c1 } else { cell.text.len() }.min(cell.text.len());
                                let (xa, mut xb) = (sh.x_for_index(a), sh.x_for_index(b));
                                if b == cell.text.len() { xb = sh.width; }
                                if r < r1 { xb += px(6.); }
                                if xb > xa || r < r1 {
                                    window.paint_quad(fill(Bounds::new(point(tx + xa, top), size((xb - xa).max(px(3.)), px(ROW_H))), pal.selection));
                                }
                            }
                        }
                        let _ = sh.paint(point(tx, top), px(ROW_H), TextAlign::Left, None, window, cx);
                        geom.borrow_mut().shaped.insert((r, col.side), sh);
                    });
                }
            }
        }
    }
}

impl Render for DiffCode {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let (add_bg, del_bg) = diff_colors(cx);
        let pal = Palette {
            bg: t.background, fg: t.foreground, muted: t.muted_foreground, border: t.border, add_bg, del_bg, add_fg: t.success, del_fg: t.danger,
            blank: t.muted.opacity(0.35), sep: t.muted.opacity(0.5), selection: t.selection,
        };
        let (rows, layout, sel, geom) = (self.rows.clone(), self.layout, self.st.sel, self.geom.clone());
        let height = px(self.rows.len() as f32 * ROW_H);
        div().id("diff-code").key_context("DiffCode").track_focus(&self.focus).w_full().h(height).cursor(CursorStyle::IBeam)
            .on_action(cx.listener(|this, _: &DiffCopy, _, cx| {
                if let Some(text) = this.selected_text() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
            }))
            .on_action(cx.listener(|this, _: &DiffSelectAll, _, cx| {
                let side = this.st.sel.map_or_else(|| this.default_side(), |s| s.side);
                let last = this.rows.len().saturating_sub(1);
                let end = this.cell(last, side).map_or(0, |c| c.text.len());
                this.st.sel = Some(Sel { side, anchor: (0, 0), head: (last, end) });
                cx.notify();
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                window.focus(&this.focus, cx);
                let (rows, geom) = (this.rows.clone(), this.geom.clone());
                this.st.press(&rows, &geom.borrow(), ev.position, ev.click_count);
                cx.notify();
            }))
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                let (rows, geom) = (this.rows.clone(), this.geom.clone());
                if this.st.drag(&rows, &geom.borrow(), ev.position) {
                    cx.notify();
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| this.finish_drag(cx)))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, cx| this.finish_drag(cx)))
            .child(
                canvas(|_, _, _| (), move |bounds, _, window, cx| paint_all(&rows, layout, sel, pal, &geom, bounds, window, cx))
                    .absolute()
                    .size_full(),
            )
    }
}

impl DiffCode {
    fn finish_drag(&mut self, cx: &mut Context<Self>) {
        self.st.release();
        cx.notify();
    }
}

// ---------- 内联版:工具卡 / Agent 右栏里的 Diff(调用点是无状态的渲染函数) ----------

/// 一个内联 Diff 的状态:行、几何、选区。按内容哈希存在线程局部表里,重新渲染时认出同一份。
struct Inline {
    rows: Rc<Vec<DRow>>,
    layout: Layout,
    geom: Rc<RefCell<Geom>>,
    st: RefCell<SelState>,
}

thread_local! {
    static INLINE: RefCell<HashMap<u64, Rc<Inline>>> = RefCell::new(HashMap::new());
    /// 当前持有选区的内联 Diff(同一时刻只有一个)
    static OWNER: StdCell<Option<u64>> = const { StdCell::new(None) };
}

/// 表里最多保留的内联 Diff 数(超出就清空重来,丢掉选区而已)。
const INLINE_MAX: usize = 64;

fn inline_selected_text() -> Option<String> {
    let id = OWNER.get()?;
    let inl = INLINE.with_borrow(|m| m.get(&id).cloned())?;
    let sel = inl.st.borrow().sel?;
    selected_text(&inl.rows, &sel)
}

fn content_id(lines: &[DiffLine], max_lines: usize, lang: &str, split: bool, dark: bool) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (max_lines, lang, split, dark).hash(&mut h);
    for l in lines {
        (l.tag as u8, l.old_no, l.new_no, &l.text).hash(&mut h);
    }
    h.finish()
}

/// 内联 Diff:同样自绘、可拖选 / 双击选词 / 三击选行,选中后 ⌘C 复制(焦点不在输入框时)。
/// 点 Diff 以外的任何地方取消选择。参数同 `diff::render_flat`(不含顶部的切换条)。
pub fn inline(lines: &[DiffLine], max_lines: usize, lang: &str, cx: &App) -> AnyElement {
    inline_with(lines, max_lines, lang, crate::prefs(cx).scm.diff_split, cx)
}

/// 同 [`inline`],但左右 / 统一由调用方指定(工具卡里的 Diff 固定统一视图)。
pub fn inline_with(lines: &[DiffLine], max_lines: usize, lang: &str, split: bool, cx: &App) -> AnyElement {
    let dark = cx.theme().is_dark();
    let id = content_id(lines, max_lines, lang, split, dark);
    let inl = INLINE.with_borrow_mut(|m| {
        if m.len() >= INLINE_MAX && !m.contains_key(&id) {
            m.clear();
            OWNER.set(None);
        }
        m.entry(id)
            .or_insert_with(|| {
                let shown = &lines[..lines.len().min(max_lines)];
                let hunks = vec![Hunk { hidden_before: 0, lines: shown.to_vec() }];
                let layout = Layout::decide(&hunks, split);
                let mut rows = build_rows(&hunks, 0, layout, lang, cx);
                if lines.len() > shown.len() {
                    rows.push(DRow::Sep(format!("… 还有 {} 行未显示", lines.len() - shown.len())));
                }
                Rc::new(Inline { rows: Rc::new(rows), layout, geom: Rc::default(), st: RefCell::default() })
            })
            .clone()
    });
    let t = cx.theme();
    let (add_bg, del_bg) = diff_colors(cx);
    let pal = Palette {
        bg: t.background, fg: t.foreground, muted: t.muted_foreground, border: t.border, add_bg, del_bg, add_fg: t.success, del_fg: t.danger,
        blank: t.muted.opacity(0.35), sep: t.muted.opacity(0.5), selection: t.selection,
    };
    let height = px(inl.rows.len() as f32 * ROW_H);
    let (down, moving, up, up_out, paint) = (inl.clone(), inl.clone(), inl.clone(), inl.clone(), inl);
    div().id(SharedString::from(format!("inline-diff-{id}"))).w_full().h(height).cursor(CursorStyle::IBeam)
        .on_mouse_down(MouseButton::Left, move |ev, window, _| {
            OWNER.set(Some(id));
            down.st.borrow_mut().press(&down.rows, &down.geom.borrow(), ev.position, ev.click_count);
            window.refresh();
        })
        .on_mouse_move(move |ev, window, _| {
            if moving.st.borrow_mut().drag(&moving.rows, &moving.geom.borrow(), ev.position) {
                window.refresh();
            }
        })
        .on_mouse_up(MouseButton::Left, move |_, window, _| {
            up.st.borrow_mut().release();
            window.refresh();
        })
        .on_mouse_up_out(MouseButton::Left, move |_, window, _| {
            up_out.st.borrow_mut().release();
            window.refresh();
        })
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, cx| {
                    let sel = paint.st.borrow().sel;
                    paint_all(&paint.rows, paint.layout, sel, pal, &paint.geom, bounds, window, cx);
                    // 点到这个 Diff 以外的地方:取消选择(也避免 ⌘C 时复制到过期的选区)
                    let clear = paint.clone();
                    window.on_mouse_event(move |ev: &MouseDownEvent, phase, window, _| {
                        if phase == DispatchPhase::Capture && OWNER.get() == Some(id) && !bounds.contains(&ev.position) {
                            clear.st.borrow_mut().sel = None;
                            OWNER.set(None);
                            window.refresh();
                        }
                    });
                },
            )
            .absolute()
            .size_full(),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::compute;
    use crate::ide_diff::hunks;

    fn cell(text: &str) -> Cell {
        Cell { text: text.to_string().into(), no: None, tag: ChangeTag::Equal, styles: Vec::new() }
    }

    fn hunks_of(old: &str, new: &str) -> Vec<Hunk> {
        hunks(&compute(old, new).0, 4).0
    }

    #[::core::prelude::v1::test]
    fn 纯新增只留右栏_纯删除只留左栏() {
        assert_eq!(Layout::decide(&hunks_of("", "a\nb\n"), true), Layout::Only(1));
        assert_eq!(Layout::decide(&hunks_of("a\nb\n", ""), true), Layout::Only(0));
        // 统一视图不受影响;有修改 / 有未改动行的仍是左右对比
        assert_eq!(Layout::decide(&hunks_of("", "a\n"), false), Layout::Unified);
        assert_eq!(Layout::decide(&hunks_of("a\n", "b\n"), true), Layout::Split);
        assert_eq!(Layout::decide(&hunks_of("a\n", "a\nb\n"), true), Layout::Split);
        // 没有任何改动块时不当成纯新增
        assert_eq!(Layout::decide(&[], true), Layout::Split);
    }

    #[::core::prelude::v1::test]
    fn 栏的几何() {
        let split = columns(Layout::Split, 1001.);
        assert_eq!(split.len(), 2);
        assert_eq!(split[0].x0, 0.);
        assert_eq!(split[0].w, 500.);
        assert_eq!(split[1].x0, 501.);
        assert_eq!(split[1].w, 500.);
        assert_eq!(columns(Layout::Only(1), 800.), vec![Col { side: 1, x0: 0., w: 800., tx: GUTTER + PAD }]);
        assert_eq!(columns(Layout::Unified, 800.)[0].side, UNIFIED);
    }

    #[::core::prelude::v1::test]
    fn 选区文字按栏取并跳过空行() {
        let rows = vec![
            DRow::Pair(Some(cell("old1")), Some(cell("new1"))),
            DRow::Sep("x".into()),
            DRow::Pair(None, Some(cell("new2"))),
            DRow::Pair(Some(cell("old3")), Some(cell("new3"))),
        ];
        // 右栏从 new1 的第 1 个字节到 new3 的第 3 个字节
        let sel = Sel { side: 1, anchor: (0, 1), head: (3, 3) };
        assert_eq!(selected_text(&rows, &sel).unwrap(), "ew1\nnew2\nnew");
        // 反向选(head 在前)结果一样
        let rev = Sel { side: 1, anchor: (3, 3), head: (0, 1) };
        assert_eq!(selected_text(&rows, &rev), selected_text(&rows, &sel));
        // 左栏:右栏独有的行(左栏为空)被跳过
        let left = Sel { side: 0, anchor: (0, 0), head: (3, 4) };
        assert_eq!(selected_text(&rows, &left).unwrap(), "old1\nold3");
        // 空选区
        assert_eq!(selected_text(&rows, &Sel { side: 1, anchor: (0, 2), head: (0, 2) }), None);
    }

    #[::core::prelude::v1::test]
    fn 双击选词() {
        let t = "let foo_bar = 42;";
        assert_eq!(word_bounds(t, 5), (4, 11));
        assert_eq!(&t[4..11], "foo_bar");
        // 词尾 / 词首都算在词里
        assert_eq!(word_bounds(t, 11), (4, 11));
        assert_eq!(word_bounds(t, 4), (4, 11));
        // 标点单独成段
        assert_eq!(word_bounds(t, 12), (12, 13));
        // 中文按字母数字连成一段
        let c = "名称 = 值";
        assert_eq!(word_bounds(c, 3), (0, 6));
        assert_eq!(word_bounds("", 0), (0, 0));
    }

    #[::core::prelude::v1::test]
    fn 内联_diff_的标识随内容与模式变化() {
        let a = compute("a\n", "b\n").0;
        let b = compute("a\n", "c\n").0;
        let id = |l: &[DiffLine], split, dark| content_id(l, 120, "rust", split, dark);
        assert_eq!(id(&a, true, false), id(&a, true, false));
        assert_ne!(id(&a, true, false), id(&b, true, false));
        assert_ne!(id(&a, true, false), id(&a, false, false));
        assert_ne!(id(&a, true, false), id(&a, true, true));
    }

    #[::core::prelude::v1::test]
    fn 着色区间切段() {
        let st = HighlightStyle { color: Some(hsla(0., 1., 0.5, 1.)), ..Default::default() };
        let segs = segments(10, &[(2..4, st), (6..12, st)]);
        let ranges: Vec<_> = segs.iter().map(|(r, s)| (r.clone(), s.is_some())).collect();
        assert_eq!(ranges, vec![(0..2, false), (2..4, true), (4..6, false), (6..10, true)]);
        assert_eq!(segments(3, &[]).len(), 1);
        assert!(segments(0, &[]).is_empty());
    }
}
