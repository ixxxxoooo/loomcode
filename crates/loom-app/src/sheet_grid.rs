//! 表格网格:列头(A、B、C…,筛选模式下带筛选按钮)固定在顶部,行用虚拟列表按需创建;
//! 单元格可选中,查找命中与选中有不同底色。

use crate::sheet_view::SheetView;
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_sheet::{CellKind, Sheet, column_letters};
use std::collections::HashSet;
use std::sync::Arc;

/// 行号栏宽度。
const GUTTER_W: f32 = 52.;
/// 行高 / 列头高。
const ROW_H: f32 = 24.;
const HEAD_H: f32 = 26.;

/// 画一行所需的只读数据(从视图克隆的 `Arc`,供虚拟列表的闭包使用)。
#[derive(Clone)]
struct RowCtx {
    sheet: Arc<Sheet>,
    visible: Arc<Vec<usize>>,
    widths: Arc<Vec<f32>>,
    matches: Arc<HashSet<(usize, usize)>>,
    current: Option<(usize, usize)>,
    selected: Option<(usize, usize)>,
    header: bool,
    view: WeakEntity<SheetView>,
}

impl SheetView {
    /// 就绪状态的整体布局:编辑栏、查找条、网格、底栏、筛选面板。
    pub(crate) fn ready_view(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        v_flex().size_full().min_h_0().relative()
            .child(self.top_bar(cx))
            .when(self.find_open, |d| d.child(self.find_bar(cx)))
            .child(self.grid(cx))
            .child(self.footer(cx))
            .children(self.filter_panel(cx))
            .into_any_element()
    }

    /// 网格:整体可横向滚动,列头在上,行在下(虚拟列表)。
    fn grid(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let total_w = GUTTER_W + self.widths.iter().sum::<f32>();
        let current = self.matches.get(self.current_match).map(|m| (m.row, m.col));
        let ctx = RowCtx {
            sheet: self.sheet.clone(), visible: self.visible.clone(), widths: self.widths.clone(), matches: self.match_set.clone(), current,
            selected: self.selected, header: self.filter.first_row_header, view: cx.weak_entity(),
        };
        let count = ctx.visible.len();
        if self.sheet.rows.is_empty() {
            return crate::ui::empty_state(IconName::FileText, "空表格", "这个工作表里没有数据", cx).into_any_element();
        }
        div().id("sheet-x").flex_1().min_h_0().w_full().overflow_x_scroll().child(
            v_flex().w(px(total_w)).h_full()
                .child(self.header_row(cx))
                .child(uniform_list("sheet-rows", count, move |range, _, cx| range.map(|i| row_el(&ctx, i, cx)).collect::<Vec<_>>()).track_scroll(&self.scroll).flex_1().min_h_0().w_full())
                .when(count == 0, |d| d.child(div().p_3().text_size(px(12.)).text_color(t.muted_foreground).child("没有符合筛选条件的行"))),
        ).into_any_element()
    }

    /// 列头行:左上角空格 + 各列字母(筛选模式或该列有筛选 / 排序时带按钮)。
    fn header_row(&self, cx: &mut Context<Self>) -> Div {
        let t = cx.theme().clone();
        let mut row = h_flex().h(px(HEAD_H)).flex_none().bg(t.sidebar).border_b_1().border_color(t.border)
            .child(div().w(px(GUTTER_W)).h_full().flex_none().border_r_1().border_color(t.border));
        for (c, w) in self.widths.iter().enumerate() {
            let sorted = self.filter.sort.filter(|s| s.column == c);
            let show = self.filter_mode || self.filter.is_filtered(c) || sorted.is_some();
            let active = self.filter.is_filtered(c) || sorted.is_some();
            row = row.child(
                h_flex().w(px(*w)).h_full().flex_none().px_2().items_center().justify_between().border_r_1().border_color(t.border).text_size(px(11.5)).text_color(t.muted_foreground)
                    .child(column_letters(c))
                    .when(show, |d| d.child(
                        div().id(("sheet-col-filter", c)).size(px(18.)).flex().items_center().justify_center().rounded(px(4.)).cursor_pointer()
                            .when(active, |d| d.bg(t.primary.opacity(0.15)).text_color(t.primary)).hover(|d| d.bg(t.list_hover))
                            .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| { this.panel = Some((c, ev.position())); cx.notify(); }))
                            .child(crate::ui::icon(match sorted { Some(s) if s.ascending => IconName::ArrowUp, Some(_) => IconName::ArrowDown, None => IconName::ChevronDown }, 11., if active { t.primary } else { t.muted_foreground })),
                    )),
            );
        }
        row
    }
}

/// 画第 `i` 个可见行。
fn row_el(ctx: &RowCtx, i: usize, cx: &mut App) -> AnyElement {
    let t = cx.theme().clone();
    let Some(&orig) = ctx.visible.get(i) else { return div().into_any_element() };
    let is_head = ctx.header && orig == 0;
    let mut row = h_flex().h(px(ROW_H)).w_full().flex_none()
        .child(div().w(px(GUTTER_W)).h_full().flex_none().px_2().flex().items_center().justify_end().bg(t.sidebar).border_r_1().border_b_1().border_color(t.border).text_size(px(11.)).text_color(t.muted_foreground).child((orig + 1).to_string()));
    for (c, w) in ctx.widths.iter().enumerate() {
        let cell = ctx.sheet.cell(orig, c);
        let (text, kind) = cell.map_or((String::new(), CellKind::Text), |x| (x.text.clone(), x.kind));
        let (selected, hit, cur) = (ctx.selected == Some((orig, c)), ctx.matches.contains(&(orig, c)), ctx.current == Some((orig, c)));
        let view = ctx.view.clone();
        row = row.child(
            div().id(("sheet-cell", c)).w(px(*w)).h_full().flex_none().px_2().flex().items_center().overflow_hidden().whitespace_nowrap()
                .border_r_1().border_b_1().border_color(t.border.opacity(0.6)).text_size(px(12.5))
                .when(kind == CellKind::Number, |d| d.justify_end().font_family(crate::ui::MONO))
                .when(kind == CellKind::Error, |d| d.text_color(t.danger))
                .when(is_head, |d| d.font_weight(FontWeight::MEDIUM).bg(t.muted.opacity(0.5)))
                .when(hit, |d| d.bg(t.warning.opacity(0.22)))
                .when(cur, |d| d.bg(t.warning.opacity(0.5)))
                .when(selected, |d| d.bg(t.primary.opacity(0.14)).border_1().border_color(t.primary))
                .on_click(move |_, window, cx| { let _ = view.update(cx, |this, cx| { this.selected = Some((orig, c)); this.focus.focus(window, cx); cx.notify(); }); })
                .child(text),
        );
    }
    row.into_any_element()
}
