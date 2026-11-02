//! IDE 窗口底部的终端面板:当前项目的终端标签条 + 当前终端。
//! 终端本体来自全局的 `ProjectTerminals`,和 Agent 窗口右侧面板共用同一份。
//!
//! @author ixxxxoooo

use crate::ide_window::IdeWindow;
use crate::right_panel::RightPanel;
use crate::term_store::{ProjectTerminals, TermSel};
use crate::terminal_view::TerminalView;
use crate::ui::empty_state;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, v_flex};
use gpui_kit::*;

/// 底部终端面板。
pub struct BottomTerminal {
    /// 用它的根目录决定显示哪个项目的终端(与文件面板同步)
    panel: Entity<RightPanel>,
    _subs: Vec<Subscription>,
}

impl BottomTerminal {
    pub fn new(panel: Entity<RightPanel>, cx: &mut Context<Self>) -> Self {
        let hub = ProjectTerminals::hub(cx);
        let subs = vec![cx.observe(&hub, |_, _, cx| cx.notify()), cx.observe(&panel, |_, _, cx| cx.notify())];
        Self { panel, _subs: subs }
    }

    fn root(&self, cx: &App) -> String {
        self.panel.read(cx).root.clone()
    }

    /// 聚焦当前终端:选中的是终端就用它,否则用最后一个;一个都没有就新建。
    pub fn focus_or_spawn(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let hub = ProjectTerminals::hub(cx);
        let root = self.root(cx);
        let target = {
            let h = hub.read(cx);
            match h.active(&root) {
                TermSel::Term(id) => Some(id),
                TermSel::Nothing => h.tabs(&root).last().map(|t| t.id),
            }
        };
        match target {
            Some(id) => {
                hub.update(cx, |h, cx| h.activate(&root, TermSel::Term(id), cx));
                self.focus_tab(&hub, &root, id, window, cx);
            }
            None => self.new_terminal(window, cx),
        }
    }

    /// 新建一个终端并聚焦。
    pub fn new_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let hub = ProjectTerminals::hub(cx);
        let root = self.root(cx);
        match hub.update(cx, |h, cx| h.spawn(&root, None, cx)) {
            Ok(id) => self.focus_tab(&hub, &root, id, window, cx),
            Err(e) => loom_diag::error!("打开终端失败:{e}"),
        }
    }

    fn focus_tab(&self, hub: &Entity<ProjectTerminals>, root: &str, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let handle = hub.read(cx).tabs(root).iter().find(|t| t.id == id).map(|t| t.view.read(cx).focus_handle());
        if let Some(h) = handle {
            window.focus(&h, cx);
        }
    }
}

impl Render for BottomTerminal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let hub = ProjectTerminals::hub(cx);
        let root = self.root(cx);
        let folder = root.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string();
        let (active, tabs): (TermSel, Vec<(u64, String, Option<u32>, Entity<TerminalView>)>) = {
            let h = hub.read(cx);
            let tabs = h.tabs(&root).iter().map(|tab| {
                let v = tab.view.read(cx);
                (tab.id, v.label(&folder), v.exit_code(), tab.view.clone())
            }).collect();
            (h.active(&root), tabs)
        };
        // 「Agent 命令」在这里没有对应标签:当前选中它时就显示最后一个终端
        let shown = match active {
            TermSel::Term(id) => Some(id),
            TermSel::Nothing => tabs.last().map(|x| x.0),
        };
        let items: Vec<AnyElement> = tabs.iter().map(|(id, title, exit, _)| {
            let id = *id;
            let (hub_a, root_a, hub_c, root_c) = (hub.clone(), root.clone(), hub.clone(), root.clone());
            crate::term_tabs::term_tab("bt", id, title, shown == Some(id), exit.is_some(),
                move |_, _, cx| hub_a.update(cx, |h, cx| h.activate(&root_a, TermSel::Term(id), cx)),
                move |_, cx| hub_c.update(cx, |h, cx| h.close(&root_c, id, cx)), cx).into_any_element()
        }).collect();
        let label = div().px_3().text_size(px(12.)).text_color(t.muted_foreground).child("终端").into_any_element();
        let new_btn = Button::new("bt-new").small().ghost().icon(IconName::Plus).tooltip("新建终端(⌃⇧`)")
            .on_click(cx.listener(|this, _, window, cx| this.new_terminal(window, cx))).into_any_element();
        let strip = crate::term_tabs::tab_strip("bt-strip", vec![label], items, new_btn, cx);
        let body = match shown.and_then(|id| tabs.iter().find(|x| x.0 == id)) {
            Some((_, _, _, view)) => div().size_full().child(view.clone()).into_any_element(),
            None => empty_state(IconName::Terminal, "还没有终端", "点上方「+」新建一个", cx).into_any_element(),
        };
        v_flex().size_full().bg(t.background).child(strip).child(div().flex_1().min_h_0().child(body))
    }
}

impl IdeWindow {
    /// ⌃` / 活动栏「终端」:显示或收起底部终端面板;显示时聚焦终端(没有就新建一个)。
    pub fn toggle_term(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.term_open = !self.term_open;
        if self.term_open {
            self.term.update(cx, |t, cx| t.focus_or_spawn(window, cx));
        } else {
            // 收起后焦点回到窗口根,否则下次 ⌃` 找不到处理者
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    /// ⌃⇧`:展开底部面板并新建一个终端。
    pub fn open_new_term(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.term_open = true;
        self.term.update(cx, |t, cx| t.new_terminal(window, cx));
        cx.notify();
    }
}
