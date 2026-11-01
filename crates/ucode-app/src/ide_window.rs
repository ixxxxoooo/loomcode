//! IDE 窗口:活动栏 + 文件树 + 单文件编辑器 + 同一会话的对话栏 + 状态栏。
//! 与 Agent 窗口共用同一个 `Workbench` 实体(会话、设置、项目),因此对话实时同步。
//! 融合模式下它整窗替换 Agent 布局;独立模式下它是一个单独的窗口。
//!
//! @author ixxxxoooo

use crate::ide_chat_header;
use crate::ide_status;
use crate::ide::{CloseTab, FindInProject, GoToLine, IdeView, NextTab, PrevTab, QuickOpen};
use crate::right_panel::{PanelEvent, RightPanel};
use crate::ui::{icon, no_drag};
use crate::workbench::{Page, RightTab, SaveFile, ToggleIde, ToggleLeft, ToggleRight, Workbench, chat_panel};
use gpui_kit::assets::IconName;
use gpui_kit::component::TitleBar;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, h_resizable, resizable_panel, v_flex, v_resizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// 从对话里的文件路径跳转到编辑器(路径可为相对项目根的相对路径)。
#[derive(Clone, PartialEq, Action)]
#[action(namespace = ide, no_json)]
pub struct OpenPath {
    pub path: String,
}

/// IDE 窗口的根视图。
pub struct IdeWindow {
    pub(crate) wb: Entity<Workbench>,
    pub(crate) panel: Entity<RightPanel>,
    pub(crate) ide: Entity<IdeView>,
    pub(crate) focus: FocusHandle,
    /// 文件面板是否展开
    pub(crate) left_open: bool,
    /// 对话栏是否展开
    pub(crate) chat_open: bool,
    /// 响应式:窗口窄时自动收起的状态(文件面板 / 对话栏)及收起前是否开着
    pub(crate) left_narrow: bool,
    pub(crate) left_before: bool,
    pub(crate) chat_narrow: bool,
    pub(crate) chat_before: bool,
    /// 面板已同步到的会话 id
    pub(crate) synced: Option<String>,
    pub(crate) _subs: Vec<Subscription>,
    /// 快速打开(⌘P / ⌘K)面板
    /// 项目内搜索面板与是否在左栏显示它
    pub(crate) search: Entity<crate::ide_search::SearchPanel>,
    pub(crate) search_open: bool,
    pub(crate) quick: Option<Entity<crate::palette::Palette>>,
    /// 底部终端面板是否展开,以及它的视图
    pub(crate) term_open: bool,
    pub(crate) term: Entity<crate::ide_terminal::BottomTerminal>,
    pub(crate) _quick_sub: Option<Subscription>,
}

impl IdeWindow {
    /// 创建窗口视图。`demo`、`root` 由调用方传入,避免在工作台更新期间回读它。
    pub fn new(wb: Entity<Workbench>, demo: bool, root: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        bind_ide_keys(cx);
        let panel = cx.new(|cx| RightPanel::new(root.clone(), demo, window, cx));
        panel.update(cx, |p, cx| {
            p.show_tabs = false;
            p.set_tab(RightTab::Files, cx);
            p.reload_files(cx);
        });
        let ide = cx.new(|_| IdeView::new());
        ide.update(cx, |v, cx| {
            v.set_root(root, cx);
            v.start_watch(window, cx);
        });
        let search = cx.new(|cx| crate::ide_search::SearchPanel::new(String::new(), window, cx));
        let mut subs = Vec::new();
        // 搜索结果点击 → 打开文件并跳行
        subs.push(cx.subscribe_in(&search, window, |this, _, ev: &crate::ide_search::SearchEvent, window, cx| match ev {
            crate::ide_search::SearchEvent::Open(path, line) => this.open_at(path.clone(), *line, window, cx),
        }));
        // 文件树点选 → 编辑器打开
        subs.push(cx.subscribe_in(&panel, window, |this, _, ev: &PanelEvent, window, cx| match ev {
            PanelEvent::OpenFile(path) => this.open_file(path.clone(), window, cx),
        }));
        // 工作台变化(切换会话等)→ 同步面板与根目录
        subs.push(cx.observe(&wb, |this, _, cx| this.sync(cx)));
        // 编辑器光标移动 → 刷新状态栏
        subs.push(cx.observe(&ide, |_, _, cx| cx.notify()));
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let term = cx.new(|cx| crate::ide_terminal::BottomTerminal::new(panel.clone(), cx));
        // 调试用:UCODE_IDE_TERM=1 启动时就展开底部终端面板,方便截图
        let term_open = std::env::var("UCODE_IDE_TERM").is_ok_and(|v| v == "1");
        // 此时工作台可能正处于更新中,不在这里回读它;首次渲染时再同步
        Self { wb, panel, ide, focus, left_open: true, chat_open: true, left_narrow: false, left_before: true, chat_narrow: false, chat_before: true, synced: None, _subs: subs, search, search_open: false, quick: None, _quick_sub: None, term_open, term }
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
            self.search.update(cx, |s, cx| s.set_root(root.clone(), cx));
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
    pub(crate) fn open_path(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
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
            p.reload_files(cx);
            cx.notify();
        });
    }

    /// 回到 Agent:融合模式下整窗原地换回对话布局,否则聚焦独立的 Agent 窗口。
    pub(crate) fn back_to_agent(&mut self, cx: &mut Context<Self>) {
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
            h_flex().w_full().items_center().gap_1().pr_2()
                .child(no_drag(Button::new("iw-left").small().ghost().icon(IconName::PanelLeft).tooltip("侧栏(⌘B)")
                    .on_click(cx.listener(|this, _, _, cx| { this.left_open = !this.left_open; cx.notify(); }))))
                .child(div().flex_1().flex().justify_center().child(no_drag(
                    h_flex().id("iw-quick").cursor_pointer().on_click(cx.listener(|this, _, window, cx| this.toggle_quick(window, cx)))
                        .w(px(420.)).max_w_full().h(px(26.)).px_3().gap_2().items_center().justify_center().rounded(px(7.))
                        .border_1().border_color(t.border).bg(t.muted).text_size(px(12.)).text_color(t.muted_foreground)
                        .child(icon(IconName::Search, 13., t.muted_foreground))
                        .child(project),
                )))
                .child(no_drag(Button::new("iw-chat").small().ghost().icon(IconName::PanelRight).tooltip("对话栏(⌥⌘B)")
                    .on_click(cx.listener(|this, _, _, cx| { this.chat_open = !this.chat_open; cx.notify(); }))))
                .child(no_drag(Button::new("iw-settings").small().ghost().icon(IconName::Settings).tooltip("设置")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.wb.update(cx, |wb, cx| wb.set_page(Page::Settings, cx));
                        this.back_to_agent(cx);
                    }))))
                .child(no_drag(crate::ui::mode_switch("iw-agent", "Agents Window", if fused { IconName::MessageSquare } else { IconName::ExternalLink }, true).tooltip("切换到 Agents Window(⇧⌘E)")
                    .on_click(cx.listener(|this, _, _, cx| this.back_to_agent(cx))))),
        )
    }
}

impl IdeWindow {
    /// 按窗口宽度自动收起:先收对话栏(<1100),再收文件面板(<780),最小尺寸只剩编辑器。
    ///
    /// 参数:`width` 窗口宽度(像素)。变宽后恢复到收起前的开关状态。
    fn apply_responsive(&mut self, width: f32) {
        use crate::layout_rules::{collapsed, NARROW_BELOW, RIGHT_HIDE_BELOW, RIGHT_SHOW_ABOVE, WIDE_ABOVE};
        let chat = collapsed(self.chat_narrow, width, RIGHT_HIDE_BELOW, RIGHT_SHOW_ABOVE);
        if chat != self.chat_narrow {
            self.chat_narrow = chat;
            if chat { self.chat_before = self.chat_open; self.chat_open = false; } else { self.chat_open = self.chat_before; }
        }
        let left = collapsed(self.left_narrow, width, NARROW_BELOW, WIDE_ABOVE);
        if left != self.left_narrow {
            self.left_narrow = left;
            if left { self.left_before = self.left_open; self.left_open = false; } else { self.left_open = self.left_before; }
        }
    }
}

impl Render for IdeWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_state(cx);
        self.apply_responsive(f32::from(window.viewport_size().width));
        let t = cx.theme().clone();
        let (parts, project, project_name, mode, fused, branch, demo) = {
            let wb = self.wb.read(cx);
            let project = wb.active_project_path().map(|p| ucode_core::project_name_from_path(&p)).unwrap_or_else(|| "UCode".into());
            let view = wb.active.as_ref().and_then(|id| wb.views.get(id));
            let name = view.and_then(|v| wb.projects.get(v.project_ix)).map(|p| p.name.clone());
            let mode = view.map(|v| v.model.read(cx).state.mode.label().to_string());
            (wb.chat_parts(), project, name, mode, wb.settings.fused_ide, wb.active_branch(), wb.demo)
        };
        v_flex()
            .key_context("IdeWindow")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &SaveFile, _, cx| this.save(cx)))
            .on_action(cx.listener(|this, a: &OpenPath, window, cx| this.open_path(&a.path, window, cx)))
            .on_action(cx.listener(|this, _: &QuickOpen, window, cx| this.toggle_quick(window, cx)))
            .on_action(cx.listener(|this, _: &GoToLine, window, cx| this.goto_line_dialog(window, cx)))
            .on_action(cx.listener(|this, _: &FindInProject, window, cx| this.show_search(window, cx)))
            .on_action(cx.listener(|this, _: &crate::ide_reference::QuoteSelection, window, cx| this.quote_selection(window, cx)))
            .on_action(cx.listener(|this, _: &crate::term_actions::ToggleTerminal, window, cx| this.toggle_term(window, cx)))
            .on_action(cx.listener(|this, _: &crate::term_actions::NewTerminal, window, cx| this.open_new_term(window, cx)))
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
                                    .child(div().size_full().child(if self.search_open { self.search.clone().into_any_element() } else { self.panel.clone().into_any_element() })))
                                .child(resizable_panel().size_range(px(320.)..Pixels::MAX).child(
                                    v_resizable("ide-vsplit")
                                        .child(resizable_panel().size_range(px(120.)..Pixels::MAX).child(div().size_full().child(self.ide.clone())))
                                        .child(resizable_panel().size(px(260.)).size_range(px(120.)..px(600.)).flex_none().visible(self.term_open)
                                            .child(div().size_full().child(self.term.clone()))),
                                ))
                                .child(resizable_panel().size(px(420.)).size_range(px(320.)..px(900.)).flex_none().visible(self.chat_open)
                                    .child(v_flex().size_full().children(ide_chat_header::chat_header(&self.wb, cx)).child(div().flex_1().min_h_0().child(chat_panel(parts, branch.clone(), demo, cx))))),
                        ),
                    ),
            )
            .when_some(self.quick.clone(), |d, p| d.child(p))
            .child(ide_status::status_bar(project_name, branch, self.ide.read(cx).cursor_info(cx), mode, demo, cx))
    }
}

/// 注册编辑器快捷键(⌘W 关闭标签、⌥⌘←/→ 切换标签);全局只注册一次。
fn bind_ide_keys(cx: &mut App) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        cx.bind_keys([
            KeyBinding::new("cmd-w", CloseTab, Some("IdeView")),
            KeyBinding::new("cmd-p", QuickOpen, Some("IdeWindow")),
            KeyBinding::new("cmd-k", QuickOpen, Some("IdeWindow")),
            KeyBinding::new("ctrl-g", GoToLine, Some("IdeWindow")),
            KeyBinding::new("cmd-shift-f", FindInProject, Some("IdeWindow")),
            KeyBinding::new("cmd-l", crate::ide_reference::QuoteSelection, Some("IdeWindow")),
            KeyBinding::new("cmd-alt-right", NextTab, Some("IdeView")),
            KeyBinding::new("cmd-alt-left", PrevTab, Some("IdeView")),
        ]);
    });
}
