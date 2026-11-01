//! 右面板收起时,悬浮在对话区右上角的快捷竖条(参考 OhMyAgent / Cursor):
//! 一键展开右栏,或直接跳到「改动 / 文件 / 终端」标签。
//!
//! @author ixxxxoooo

use crate::ui::icon;
use crate::window_header::HEADER_H;
use crate::workbench::{RightTab, Workbench};
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// 竖条里的快捷标签。
const SHORTCUTS: [(IconName, &str, RightTab); 3] = [
    (IconName::GitBranch, "改动与 Git 更改", RightTab::Changes),
    (IconName::Folder, "文件", RightTab::Files),
    (IconName::Terminal, "终端", RightTab::Terminal),
];

impl Workbench {
    /// 快捷竖条。展开右栏的按钮在最上面;`changes` 为 Claude 已改动的文件数,大于 0 时「改动」右上角有圆点。
    pub(crate) fn right_rail(&self, changes: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let button = |id: &'static str, ic: IconName, tip: &'static str, dot: bool| {
            h_flex().id(id).relative().size(px(28.)).items_center().justify_center().rounded(px(6.)).cursor_pointer()
                .hover(|d| d.bg(t.list_hover))
                .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tip).build(window, cx))
                .child(icon(ic, 14., t.muted_foreground))
                .when(dot, |d| d.child(div().absolute().top(px(4.)).right(px(4.)).size(px(7.)).rounded_full().bg(t.primary)))
        };
        v_flex().absolute().top(px(HEADER_H + 8.)).right(px(10.)).p_1().gap_0p5().rounded(px(10.)).border_1().border_color(t.border).bg(t.popover).shadow_md()
            .child(button("rail-expand", IconName::PanelRight, "展开右侧面板(⌥⌘B)", false)
                .on_click(cx.listener(|this, _, _, cx| { this.right_open = true; cx.notify(); })))
            .children(SHORTCUTS.into_iter().map(|(ic, tip, tab)| {
                button(tab.key(), ic, tip, tab == RightTab::Changes && changes > 0).on_click(cx.listener(move |this, _, _, cx| {
                    this.right_open = true;
                    this.panel.update(cx, |p, cx| p.open_tab(tab, cx));
                    cx.notify();
                }))
            }))
    }
}
