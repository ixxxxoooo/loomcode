//! IDE 窗口的活动栏(文件 / 搜索 / 改动 + 设置)。
//!
//! @author ixxxxoooo

use crate::ide_window::IdeWindow;
use crate::workbench::RightTab;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Selectable, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// 活动栏条目:(图标, 提示, 对应的面板分段)。
/// 终端开关在标题栏右上角(PanelBottom),计划 / 待办跟随对话栏的时间线(业界做法:Cursor、VS Code Copilot、Claude Code 扩展),都不放活动栏。
const ACTIVITIES: [(IconName, &str, RightTab); 2] = [
    (IconName::Folder, "文件", RightTab::Files),
    (IconName::GitBranch, "源代码管理(⌃⇧G)", RightTab::Changes),
];

impl IdeWindow {
    /// ⌃⇧G:切到左栏的「源代码管理」(和 VS Code 一致)。
    pub(crate) fn show_scm(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.left_open = true;
        self.search_open = false;
        self.panel.update(cx, |p, cx| p.set_tab(RightTab::Changes, cx));
        cx.notify();
    }

    /// 切到左栏的「搜索」并聚焦输入框(⌘⇧F)。
    pub(crate) fn show_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.left_open = true;
        self.search_open = true;
        // 编辑器里有单行选区就拿它当搜索词(对标 VS Code 的 seedSearchStringFromSelection)
        if let Some(text) = self.ide.read(cx).selected_text(cx) {
            self.search.update(cx, |s, cx| s.seed(text, window, cx));
        }
        let input = self.search.read(cx).input.clone();
        input.update(cx, |i, cx| i.focus(window, cx));
        cx.notify();
    }

    /// Esc:关闭搜索面板(回到文件树),焦点还给编辑器。
    pub(crate) fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.search_open {
            return;
        }
        self.search_open = false;
        self.ide.update(cx, |ide, cx| ide.focus_active(window, cx));
        cx.notify();
    }

    /// 最左侧的窄活动栏:文件 / 搜索 / 改动,选中项左侧有竖条(与 VSCode 一致),底部是设置。
    pub(crate) fn activity_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let cur = self.panel.read(cx).tab;
        let (left_open, search_open, primary) = (self.left_open, self.search_open, t.primary);
        // 活动栏的一项:图标按钮 + 选中时左侧竖条
        let scm_count = self.panel.read(cx).scm.read(cx).count(cx);
        let badge_bg = t.primary;
        let badge_fg = t.primary_foreground;
        let item = move |id: SharedString, ic: IconName, tip: &'static str, active: bool| {
            // 源代码管理图标右上角显示改动文件数(和 VS Code 一致)
            let badge = (ic == IconName::GitBranch && scm_count > 0).then(|| {
                div().absolute().right(px(4.)).top(px(2.)).min_w(px(15.)).h(px(15.)).px(px(3.)).flex().items_center().justify_center().rounded(px(8.)).bg(badge_bg).text_color(badge_fg).text_size(px(9.5)).font_weight(FontWeight::BOLD)
                    .child(if scm_count > 99 { "99+".to_string() } else { scm_count.to_string() })
            });
            div().relative().w_full().flex().justify_center()
                .when(active, |d| d.child(div().absolute().left_0().top(px(6.)).bottom(px(6.)).w(px(2.)).rounded_r(px(2.)).bg(primary)))
                .child(Button::new(id).ghost().icon(ic).tooltip(tip).selected(active))
                .children(badge)
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
            let active = left_open && !search_open && cur == tab;
            col = col.child(div().w_full().on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _, cx| {
                this.left_open = !(this.left_open && !this.search_open && this.panel.read(cx).tab == tab);
                this.search_open = false;
                this.panel.update(cx, |p, cx| p.set_tab(tab, cx));
                cx.notify();
            })).child(item(SharedString::from(format!("act-{}", tab.key())), ic, tip, active)));
        }
        col.child(div().flex_1()).child(div().pb_2().child(
            Button::new("act-settings").ghost().icon(IconName::Settings).tooltip("设置").on_click(cx.listener(|this, _, window, cx| {
                // 在当前窗口里打开:独立 IDE 窗口里整窗显示设置;融合模式是整窗页面,关掉后回到 IDE
                this.wb.update(cx, |wb, cx| wb.open_settings_here(window, cx));
            })),
        ))
    }
}
