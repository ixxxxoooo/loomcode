//! IDE 独立窗口:文件树 + 单文件编辑器 + 同一会话的精简对话栏。
//! 与 Agent 窗口共用同一个 `Workbench` 实体(会话、设置、项目),因此对话实时同步。
//!
//! @author ixxxxoooo

use crate::ide::IdeView;
use crate::ui::dev_badge;
use crate::right_panel::{PanelEvent, RightPanel};
use crate::workbench::{Page, RightTab, SaveFile, ToggleIde, Workbench, chat_panel};
use gpui_kit::assets::IconName;
use gpui_kit::component::TitleBar;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// IDE 窗口的根视图。
pub struct IdeWindow {
    wb: Entity<Workbench>,
    panel: Entity<RightPanel>,
    ide: Entity<IdeView>,
    focus: FocusHandle,
    /// 文件面板是否展开
    left_open: bool,
    /// 面板已同步到的会话 id
    synced: Option<String>,
    _subs: Vec<Subscription>,
}

impl IdeWindow {
    /// 创建窗口视图。`demo`、`root` 由调用方传入,避免在工作台更新期间回读它。
    pub fn new(wb: Entity<Workbench>, demo: bool, root: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let panel = cx.new(|cx| RightPanel::new(root.clone(), demo, window, cx));
        panel.update(cx, |p, cx| p.set_tab(RightTab::Files, cx));
        let ide = cx.new(|_| IdeView::new());
        ide.update(cx, |v, cx| v.set_root(root, cx));
        let mut subs = Vec::new();
        // 文件树点选 → 编辑器打开
        subs.push(cx.subscribe_in(&panel, window, |this, _, ev: &PanelEvent, window, cx| match ev {
            PanelEvent::OpenFile(path) => {
                let path = path.clone();
                this.panel.update(cx, |p, _| p.files.reveal(&path));
                this.ide.update(cx, |v, cx| v.open(path, window, cx));
            }
        }));
        // 工作台变化(切换会话等)→ 同步面板与根目录
        subs.push(cx.observe(&wb, |this, _, cx| this.sync(cx)));
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        // 此时工作台可能正处于更新中,不在这里回读它;首次渲染时再同步
        Self { wb, panel, ide, focus, left_open: true, synced: None, _subs: subs }
    }

    /// 让面板与根目录跟随工作台当前会话。
    fn sync(&mut self, cx: &mut Context<Self>) {
        if self.sync_state(cx) {
            cx.notify();
        }
    }

    /// 实际同步;有变化返回 true。渲染时也调用它(不触发 notify)。
    fn sync_state(&mut self, cx: &mut Context<Self>) -> bool {
        let (id, model, root) = {
            let wb = self.wb.read(cx);
            (wb.active.clone(), wb.active_model().cloned(), wb.active_project_path())
        };
        if id == self.synced {
            return false;
        }
        self.synced = id;
        if let Some(root) = root {
            self.ide.update(cx, |v, cx| v.set_root(root.clone(), cx));
            self.panel.update(cx, |p, cx| p.set_session(model, root, cx));
        }
        true
    }

    /// 在编辑器里打开文件(融合模式下由工作台转发)。
    pub fn open_file(&mut self, path: std::path::PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.panel.update(cx, |p, _| p.files.reveal(&path));
        self.ide.update(cx, |v, cx| v.open(path, window, cx));
    }

    /// 保存当前文件。
    pub fn save(&mut self, cx: &mut Context<Self>) {
        self.ide.update(cx, |v, cx| v.save(cx));
    }

    /// 回到 Agent:融合模式下整窗原地换回对话布局,否则聚焦独立的 Agent 窗口。
    fn back_to_agent(&mut self, cx: &mut Context<Self>) {
        if self.wb.read(cx).settings.fused_ide {
            self.wb.update(cx, |wb, cx| {
                wb.ide = false;
                cx.notify();
            });
            return;
        }
        if let Some(handle) = self.wb.read(cx).agent_window {
            let _ = handle.update(cx, |_, window, _| window.activate_window());
        }
    }
}

impl Render for IdeWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_state(cx);
        let t = cx.theme().clone();
        let (parts, title, status, fused, branch, demo) = {
            let wb = self.wb.read(cx);
            (wb.chat_parts(), wb.active_model().map(|m| m.read(cx).state.title.clone()).unwrap_or_else(|| "UCode".into()), wb.status_bar(cx), wb.settings.fused_ide, wb.active_branch(), wb.demo)
        };
        v_flex()
            .key_context("IdeWindow")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &SaveFile, _, cx| this.ide.update(cx, |v, cx| v.save(cx))))
            .on_action(cx.listener(|this, _: &ToggleIde, _, cx| this.back_to_agent(cx)))
            .size_full()
            .bg(t.background)
            .text_color(t.foreground)
            .child(TitleBar::new().child(
                h_flex().w_full().items_center().gap_1()
                    .when_some(dev_badge(cx), |d, b| d.child(b))
                    .child(Button::new("iw-left").small().ghost().icon(if self.left_open { IconName::PanelLeftClose } else { IconName::PanelLeftOpen })
                        .on_click(cx.listener(|this, _, _, cx| { this.left_open = !this.left_open; cx.notify(); })))
                    .child(div().flex_1().flex().justify_center().text_size(px(13.)).text_color(t.muted_foreground).child(format!("IDE · {title}")))
                    .child(Button::new("iw-settings").small().ghost().icon(IconName::Settings).tooltip("设置")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.wb.update(cx, |wb, cx| wb.set_page(Page::Settings, cx));
                            this.back_to_agent(cx);
                        })))
                    .child(Button::new("iw-agent").small().primary().label("Agents Window").icon(if fused { IconName::MessageSquare } else { IconName::ExternalLink }).tooltip("回到 Agent 窗口(⇧⌘E)")
                        .on_click(cx.listener(|this, _, _, cx| this.back_to_agent(cx)))),
            ))
            .child(
                h_flex().flex_1().min_h_0().w_full()
                    .when(self.left_open, |d| d.child(div().w(px(280.)).h_full().flex_none().border_r_1().border_color(t.border).child(self.panel.clone())))
                    .child(div().flex_1().min_w_0().h_full().child(self.ide.clone()))
                    .child(div().w(px(420.)).h_full().flex_none().border_l_1().border_color(t.border).child(chat_panel(parts, branch, demo, cx))),
            )
            .child(status)
    }
}
