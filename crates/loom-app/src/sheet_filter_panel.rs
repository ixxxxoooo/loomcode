//! 列筛选面板(点列头的按钮弹出):排序、按值勾选(带出现次数)、全选、清除;改动即时生效。

use crate::sheet_view::SheetView;
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_sheet::{Sort, column_letters};
use std::collections::HashSet;

/// 面板里最多列出的候选值数。
const MAX_VALUES: usize = 300;

impl SheetView {
    /// 打开着的筛选面板(没有打开返回 `None`):锚在点击位置,点外部或 Esc 关闭。
    pub(crate) fn filter_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (col, pos) = self.panel?;
        let t = cx.theme().clone();
        let values = self.filter.distinct_values(&self.sheet, col);
        let allowed: Option<HashSet<String>> = self.filter.filters.get(&col).cloned();
        let sorted = self.filter.sort.filter(|s| s.column == col);
        let title = {
            let head = if self.filter.first_row_header { self.sheet.text(0, col) } else { "" };
            if head.is_empty() { format!("列 {}", column_letters(col)) } else { format!("{} · {}", column_letters(col), head) }
        };
        let sort_btn = |id: &'static str, label: &'static str, asc: bool, cx: &mut Context<Self>| {
            let on = sorted.is_some_and(|s| s.ascending == asc);
            div().id(id).flex_1().h(px(24.)).flex().items_center().justify_center().rounded(px(5.)).cursor_pointer().text_size(px(12.))
                .when(on, |d| d.bg(t.primary.opacity(0.15)).text_color(t.primary)).when(!on, |d| d.hover(|d| d.bg(t.list_hover)))
                .on_click(cx.listener(move |this, _, _, cx| { this.filter.sort = if on { None } else { Some(Sort { column: col, ascending: asc }) }; this.recompute(cx); }))
                .child(label)
        };
        let all_values = values.clone();
        let rows = values.iter().take(MAX_VALUES).enumerate().map(|(i, v)| {
            let checked = allowed.as_ref().is_none_or(|a| a.contains(&v.text));
            let (text, avail) = (v.text.clone(), all_values.clone());
            h_flex().id(("sheet-fv", i)).h(px(24.)).px_2().gap_2().items_center().rounded(px(4.)).cursor_pointer().hover(|d| d.bg(t.list_hover))
                .on_click(cx.listener(move |this, _, _, cx| { this.filter.toggle(col, &text, &avail); this.recompute(cx); }))
                .child(div().w(px(14.)).flex_none().child(if checked { crate::ui::icon(IconName::Check, 12., t.primary).into_any_element() } else { div().into_any_element() }))
                .child(div().flex_1().min_w_0().truncate().text_size(px(12.)).child(if v.text.is_empty() { "(空白)".to_string() } else { v.text.clone() }))
                .child(div().text_size(px(11.)).text_color(t.muted_foreground).child(v.count.to_string()))
        }).collect::<Vec<_>>();
        let (asc_btn, desc_btn) = (sort_btn("sheet-sort-asc", "升序", true, cx), sort_btn("sheet-sort-desc", "降序", false, cx));
        let more = values.len().saturating_sub(MAX_VALUES);
        let avail_all = values.clone();
        Some(
            deferred(
                anchored().position(pos).anchor(Anchor::TopLeft).snap_to_window_with_margin(px(8.)).child(
                    v_flex().id("sheet-filter-panel").w(px(260.)).max_h(px(380.)).p_2().gap_1().rounded(px(8.)).border_1().border_color(t.border).bg(t.popover).shadow_md().occlude()
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| { this.panel = None; cx.notify(); }))
                        .child(div().px_2().text_size(px(12.)).font_weight(FontWeight::MEDIUM).truncate().child(title))
                        .child(h_flex().gap_1().child(asc_btn).child(desc_btn))
                        .child(div().h(px(1.)).bg(t.border).my_0p5())
                        .child(div().id("sheet-fv-list").flex_1().min_h_0().overflow_y_scroll().children(rows).when(more > 0, |d| d.child(div().px_2().py_1().text_size(px(11.)).text_color(t.muted_foreground).child(format!("… 还有 {more} 个值未列出")))))
                        .child(
                            h_flex().gap_1().pt_1().child(
                                div().id("sheet-fv-all").flex_1().h(px(24.)).flex().items_center().justify_center().rounded(px(5.)).cursor_pointer().text_size(px(12.)).hover(|d| d.bg(t.list_hover))
                                    .on_click(cx.listener(move |this, _, _, cx| { this.filter.set_allowed(col, avail_all.iter().map(|v| v.text.clone()).collect(), &avail_all, ); this.recompute(cx); })).child("全选"),
                            ).child(
                                div().id("sheet-fv-close").flex_1().h(px(24.)).flex().items_center().justify_center().rounded(px(5.)).cursor_pointer().text_size(px(12.)).hover(|d| d.bg(t.list_hover))
                                    .on_click(cx.listener(|this, _, _, cx| { this.panel = None; cx.notify(); })).child("完成"),
                            ),
                        ),
                ),
            )
            .into_any_element(),
        )
    }
}
