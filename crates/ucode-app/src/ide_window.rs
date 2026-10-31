//! IDE 窗口:活动栏 + 文件树 + 单文件编辑器 + 同一会话的对话栏 + 状态栏。
//! 与 Agent 窗口共用同一个 `Workbench` 实体(会话、设置、项目),因此对话实时同步。
//! 融合模式下它整窗替换 Agent 布局;独立模式下它是一个单独的窗口。
//!
//! @author ixxxxoooo

use crate::ide_chat_header;
use crate::ide::{CloseTab, IdeView, NextTab, PrevTab};
use crate::right_panel::{PanelEvent, RightPanel};
use crate::ui::{dev_badge, icon, no_drag};
use crate::workbench::{Page, RightTab, SaveFile, ToggleIde, ToggleLeft, ToggleRight, Workbench, chat_panel};
use gpui_kit::assets::IconName;
use gpui_kit::component::TitleBar;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Selectable, Sizable, h_flex, h_resizable, resizable_panel, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// 活动栏条目:(图标, 提示, 对应的面板分段)。
const ACTIVITIES: [(IconName, &str, RightTab); 4] = [
    (IconName::Folder, "文件", RightTab::Files),
    (IconName::GitBranch, "改动", RightTab::Changes),
    (IconName::Terminal, "终端", RightTab::Terminal),
    (IconName::ListTodo, "计划", RightTab::Plan),
];

/// 从对话里的文件路径跳转到编辑器(路径可为相对项目根的相对路径)。
#[derive(Clone, PartialEq, Action)]
#[action(namespace = ide, no_json)]
pub struct OpenPath {
    pub path: String,
}

/// IDE 窗口的根视图。
pub struct IdeWindow {
    wb: Entity<Workbench>,
    panel: Entity<RightPanel>,
    ide: Entity<IdeView>,
    focus: FocusHandle,
    /// 文件面板是否展开
    left_open: bool,
    /// 对话栏是否展开
    chat_open: bool,
    /// 面板已同步到的会话 id
    synced: Option<String>,
    _subs: Vec<Subscription>,
}

impl IdeWindow {
    /// 创建窗口视图。`demo`、`root` 由调用方传入,避免在工作台更新期间回读它。
    pub fn new(wb: Entity<Workbench>, demo: bool, root: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        bind_ide_keys(cx);
        let panel = cx.new(|cx| RightPanel::new(root.clone(), demo, window, cx));
        panel.update(cx, |p, cx| {
            p.show_tabs = false;
            p.set_tab(RightTab::Files, cx);
        });
        let ide = cx.new(|_| IdeView::new());
        ide.update(cx, |v, cx| v.set_root(root, cx));
        let mut subs = Vec::new();
        // 文件树点选 → 编辑器打开
        subs.push(cx.subscribe_in(&panel, window, |this, _, ev: &PanelEvent, window, cx| match ev {
            PanelEvent::OpenFile(path) => this.open_file(path.clone(), window, cx),
        }));
        // 工作台变化(切换会话等)→ 同步面板与根目录
        subs.push(cx.observe(&wb, |this, _, cx| this.sync(cx)));
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        // 此时工作台可能正处于更新中,不在这里回读它;首次渲染时再同步
        Self { wb, panel, ide, focus, left_open: true, chat_open: true, synced: None, _subs: subs }
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

    /// 在编辑器里打开文件(文件树点选,或融合模式下由工作台转发)。
    pub fn open_file(&mut self, path: std::path::PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.panel.update(cx, |p, _| p.files.reveal(&path));
        self.ide.update(cx, |v, cx| v.open(path, true, window, cx));
    }

    /// 打开对话里点击的文件(固定标签,不是预览)。
    fn open_path(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let p = std::path::PathBuf::from(path);
        let full = if p.is_absolute() { p } else { std::path::PathBuf::from(self.wb.read(cx).active_project_path().unwrap_or_default()).join(p) };
        self.panel.update(cx, |p, _| p.files.reveal(&full));
        self.ide.update(cx, |v, cx| v.open(full, false, window, cx));
    }

    /// 保存当前文件。
    pub fn save(&mut self, cx: &mut Context<Self>) {
        self.ide.update(cx, |v, cx| v.save(cx));
        // 保存后刷新文件树的 Git 标记
        self.panel.update(cx, |p, cx| {
            p.files.reload();
            cx.notify();
        });
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

    /// 标题栏:左侧面板开关、居中的项目名搜索框、右侧对话栏开关 / 设置 / 回 Agent。
    fn title_bar(&self, project: String, fused: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        TitleBar::new().child(
            h_flex().w_full().items_center().gap_1()
                .child(no_drag(Button::new("iw-left").small().ghost().icon(IconName::PanelLeft).selected(self.left_open).tooltip("侧栏(⌘B)")
                    .on_click(cx.listener(|this, _, _, cx| { this.left_open = !this.left_open; cx.notify(); }))))
                .when_some(dev_badge(cx), |d, b| d.child(b))
                .child(div().flex_1().flex().justify_center().child(no_drag(
                    h_flex().w(px(420.)).max_w_full().h(px(26.)).px_3().gap_2().items_center().justify_center().rounded(px(7.))
                        .border_1().border_color(t.border).bg(t.muted).text_size(px(12.)).text_color(t.muted_foreground)
                        .child(icon(IconName::Search, 13., t.muted_foreground))
                        .child(project),
                )))
                .child(no_drag(Button::new("iw-chat").small().ghost().icon(IconName::PanelRight).selected(self.chat_open).tooltip("对话栏(⌥⌘B)")
                    .on_click(cx.listener(|this, _, _, cx| { this.chat_open = !this.chat_open; cx.notify(); }))))
                .child(no_drag(Button::new("iw-settings").small().ghost().icon(IconName::Settings).tooltip("设置")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.wb.update(cx, |wb, cx| wb.set_page(Page::Settings, cx));
                        this.back_to_agent(cx);
                    }))))
                .child(no_drag(Button::new("iw-agent").small().primary().label("Agents Window").icon(if fused { IconName::MessageSquare } else { IconName::ExternalLink }).tooltip("切换到 Agents Window(⇧⌘E)")
                    .on_click(cx.listener(|this, _, _, cx| this.back_to_agent(cx))))),
        )
    }

    /// 最左侧的窄活动栏:切换文件 / 改动 / 终端 / 计划。
    fn activity_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let cur = self.panel.read(cx).tab;
        v_flex().w(px(44.)).h_full().flex_none().items_center().pt_2().gap_1().border_r_1().border_color(t.border).bg(t.sidebar)
            .children(ACTIVITIES.into_iter().map(|(ic, tip, tab)| {
                Button::new(SharedString::from(format!("act-{}", tab.key()))).ghost().icon(ic).tooltip(tip).selected(cur == tab && self.left_open)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.left_open = true;
                        this.panel.update(cx, |p, cx| p.set_tab(tab, cx));
                        cx.notify();
                    }))
            }))
    }
}

impl Render for IdeWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_state(cx);
        let t = cx.theme().clone();
        let (parts, project, status, fused, branch, demo) = {
            let wb = self.wb.read(cx);
            let project = wb.active_project_path().map(|p| ucode_core::project_name_from_path(&p)).unwrap_or_else(|| "UCode".into());
            (wb.chat_parts(), project, wb.status_bar(cx), wb.settings.fused_ide, wb.active_branch(), wb.demo)
        };
        v_flex()
            .key_context("IdeWindow")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &SaveFile, _, cx| this.save(cx)))
            .on_action(cx.listener(|this, a: &OpenPath, window, cx| this.open_path(&a.path, window, cx)))
            .on_action(cx.listener(|this, _: &ToggleIde, _, cx| this.back_to_agent(cx)))
            .on_action(cx.listener(|this, _: &ToggleLeft, _, cx| { this.left_open = !this.left_open; cx.notify(); }))
            .on_action(cx.listener(|this, _: &ToggleRight, _, cx| { this.chat_open = !this.chat_open; cx.notify(); }))
            .size_full()
            .bg(t.background)
            .text_color(t.foreground)
            .child(self.title_bar(project, fused, cx))
            .child(
                h_flex().flex_1().min_h_0().w_full()
                    .child(self.activity_bar(cx))
                    .child(
                        div().flex_1().min_w_0().h_full().child(
                            h_resizable("ide-split")
                                .child(resizable_panel().size(px(260.)).size_range(px(160.)..px(520.)).flex_none().visible(self.left_open)
                                    .child(div().size_full().border_r_1().border_color(t.border).child(self.panel.clone())))
                                .child(resizable_panel().size_range(px(320.)..Pixels::MAX).child(div().size_full().child(self.ide.clone())))
                                .child(resizable_panel().size(px(420.)).size_range(px(320.)..px(900.)).flex_none().visible(self.chat_open)
                                    .child(v_flex().size_full().border_l_1().border_color(t.border).children(ide_chat_header::chat_header(&self.wb, cx)).child(div().flex_1().min_h_0().child(chat_panel(parts, branch, demo, cx))))),
                        ),
                    ),
            )
            .child(status)
    }
}

/// 注册编辑器快捷键(⌘W 关闭标签、⌥⌘←/→ 切换标签);全局只注册一次。
fn bind_ide_keys(cx: &mut App) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        cx.bind_keys([
            KeyBinding::new("cmd-w", CloseTab, Some("IdeView")),
            KeyBinding::new("cmd-alt-right", NextTab, Some("IdeView")),
            KeyBinding::new("cmd-alt-left", PrevTab, Some("IdeView")),
        ]);
    });
}
