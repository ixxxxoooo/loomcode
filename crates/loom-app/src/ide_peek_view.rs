//! 引用预览窗的渲染(尺寸与配色照 VS Code:`peekView*` 主题色、树行高 23、预览与树 7:3、18 行高)。

use crate::code_hl::{overlay_background, split_by_line, styles};
use crate::ide::IdeView;
use crate::ide_overlay::Overlay;
use crate::ide_peek::PeekState;
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
/// 预览里最多渲染目标行前后各多少行。
const WINDOW: usize = 400;

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
        .child(div().id("peek-tree").flex_1().min_w_0().h_full().overflow_y_scroll().bg(c.result_bg).child(tree_pane(st, &c, cx)));
    v_flex().w(px(width)).border_t_1().border_b_1().border_color(c.border).shadow_lg().occlude()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(head).child(body).into_any_element()
}

/// 右边的引用树:单文件时直接列引用;多文件时按文件分组(文件名 + 目录 + 引用数徽标)。
fn tree_pane(st: &PeekState, c: &Colors, cx: &mut Context<IdeView>) -> AnyElement {
    let multi = st.file_count() > 1;
    let mut col = v_flex().w_full().text_size(px(13.));
    let mut last = None;
    for (i, it) in st.items.iter().enumerate() {
        if multi && last != Some(&it.path) {
            last = Some(&it.path);
            let fname = it.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let dir = it.path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let count = st.items.iter().filter(|x| x.path == it.path).count();
            col = col.child(
                h_flex().h(px(ROW_H)).w_full().flex_none().px_2().gap_1p5().items_center().text_color(c.result_file)
                    .child(crate::file_icons::file_icon_img(&fname, false, false, 14.))
                    .child(div().flex_none().child(fname))
                    .child(div().flex_1().min_w_0().truncate().text_size(px(11.7)).opacity(0.7).child(dir))
                    .child(div().mr(px(12.)).px_1p5().rounded_full().bg(c.selection).text_size(px(11.)).child(count.to_string())),
            );
        }
        let sel = i == st.selected;
        let hl = HighlightStyle { color: Some(c.result_file), background_color: Some(c.result_match), ..Default::default() };
        col = col.child(
            h_flex().id(("peek-ref", i)).h(px(ROW_H)).w_full().flex_none().when(multi, |d| d.pl(px(26.))).when(!multi, |d| d.pl_2()).pr_2().items_center().cursor_pointer().overflow_hidden()
                .text_color(c.result_line).when(sel, |d| d.bg(c.selection).text_color(c.result_file))
                .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| if ev.click_count() >= 2 { this.peek_open(i, window, cx) } else { this.peek_select(i, cx) }))
                .child(div().min_w_0().whitespace_nowrap().child(StyledText::new(it.text.clone()).with_highlights(vec![(it.hit.clone(), hl)]))),
        );
    }
    col.into_any_element()
}

/// 左边的代码预览:选中引用所在文件(目标行前后各 `WINDOW` 行),语法高亮、行号、本文件里所有引用加底色、目标行滚到中间。
fn preview_pane(st: &PeekState, c: &Colors, lh: f32, cx: &mut Context<IdeView>) -> AnyElement {
    let (Some(p), Some(it)) = (&st.preview, st.items.get(st.selected)) else {
        return div().p_3().text_size(px(12.)).text_color(c.title_desc).child("没有可用的预览").into_any_element();
    };
    let first = (it.line as usize).saturating_sub(WINDOW);
    let last = (it.line as usize + WINDOW).min(p.lines.len());
    let slice: Vec<&str> = p.lines[first..last].iter().map(String::as_str).collect();
    let all = styles(&slice.join("\n"), crate::file_tree::language_for(&p.path), cx);
    let per = split_by_line(&slice, &all);
    let fg = cx.theme().foreground;
    let muted = cx.theme().muted_foreground;
    let rows = slice.iter().enumerate().map(|(k, l)| {
        let ln = first + k;
        let mut hl = per[k].clone();
        // 本文件里的每处引用都加底色(VS Code 的 reference-decoration)
        for r in st.items.iter().filter(|r| r.path == p.path && r.line as usize == ln) {
            let (a, b) = (l.char_indices().nth(r.col as usize).map_or(l.len(), |(i, _)| i), l.char_indices().nth(r.end_col as usize).map_or(l.len(), |(i, _)| i));
            hl = overlay_background(&hl, a..b.max(a), c.editor_match);
        }
        h_flex().h(px(lh)).flex_none().items_center()
            .child(div().w(px(48.)).flex_none().pr_3().text_right().text_size(px(12.)).text_color(muted).font_family(MONO).child((ln + 1).to_string()))
            .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_size(px(13.)).text_color(fg).font_family(MONO).child(StyledText::new(if l.is_empty() { " ".to_string() } else { (*l).to_string() }).with_highlights(hl)))
    }).collect::<Vec<_>>();
    // 目标行滚到视口中间:往上留约半个窗口的行数
    if st.scrolled_for.get() != st.selected {
        st.scrolled_for.set(st.selected);
        st.scroll.scroll_to_top_of_item((it.line as usize - first).saturating_sub((HEIGHT_LINES / 2.) as usize));
    }
    div().id("peek-preview").size_full().overflow_y_scroll().track_scroll(&st.scroll).children(rows).into_any_element()
}
