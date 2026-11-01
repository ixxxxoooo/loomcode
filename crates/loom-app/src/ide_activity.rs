//! IDE 窗口的活动栏(文件 / 搜索 / 改动 / 终端 / 计划 + 设置)。
//!
//! @author ixxxxoooo

use crate::ide_window::IdeWindow;
use crate::workbench::{Page, RightTab};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Selectable, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// 活动栏条目:(图标, 提示, 对应的面板分段)。
const ACTIVITIES: [(IconName, &str, RightTab); 4] = [
    (IconName::Folder, "文件", RightTab::Files),
    (IconName::GitBranch, "改动", RightTab::Changes),
    (IconName::Terminal, "终端", RightTab::Terminal),
    (IconName::ListTodo, "计划", RightTab::Plan),
];

impl IdeWindow {
    /// 切到左栏的「搜索」并聚焦输入框(⌘⇧F)。
    pub(crate) fn show_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.left_open = true;
        self.search_open = true;
        let input = self.search.read(cx).input.clone();
        input.update(cx, |i, cx| i.focus(window, cx));
        cx.notify();
    }

    /// 最左侧的窄活动栏:文件 / 搜索 / 改动 / 终端 / 计划,选中项左侧有竖条(与 VSCode 一致),底部是设置。
    pub(crate) fn activity_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let cur = self.panel.read(cx).tab;
        let (left_open, search_open, primary) = (self.left_open, self.search_open, t.primary);
        // 活动栏的一项:图标按钮 + 选中时左侧竖条
        let item = move |id: SharedString, ic: IconName, tip: &'static str, active: bool| {
            div().relative().w_full().flex().justify_center()
                .when(active, |d| d.child(div().absolute().left_0().top(px(6.)).bottom(px(6.)).w(px(2.)).rounded_r(px(2.)).bg(primary)))
                .child(Button::new(id).ghost().icon(ic).tooltip(tip).selected(active))
        };
        let mut col = v_flex().w(px(44.)).h_full().flex_none().items_center().pt_2().gap_1().border_r_1().border_color(t.border).bg(t.sidebar);
        for (ic, tip, tab) in ACTIVITIES.into_iter().take(1) {
            let active = left_open && !search_open && cur == tab;
            col = col.child(div().w_full().on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _, cx| {
                this.left_open = !(this.left_open && !this.search_open && this.panel.read(cx).tab == tab);
                this.search_open = false;
                this.panel.update(cx, |p, cx| p.set_tab(tab, cx));
                cx.notify();
            })).child(item(SharedString::from(format!("act-{}", tab.key())), ic, tip, active)));
        }
        col = col.child(div().w_full().on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
            this.left_open = !(this.left_open && this.search_open);
            this.search_open = true;
            cx.notify();
        })).child(item("act-search".into(), IconName::Search, "搜索(⇧⌘F)", left_open && search_open)));
        for (ic, tip, tab) in ACTIVITIES.into_iter().skip(1) {
            // 「终端」不在左栏显示,而是开关底部终端面板(和 VS Code 一致)
            if tab == RightTab::Terminal {
                let active = self.term_open;
                col = col.child(div().w_full().on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| this.toggle_term(window, cx)))
                    .child(item("act-terminal".into(), ic, "终端(⌃`)", active)));
                continue;
            }
            let active = left_open && !search_open && cur == tab;
            col = col.child(div().w_full().on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _, cx| {
                this.left_open = !(this.left_open && !this.search_open && this.panel.read(cx).tab == tab);
                this.search_open = false;
                this.panel.update(cx, |p, cx| p.set_tab(tab, cx));
                cx.notify();
            })).child(item(SharedString::from(format!("act-{}", tab.key())), ic, tip, active)));
        }
        col.child(div().flex_1()).child(div().pb_2().child(
            Button::new("act-settings").ghost().icon(IconName::Settings).tooltip("设置").on_click(cx.listener(|this, _, _, cx| {
                this.wb.update(cx, |wb, cx| wb.set_page(Page::Settings, cx));
                this.back_to_agent(cx);
            })),
        ))
    }
}
