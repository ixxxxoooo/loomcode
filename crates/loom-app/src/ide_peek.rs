//! 「转到引用」的预览窗(对标 VS Code 的 Peek References):左边按文件分组列出所有引用(每行是带语法高亮的代码,命中词加底色),
//! 右边是选中引用所在文件的代码预览(语法高亮、目标行高亮)。单击选中并预览,双击或回车在编辑器里打开,↑↓ 切换,Esc 关闭。
//!
//! @author ixxxxoooo

use crate::code_hl::{overlay_background, split_by_line, styles};
use crate::ide::IdeView;
use crate::ui::MONO;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

/// 预览窗右侧最多画多少行(目标行前后)。
const PREVIEW_LINES: usize = 60;
/// 目标行之前多留几行上文。
const LEAD: usize = 8;
/// 超过这个大小的文件不做预览。
const MAX_PREVIEW_BYTES: usize = 2_000_000;

/// 服务器给的一处引用(行 / 列都是 0 起的字符位置)。
#[derive(Debug, Clone, PartialEq)]
pub struct RawRef {
    pub path: PathBuf,
    pub line: u32,
    pub col: u32,
    pub end_col: u32,
    pub line_text: String,
}

/// 列表里的一项:已去掉行首空白的代码文本、命中词区间与语法高亮。
#[derive(Clone)]
pub struct RefItem {
    pub path: PathBuf,
    pub line: u32,
    pub col: u32,
    pub text: String,
    pub highlights: Vec<(Range<usize>, HighlightStyle)>,
}

/// 右侧预览的文件内容。
pub struct Preview {
    pub path: PathBuf,
    pub lines: Arc<Vec<String>>,
}

/// 引用预览窗的状态。
pub struct PeekState {
    pub items: Vec<RefItem>,
    pub selected: usize,
    pub symbol: String,
    pub preview: Option<Preview>,
}

/// 字符列 → 字节偏移(越界取行尾)。
fn byte_at(s: &str, col: u32) -> usize {
    s.char_indices().nth(col as usize).map_or(s.len(), |(i, _)| i)
}

impl PeekState {
    /// 由原始引用构造:按文件、行号排序,算好每行的语法高亮与命中词底色,并载入第一项的预览。
    ///
    /// 参数:`raw` 引用;`symbol` 符号名(标题用);`cx` 取主题(着色用)。
    pub fn new(mut raw: Vec<RawRef>, symbol: String, cx: &App) -> Self {
        raw.sort_by(|a, b| (&a.path, a.line, a.col).cmp(&(&b.path, b.line, b.col)));
        let hit_bg = cx.theme().warning.opacity(0.35);
        let items = raw
            .into_iter()
            .map(|r| {
                let lead = r.line_text.len() - r.line_text.trim_start().len();
                let text = r.line_text.trim().to_string();
                let (a, b) = (byte_at(&r.line_text, r.col).saturating_sub(lead), byte_at(&r.line_text, r.end_col).saturating_sub(lead));
                let base = styles(&text, crate::file_tree::language_for(&r.path), cx);
                let hit = a.min(text.len())..b.min(text.len());
                RefItem { path: r.path, line: r.line, col: r.col, highlights: overlay_background(&base, hit, hit_bg), text }
            })
            .collect();
        let mut st = Self { items, selected: 0, symbol, preview: None };
        st.load_preview();
        st
    }

    /// 载入选中引用所在文件的内容(同一文件不重复读)。
    pub fn load_preview(&mut self) {
        let Some(item) = self.items.get(self.selected) else { return };
        if self.preview.as_ref().is_some_and(|p| p.path == item.path) {
            return;
        }
        self.preview = std::fs::read_to_string(&item.path)
            .ok()
            .filter(|t| t.len() <= MAX_PREVIEW_BYTES)
            .map(|t| Preview { path: item.path.clone(), lines: Arc::new(t.lines().map(String::from).collect()) });
    }

    /// 引用涉及的文件数。
    fn file_count(&self) -> usize {
        let mut seen: Vec<&PathBuf> = Vec::new();
        for i in &self.items {
            if !seen.contains(&&i.path) {
                seen.push(&i.path);
            }
        }
        seen.len()
    }
}

impl IdeView {
    /// 选中第 `i` 项并更新预览。
    pub(crate) fn peek_select(&mut self, i: usize, cx: &mut Context<Self>) {
        if let Some(crate::ide_overlay::Overlay::Refs(st)) = &mut self.overlay {
            if i < st.items.len() {
                st.selected = i;
                st.load_preview();
                cx.notify();
            }
        }
    }

    /// ↑ / ↓ 切换选中项。
    pub(crate) fn peek_move(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(crate::ide_overlay::Overlay::Refs(st)) = &self.overlay {
            let n = st.items.len() as isize;
            if n > 0 {
                let next = (st.selected as isize + delta).rem_euclid(n) as usize;
                self.peek_select(next, cx);
            }
        }
    }

    /// 在编辑器里打开选中的引用并关闭预览窗。
    pub(crate) fn peek_open(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(crate::ide_overlay::Overlay::Refs(st)) = &self.overlay else { return };
        let Some(r) = st.items.get(i).cloned() else { return };
        self.close_overlay(window, cx);
        self.open_at(r.path, r.line, r.col, window, cx);
    }
}

/// 预览窗的标题与内容(固定高度,左右分栏)。
pub fn peek_body(st: &PeekState, cx: &mut Context<IdeView>) -> (String, AnyElement) {
    let t = cx.theme().clone();
    let title = format!("{} — {} 个引用({} 个文件)", st.symbol, st.items.len(), st.file_count());
    // 左:按文件分组的列表
    let mut list = v_flex().w_full();
    let mut last: Option<&PathBuf> = None;
    for (i, it) in st.items.iter().enumerate() {
        if last != Some(&it.path) {
            last = Some(&it.path);
            let name = it.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let dir = it.path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let count = st.items.iter().filter(|x| x.path == it.path).count();
            list = list.child(
                h_flex().h(px(24.)).px_2().gap_1p5().items_center().flex_none()
                    .child(crate::file_icons::file_icon_img(&name, false, false, 14.))
                    .child(div().text_size(px(12.5)).font_weight(FontWeight::MEDIUM).child(name))
                    .child(div().flex_1().min_w_0().truncate().text_size(px(11.)).text_color(t.muted_foreground).child(dir))
                    .child(div().px_1p5().rounded_full().bg(t.muted).text_size(px(10.5)).child(count.to_string())),
            );
        }
        let sel = i == st.selected;
        list = list.child(
            h_flex().id(("peek-ref", i)).h(px(22.)).pl_6().pr_2().gap_2().items_center().flex_none().cursor_pointer().overflow_hidden()
                .when(sel, |d| d.bg(t.list_active)).when(!sel, |d| d.hover(|d| d.bg(t.list_hover)))
                .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| if ev.click_count() >= 2 { this.peek_open(i, window, cx) } else { this.peek_select(i, cx) }))
                .child(div().flex_none().w(px(34.)).text_size(px(10.5)).text_color(t.muted_foreground).font_family(MONO).child((it.line + 1).to_string()))
                .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_size(px(12.)).font_family(MONO).child(StyledText::new(it.text.clone()).with_highlights(it.highlights.clone()))),
        );
    }
    // 右:代码预览
    let target = st.items.get(st.selected);
    let preview = match (&st.preview, target) {
        (Some(p), Some(it)) => {
            let first = (it.line as usize).saturating_sub(LEAD);
            let last = (first + PREVIEW_LINES).min(p.lines.len());
            let slice: Vec<&str> = p.lines[first..last].iter().map(String::as_str).collect();
            let all = styles(&slice.join("\n"), crate::file_tree::language_for(&p.path), cx);
            let per = split_by_line(&slice, &all);
            let name = p.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let rows = slice.iter().enumerate().map(|(k, l)| {
                let ln = first + k;
                let hit = ln == it.line as usize;
                h_flex().h(px(20.)).flex_none().when(hit, |d| d.bg(t.warning.opacity(0.14)))
                    .child(div().w(px(44.)).flex_none().pr_2().text_right().text_size(px(11.)).text_color(t.muted_foreground).font_family(MONO).child((ln + 1).to_string()))
                    .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_size(px(12.)).font_family(MONO).child(StyledText::new(if l.is_empty() { " ".to_string() } else { l.to_string() }).with_highlights(per[k].clone())))
            });
            v_flex().size_full().min_w_0()
                .child(h_flex().h(px(24.)).px_2().items_center().flex_none().border_b_1().border_color(t.border).text_size(px(12.)).text_color(t.muted_foreground).child(name))
                .child(div().id("peek-preview").flex_1().min_h_0().overflow_y_scroll().py_1().children(rows))
                .into_any_element()
        }
        _ => div().p_3().text_size(px(12.)).text_color(t.muted_foreground).child("无法预览这个文件").into_any_element(),
    };
    let body = h_flex().w_full().h(px(380.)).border_t_1().border_color(t.border)
        .child(div().id("peek-list").w(px(360.)).h_full().flex_none().overflow_y_scroll().border_r_1().border_color(t.border).child(list))
        .child(div().flex_1().min_w_0().h_full().child(preview))
        .into_any_element();
    (title, body)
}
