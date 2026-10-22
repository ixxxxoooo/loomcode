//! 右侧固定面板:改动 / 文件 / 终端 / 计划 / 输出,分段切换,同一时刻只显示一个。

use crate::ui::{empty_state, icon};
use crate::workbench::{RightTab, Workbench};
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

pub fn render(wb: &Workbench, cx: &mut Context<Workbench>) -> impl IntoElement {
    let t = cx.theme().clone();
    let tab = wb.right_tab;
    v_flex().w(px(wb.right_w)).h_full().flex_none().border_l_1().border_color(t.border).bg(t.background)
        .child(
            h_flex().h(px(36.)).px_2().gap_0p5().items_center().border_b_1().border_color(t.border)
                .children(RightTab::ALL.into_iter().map(|rt| {
                    let on = rt == tab;
                    h_flex().id(SharedString::from(format!("rt-{}", rt.label()))).h(px(26.)).px_2().gap_1().items_center().rounded(px(6.)).cursor_pointer()
                        .text_size(px(12.))
                        .text_color(if on { t.foreground } else { t.muted_foreground })
                        .when(on, |d| d.bg(t.muted))
                        .hover(|d| d.bg(t.list_hover))
                        .on_click(cx.listener(move |this, _, _, cx| { this.right_tab = rt; cx.notify(); }))
                        .child(icon(rt.icon(), 12., if on { t.foreground } else { t.muted_foreground }))
                        .child(rt.label())
                })),
        )
        .child(div().flex_1().min_h_0().child(empty_state(tab.icon(), tab.label(), "稍后实现", cx)))
}
