//! 终端快捷键:⌃` 显示 / 隐藏终端,⌃⇧` 新建终端。
//!
//! @author ixxxxoooo

use crate::term_store::{ProjectTerminals, TermSel};
use crate::workbench::{Page, RightTab, Workbench};
use gpui_kit::*;

actions!(loomcode, [ToggleTerminal, NewTerminal]);

impl Workbench {
    /// ⌃`:终端已经显示就收起右侧面板;否则展开面板、切到终端分段并聚焦(没有终端就新建一个)。
    pub fn toggle_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page == Page::Chat && self.right_open && self.panel.read(cx).tab == RightTab::Terminal {
            self.right_open = false;
        } else {
            self.page = Page::Chat;
            self.right_open = true;
            self.panel.update(cx, |p, cx| {
                p.set_tab(RightTab::Terminal, cx);
                p.focus_terminal(window, cx);
            });
        }
        cx.notify();
    }

    /// ⌃⇧`:展开面板、切到终端分段并新建一个终端。
    pub fn open_new_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.page = Page::Chat;
        self.right_open = true;
        self.panel.update(cx, |p, cx| {
            p.set_tab(RightTab::Terminal, cx);
            p.new_terminal(window, cx);
        });
        cx.notify();
    }
}

impl crate::right_panel::RightPanel {
    /// 聚焦当前终端:当前选中的是终端就用它,否则用最后一个;一个都没有就新建。
    pub fn focus_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let hub = ProjectTerminals::hub(cx);
        let root = self.root.clone();
        let target = {
            let h = hub.read(cx);
            match h.active(&root) {
                TermSel::Term(id) => Some(id),
                TermSel::Nothing => h.tabs(&root).last().map(|t| t.id),
            }
        };
        let Some(id) = target else {
            self.new_terminal(window, cx);
            return;
        };
        hub.update(cx, |h, cx| h.activate(&root, TermSel::Term(id), cx));
        let handle = hub.read(cx).tabs(&root).iter().find(|t| t.id == id).map(|t| t.view.read(cx).focus_handle());
        if let Some(h) = handle {
            window.focus(&h, cx);
        }
    }
}
