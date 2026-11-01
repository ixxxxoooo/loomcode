//! 设置页公共控件:设置行、分段选择器、标题。
//!
//! @author ixxxxoooo

use crate::ui::caption;
use crate::workbench::Workbench;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// 一行设置:左标题与说明,右控件。
///
/// 参数:`title` 标题;`desc` 说明(可空);`control` 右侧控件。返回:行元素。
pub fn row(title: &str, desc: &str, control: impl IntoElement, cx: &App) -> Div {
    let t = cx.theme();
    h_flex().w_full().py_3().gap_4().items_center().border_b_1().border_color(t.border)
        .child(v_flex().flex_1().min_w_0().gap_0p5()
            .child(div().text_size(px(13.5)).child(title.to_string()))
            .when(!desc.is_empty(), |d| d.child(caption(desc.to_string(), cx))))
        .child(div().flex_none().child(control))
}

/// 分段选择器。
///
/// 参数:`id` 元素 id 前缀;`options` (值, 标签) 列表;`current` 当前值;`on` 选中回调。返回:选择器元素。
pub fn segmented<T: Copy + PartialEq + 'static>(
    id: &'static str,
    options: Vec<(T, &'static str)>,
    current: T,
    cx: &mut Context<Workbench>,
    on: impl Fn(&mut Workbench, T, &mut Context<Workbench>) + Clone + 'static,
) -> Div {
    let t = cx.theme().clone();
    h_flex().p(px(2.)).gap_0p5().rounded(px(8.)).bg(t.muted)
        .children(options.into_iter().enumerate().map(|(i, (v, label))| {
            let on = on.clone();
            let active = v == current;
            div().id(SharedString::from(format!("{id}-{i}"))).px_2p5().h(px(24.)).flex().items_center().rounded(px(6.)).cursor_pointer().text_size(px(12.))
                .when(active, |d| d.bg(t.background).shadow_sm().text_color(t.foreground))
                .when(!active, |d| d.text_color(t.muted_foreground).hover(|d| d.text_color(t.foreground)))
                .on_click(cx.listener(move |this, _, _, cx| on(this, v, cx)))
                .child(label)
        }))
}

/// 分组小标题。
pub fn section_title(text: &str, cx: &App) -> Div {
    div().pt_5().pb_1().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(cx.theme().muted_foreground).child(text.to_string())
}

/// 页面大标题。
pub fn page_title(text: &str) -> Div {
    div().text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).pb_1().child(text.to_string())
}
