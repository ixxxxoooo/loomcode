//! 引用预览窗的渲染(尺寸与配色照 VS Code:`peekView*` 主题色、树行高 23、预览与树 7:3、18 行高)。

use crate::ide::IdeView;
use crate::ide_overlay::Overlay;
use crate::ide_peek::{PLine, PeekState, TreeRow};
use std::sync::Arc;
use crate::ui::MONO;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// 预览与树的宽度比(VS Code 默认 0.7)。
const RATIO: f32 = 0.7;
/// 预览窗默认高度(行数,VS Code 默认 18)。
const HEIGHT_LINES: f32 = 18.;
/// 标题栏高度。
const HEAD_H: f32 = 27.;
/// 树的行高(VS Code 的 `ref-tree` 行高 23)。
const ROW_H: f32 = 23.;

/// VS Code 默认主题里 peekView 相关的颜色(浅色 / 深色各一套)。
struct Colors {
    border: Hsla,
    title_bg: Hsla,
    title_label: Hsla,
    title_desc: Hsla,
    result_bg: Hsla,
    result_line: Hsla,
    result_file: Hsla,
    selection: Hsla,
    result_match: Hsla,
    editor_bg: Hsla,
    editor_match: Hsla,
}

/// 预览里引用命中处的底色(`peekViewEditor.matchHighlightBackground`)。
pub fn match_background(dark: bool) -> Hsla {
    colors(dark).editor_match
}

fn colors(dark: bool) -> Colors {
    let c = |v: u32| -> Hsla { rgba(v).into() };
    if dark {
        Colors { border: c(0x3794ffff), title_bg: c(0x252526ff), title_label: c(0xffffffff), title_desc: c(0xccccccb3), result_bg: c(0x252526ff), result_line: c(0xbbbbbbff), result_file: c(0xffffffff), selection: c(0x3399ff33), result_match: c(0xea5c004d), editor_bg: c(0x001f33ff), editor_match: c(0xff8f0099) }
    } else {
        Colors { border: c(0x1a85ffff), title_bg: c(0xf3f3f3ff), title_label: c(0x000000ff), title_desc: c(0x616161ff), result_bg: c(0xf3f3f3ff), result_line: c(0x646465ff), result_file: c(0x1e1e1eff), selection: c(0x3399ff33), result_match: c(0xea5c004d), editor_bg: c(0xf2f8fcff), editor_match: c(0xf5d802de) }
    }
}

impl IdeView {
    /// 引用预览窗:锚在触发光标所在行的下方、铺满编辑区宽度(VS Code 是插进文本里把下面的代码顶开,这里不能插行,只能盖在上面)。
    pub(crate) fn peek_overlay(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let Some(Overlay::Refs(st)) = &self.overlay else { return None };
        let doc = self.tabs.active().and_then(|p| self.docs.get(p))?;
        let ed = doc.editor.read(cx);
        let line = ed.range_to_bounds(&(st.anchor_offset..st.anchor_offset))?;
        let text = ed.text_bounds()?;
        let lh = f32::from(ed.line_height().unwrap_or(px(20.)));
        let origin = point(text.left(), line.origin.y + line.size.height + px(2.));
        let width = f32::from(text.size.width).max(520.);
        let zone = peek_zone(st, width, lh, cx);
        let keys = crate::ide_overlay::key_canvas(cx.weak_entity());
        Some(deferred(anchored().position(origin).anchor(Anchor::TopLeft).snap_to_window_with_margin(px(8.)).child(div().relative().child(keys).child(zone))).into_any_element())
    }
}

/// 预览窗本体:标题栏 + 左预览 + 右树,上下各一条蓝色边框。
fn peek_zone(st: &PeekState, width: f32, lh: f32, cx: &mut Context<IdeView>) -> AnyElement {
    let c = colors(cx.theme().is_dark());
    let body_h = HEIGHT_LINES * lh;
    let target = st.items.get(st.selected);
    let (name, dir) = target.map(|t| (t.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), t.path.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default())).unwrap_or_default();
    let home = std::env::var("HOME").unwrap_or_default();
    let dir = if !home.is_empty() && dir.starts_with(&home) { dir.replacen(&home, "~", 1) } else { dir };
    let head = h_flex().h(px(HEAD_H)).w_full().flex_none().items_center().justify_between().bg(c.title_bg)
        .child(
            h_flex().min_w_0().ml(px(20.)).gap_1().items_baseline().text_size(px(13.))
                .child(div().flex_none().text_color(c.title_label).child(name))
                .child(div().min_w_0().truncate().text_size(px(11.7)).text_color(c.title_desc).child(dir))
                .child(div().flex_none().text_color(c.title_desc).child(format!("- 引用 ({})", st.items.len()))),
        )
        .child(div().id("peek-close").size(px(22.)).mr_1().flex().items_center().justify_center().rounded(px(4.)).cursor_pointer().hover(|d| d.bg(c.selection))
            .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))).child(crate::ui::icon(gpui_kit::assets::IconName::X, 13., c.title_label)));
    let preview_w = (width * RATIO).floor();
    let body = h_flex().w_full().h(px(body_h)).flex_none().overflow_hidden().border_t_1().border_color(c.border.opacity(0.4))
        .child(div().w(px(preview_w)).h_full().flex_none().bg(c.editor_bg).child(preview_pane(st, &c, lh, cx)))
        .child(div().id("peek-tree").flex_1().min_w_0().h_full().overflow_hidden().bg(c.result_bg).child(tree_pane(st, &c, cx)));
    v_flex().w(px(width)).border_t_1().border_b_1().border_color(c.border).shadow_lg().occlude()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(head).child(body).into_any_element()
}

/// 右边的引用树(虚拟列表,只画可见行):单文件时直接列引用;多文件时按文件分组(文件名 + 目录 + 引用数徽标)。
fn tree_pane(st: &PeekState, c: &Colors, cx: &mut Context<IdeView>) -> AnyElement {
    let rows = st.rows.clone();
    let items = Arc::new(st.items.iter().map(|i| (i.text.clone(), i.hit.clone())).collect::<Vec<_>>());
    let (multi, selected) = (st.file_count > 1, st.selected);
    let view = cx.weak_entity();
    let (sel_bg, file_fg, line_fg, badge_bg, match_bg) = (c.selection, c.result_file, c.result_line, c.selection, c.result_match);
    uniform_list("peek-tree-list", rows.len(), move |range, _, _| {
        range.map(|ix| match &rows[ix] {
            TreeRow::File { name, dir, count } => h_flex().h(px(ROW_H)).w_full().px_2().gap_1p5().items_center().text_size(px(13.)).text_color(file_fg)
                .child(crate::file_icons::file_icon_img(name, false, false, 14.))
                .child(div().flex_none().child(name.clone()))
                .child(div().flex_1().min_w_0().truncate().text_size(px(11.7)).opacity(0.7).child(dir.clone()))
                .child(div().mr(px(12.)).px_1p5().rounded_full().bg(badge_bg).text_size(px(11.)).child(count.to_string()))
                .into_any_element(),
            TreeRow::Ref(i) => {
                let i = *i;
                let (text, hit) = &items[i];
                let hl = HighlightStyle { color: Some(file_fg), background_color: Some(match_bg), ..Default::default() };
                let v = view.clone();
                h_flex().id(("peek-ref", ix)).h(px(ROW_H)).w_full().when(multi, |d| d.pl(px(26.))).when(!multi, |d| d.pl_2()).pr_2().items_center().cursor_pointer().overflow_hidden().text_size(px(13.))
                    .text_color(line_fg).when(i == selected, |d| d.bg(sel_bg).text_color(file_fg))
                    .on_click(move |ev, window, cx| { let _ = v.update(cx, |this, cx| if ev.click_count() >= 2 { this.peek_open(i, window, cx) } else { this.peek_select(i, cx) }); })
                    .child(div().min_w_0().whitespace_nowrap().child(StyledText::new(text.clone()).with_highlights(vec![(hit.clone(), hl)])))
                    .into_any_element()
            }
        }).collect::<Vec<_>>()
    }).track_scroll(&st.scroll_tree).size_full().into_any_element()
}

/// 左边的代码预览(虚拟列表):语法高亮与引用底色在载入时已算好,这里只按可见范围画行。
fn preview_pane(st: &PeekState, c: &Colors, lh: f32, cx: &mut Context<IdeView>) -> AnyElement {
    let Some(p) = &st.preview else {
        return div().p_3().text_size(px(12.)).text_color(c.title_desc).child("没有可用的预览").into_any_element();
    };
    let lines: Arc<Vec<PLine>> = p.lines.clone();
    let (fg, muted) = (cx.theme().foreground, cx.theme().muted_foreground);
    uniform_list("peek-preview-list", lines.len(), move |range, _, _| {
        range.filter_map(|ix| lines.get(ix)).map(|l| {
            h_flex().h(px(lh)).w_full().items_center()
                .child(div().w(px(48.)).flex_none().pr_3().text_right().text_size(px(12.)).text_color(muted).font_family(MONO).child((l.ln + 1).to_string()))
                .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_size(px(13.)).text_color(fg).font_family(MONO)
                    .child(StyledText::new(if l.text.is_empty() { " ".to_string() } else { l.text.clone() }).with_highlights(l.hl.clone())))
        }).collect::<Vec<_>>()
    }).track_scroll(&st.scroll_preview).size_full().into_any_element()
}
