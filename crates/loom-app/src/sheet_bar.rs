//! 表格视图的编辑栏(选中单元格的引用与完整文本、查找 / 筛选开关)与底栏(工作表切换、行列统计)。

use crate::sheet_view::SheetView;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_sheet::{Kind, column_letters};

impl SheetView {
    /// 顶部编辑栏:选中单元格的引用(如 B12)与完整文本;右侧是查找、筛选、首行表头与清除筛选。
    pub(crate) fn top_bar(&self, cx: &mut Context<Self>) -> Div {
        let t = cx.theme().clone();
        let (reference, text) = match self.selected {
            Some((r, c)) => (format!("{}{}", column_letters(c), r + 1), self.sheet.text(r, c).to_string()),
            None => (String::new(), String::new()),
        };
        let btn = |id: &'static str, label: &'static str, on: bool| {
            let b = Button::new(id).xsmall().label(label);
            if on { b.primary() } else { b.ghost() }
        };
        h_flex().h(px(34.)).w_full().flex_none().px_2().gap_2().items_center().border_b_1().border_color(t.border).bg(t.sidebar)
            .child(div().w(px(56.)).flex_none().px_2().py_0p5().rounded(px(4.)).border_1().border_color(t.border).bg(t.background).text_size(px(12.)).font_family(crate::ui::MONO).text_color(t.muted_foreground).child(reference))
            .child(div().flex_1().min_w_0().truncate().text_size(px(12.5)).child(text))
            .child(btn("sheet-find", "查找", self.find_open).on_click(cx.listener(|this, _, window, cx| this.toggle_find(window, cx))))
            .child(btn("sheet-filter", "筛选", self.filter_mode).tooltip("在列头显示筛选 / 排序按钮").on_click(cx.listener(|this, _, _, cx| { this.filter_mode = !this.filter_mode; cx.notify(); })))
            .child(btn("sheet-header", "首行表头", self.filter.first_row_header).tooltip("首行始终在最前,不参与筛选与排序").on_click(cx.listener(|this, _, _, cx| { this.filter.first_row_header = !this.filter.first_row_header; this.recompute(cx); })))
            .when(self.filter.is_active(), |d| d.child(Button::new("sheet-clear").xsmall().ghost().label("清除筛选").on_click(cx.listener(|this, _, _, cx| { this.filter.clear_filters(); this.filter.sort = None; this.recompute(cx); }))))
    }

    /// 底栏:工作表切换(多于一张才显示)+ 行列统计 + 截断提示;分隔文本可「以文本打开」。
    pub(crate) fn footer(&self, cx: &mut Context<Self>) -> Div {
        let t = cx.theme().clone();
        let total = self.sheet.rows.len();
        let shown = self.visible.len();
        let stats = if self.filter.is_active() && shown != total { format!("显示 {shown} / {total} 行 × {} 列", self.sheet.column_count) } else { format!("{total} 行 × {} 列", self.sheet.column_count) };
        h_flex().h(px(28.)).w_full().flex_none().px_2().gap_2().items_center().border_t_1().border_color(t.border).bg(t.sidebar).text_size(px(11.5)).text_color(t.muted_foreground)
            .child(
                h_flex().flex_1().min_w_0().gap_0p5().overflow_hidden().children(if self.names.len() > 1 { self.names.clone() } else { Vec::new() }.into_iter().enumerate().map(|(i, name)| {
                    let on = i == self.active;
                    div().id(("sheet-tab", i)).px_2().h(px(20.)).flex().items_center().rounded(px(4.)).cursor_pointer().whitespace_nowrap()
                        .when(on, |d| d.bg(t.background).text_color(t.foreground).font_weight(FontWeight::MEDIUM)).when(!on, |d| d.hover(|d| d.bg(t.list_hover)))
                        .on_click(cx.listener(move |this, _, _, cx| this.select_sheet(i, cx))).child(name)
                })),
            )
            .when(self.sheet.truncated, |d| d.child(div().text_color(t.warning).child(format!("已截断(最多 {} 行 × {} 列)", loom_sheet::MAX_ROWS, loom_sheet::MAX_COLUMNS))))
            .child(div().child(stats))
            .when(self.kind == Kind::Delimited, |d| d.child(Button::new("sheet-as-text").xsmall().ghost().label("以文本打开").on_click(cx.listener(|_, _, _, cx| cx.emit(crate::sheet_view::SheetEvent::OpenAsText)))))
    }
}
