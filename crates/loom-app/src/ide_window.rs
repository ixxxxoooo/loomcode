//! IDE 窗口:活动栏 + 文件树 + 单文件编辑器 + 同一会话的对话栏 + 状态栏。
//! 与 Agent 窗口共用同一个 `Workbench` 实体(会话、设置、项目),因此对话实时同步。
//! 融合模式下它整窗替换 Agent 布局;独立模式下它是一个单独的窗口。
//!
//! @author ixxxxoooo

use crate::ide_status;
use crate::ide::{ChangeAllOccurrences, CloseTab, FindReferences, FormatDocument, GoToImplementation, OpenCommandPalette, RenameSymbol, ShowFileHistory, ShowRefactor, ToggleWordWrap, FindInProject, GoToLine, IdeView, NavBack, NavForward, NextTab, PrevTab, QuickOpen, ShowScm, TogglePanel};
use crate::right_panel::{PanelEvent, RightPanel};
use crate::ui::{icon, no_drag};
use gpui_kit::component::Disableable as _;
use gpui_kit::component::Selectable as _;
use crate::workbench::{NewSession, RightTab, SaveFile, ToggleIde, ToggleLeft, ToggleRight, Workbench, chat_panel};
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
    /// 面板已同步到的 (会话 id, 项目根目录);没有会话时项目由侧栏点选决定
    pub(crate) synced: Option<(Option<String>, Option<String>)>,
    /// 文件面板当前所在的项目根(变了就按记忆重新展开目录)
    panel_root: String,
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
    /// 对话栏的会话标签(会话 id,顺序即标签顺序)与相关状态
    pub(crate) chat_tabs: Vec<String>,
    /// IDE 固定的项目(项目路径,取自进入 IDE 时的当前项目):文件树、编辑器、对话标签、历史会话都限定在它之内
    pub(crate) pinned: Option<String>,
    /// 已处理到的 `Workbench.ide_enter`;变了说明又一次从 Agent 进入 IDE,要重新固定
    pub(crate) seen_enter: u64,
    pub(crate) chat_last_active: Option<String>,
    /// 所有标签都被关掉了
    pub(crate) chat_empty: bool,
    /// 对话栏展开为占满窗口
    pub(crate) chat_wide: bool,
    /// 通知面板(状态栏铃铛)是否展开
    pub(crate) notice_open: bool,
    /// 分支选择器(点状态栏分支名或 SCM 顶部分支名弹出)
    pub(crate) branch_picker: Option<Entity<crate::branch_picker::BranchPicker>>,
    pub(crate) _branch_sub: Option<Subscription>,
    /// 对话栏的历史会话选择器
    pub(crate) history_picker: Option<Entity<crate::history_picker::HistoryPicker>>,
    pub(crate) _history_sub: Option<Subscription>,
    /// 对话标签条的滚动句柄,以及上次已滚到可见处的 (当前标签, 标签数)
    pub(crate) chat_scroll: ScrollHandle,
    pub(crate) chat_revealed: std::cell::RefCell<Option<(Option<String>, usize)>>,
}

impl IdeWindow {
    /// 创建窗口视图。`demo`、`root`、`wb_tabs` 由调用方传入:创建发生在工作台的更新期间,这里不能再回读它(会重复借用而崩溃)。
    ///
    /// 参数:`ide` 工作台里唯一的编辑器(与 Agent 的「文件」标签共用);`wb_tabs` 上次关闭时对话栏的会话标签。
    pub fn new(wb: Entity<Workbench>, ide: Entity<IdeView>, scm: Entity<crate::scm::ScmView>, demo: bool, _root: String, wb_tabs: Vec<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        bind_ide_keys(cx);
        crate::window_memory::track("ide", window, cx);
        let scm_events = scm.clone();
        let panel = cx.new(|cx| RightPanel::new(_root.clone(), demo, None, scm, window, cx));
        panel.update(cx, |p, cx| {
            p.attach_outline(ide.clone(), cx);
            p.show_tabs = false;
            p.set_tab(RightTab::Files, cx);
            p.reload_files(cx);
        });
        let search = cx.new(|cx| crate::ide_search::SearchPanel::new(String::new(), window, cx));
        let mut subs = Vec::new();
        // SCM 顶部点分支名 → 分支选择器
        subs.push(cx.subscribe_in(&scm_events, window, |this, _, ev: &crate::scm::ScmEvent, window, cx| {
            if matches!(ev, crate::scm::ScmEvent::PickBranch) {
                this.toggle_branch_picker(window, cx);
            }
        }));
        // 搜索结果点击 → 打开文件并跳行
        subs.push(cx.subscribe_in(&search, window, |this, _, ev: &crate::ide_search::SearchEvent, window, cx| match ev {
            crate::ide_search::SearchEvent::Open { path, line, start, end } => this.open_match(path.clone(), *line, *start, *end, window, cx),
        }));
        // 文件树点选 → 编辑器打开
        subs.push(cx.subscribe_in(&panel, window, |this, _, ev: &PanelEvent, window, cx| match ev {
            PanelEvent::OpenFile(path) => this.open_file(path.clone(), window, cx),
            PanelEvent::OpenFilePinned(path) => this.open_path(&path.to_string_lossy(), window, cx),
            // IDE 窗口的左栏不记右栏标签状态
            PanelEvent::StateChanged | PanelEvent::OpenIdeWindow => {}
        }));
        // 工作台变化(切换会话等)→ 同步面板与根目录
        subs.push(cx.observe(&wb, |this, _, cx| this.sync(cx)));
        // 编辑器光标移动 → 刷新状态栏(标签记忆由工作台统一保存)
        subs.push(cx.observe(&ide, |_, _, cx| cx.notify()));
        // 标签 / 面包屑右键「在资源管理器中定位」:展开左栏的文件树并定位
        subs.push(cx.subscribe(&ide, |this, _, ev: &crate::ide::IdeEvent, cx| {
            let crate::ide::IdeEvent::RevealInExplorer(path) = ev else { return };
            this.left_open = true;
            this.search_open = false;
            let path = path.clone();
            this.panel.update(cx, |p, cx| {
                p.set_tab(RightTab::Files, cx);
                p.tree_collapsed = false;
                p.files.reveal(&path);
                cx.notify();
            });
            cx.notify();
        }));
        // 通知历史变化 → 刷新铃铛
        if let Some(store) = crate::notice::store(cx) {
            subs.push(cx.observe(&store, |_, _, cx| cx.notify()));
        }
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let term = cx.new(|cx| crate::ide_terminal::BottomTerminal::new(panel.clone(), cx));
        // 调试用:LOOM_IDE_TERM=1 启动时就展开底部终端面板,方便截图
        let term_open = std::env::var("LOOM_IDE_TERM").is_ok_and(|v| v == "1");
        // 调试用:LOOM_DIFF=N 启动后打开 SCM 里第 N 个文件的差异标签
        if let Some(n) = std::env::var("LOOM_DIFF").ok().and_then(|v| v.parse::<usize>().ok()) {
            let scm = scm_events.clone();
            cx.spawn_in(window, async move |_, cx| {
                cx.background_executor().timer(std::time::Duration::from_millis(2500)).await;
                let _ = scm.update(cx, |s, cx| s.open_nth_diff(n, cx));
            })
            .detach();
        }
        // 调试用:LOOM_HISTORY=1 启动后自动打开对话栏的历史会话选择器
        if std::env::var("LOOM_HISTORY").is_ok() {
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(std::time::Duration::from_millis(1200)).await;
                let _ = this.update_in(cx, |this, window, cx| this.toggle_history(window, cx));
            })
            .detach();
        }
        // 调试用:LOOM_SEARCH=<关键字> 启动后自动打开左栏搜索并填入关键字(LOOM_SEARCH_REPLACE=<文本> 同时展开替换行)
        if let Ok(q) = std::env::var("LOOM_SEARCH") {
            let rep = std::env::var("LOOM_SEARCH_REPLACE").ok();
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(std::time::Duration::from_millis(800)).await;
                let _ = this.update_in(cx, |this, window, cx| {
                    this.show_search(window, cx);
                    this.search.update(cx, |s, cx| {
                        if let Some(r) = rep {
                            s.replace_open = true;
                            s.replace_input.update(cx, |i, cx| i.set_value(r, window, cx));
                        }
                        s.seed(q, window, cx);
                    });
                });
            })
            .detach();
        }
        // 调试用:LOOM_BRANCH=1 启动后自动打开分支选择器(=查询词则预填)
        if let Ok(q) = std::env::var("LOOM_BRANCH") {
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(std::time::Duration::from_millis(800)).await;
                let _ = this.update_in(cx, |this, window, cx| {
                    this.toggle_branch_picker(window, cx);
                    if q != "1" && let Some(p) = &this.branch_picker {
                        p.update(cx, |p, cx| p.set_query(&q, window, cx));
                    }
                });
            })
            .detach();
        }
        // 调试用:LOOM_NOTICES=1 启动后弹出一组示例通知(=open 同时展开通知面板),验证右下角提示与铃铛
        if let Ok(v) = std::env::var("LOOM_NOTICES") {
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(std::time::Duration::from_millis(800)).await;
                let _ = this.update_in(cx, |this, window, cx| {
                    crate::notice::success(window, cx, "已提交 3 个文件的更改");
                    crate::notice::warning(window, cx, "语言服务器 rust-analyzer 仍在索引,部分跳转可能不完整");
                    crate::notice::error(window, cx, "拉取失败:无法连接到远端 origin");
                    crate::notice::post(cx, crate::notice::Kind::Info, "发现新版本 v9.9.9", Some(crate::notice::NoticeAction { label: "下载更新".into(), run: std::rc::Rc::new(|_, _| {}) }));
                    if v == "open" {
                        this.notice_open = true;
                        if let Some(s) = crate::notice::store(cx) {
                            s.update(cx, |s, cx| { s.mark_all_read(); cx.notify(); });
                        }
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        // 此时工作台可能正处于更新中,不在这里回读它;首次渲染时再同步
        Self { wb, panel, ide, panel_root: String::new(), focus, left_open: true, chat_open: true, left_narrow: false, left_before: true, chat_narrow: false, chat_before: true, synced: None, _subs: subs, search, search_open: false, quick: None, _quick_sub: None, term_open, term, chat_tabs: wb_tabs, pinned: None, seen_enter: 0, chat_last_active: None, chat_empty: false, chat_wide: std::env::var("LOOM_CHAT_WIDE").is_ok(), notice_open: false, branch_picker: None, _branch_sub: None, history_picker: None, _history_sub: None, chat_scroll: ScrollHandle::new(), chat_revealed: Default::default() }
    }

    /// 让面板与根目录跟随工作台当前会话。
    fn sync(&mut self, cx: &mut Context<Self>) {
        if self.sync_state(cx) {
            cx.notify();
        }
    }

    /// 实际同步;有变化返回 true。渲染时也调用它(不触发 notify)。
    fn sync_state(&mut self, cx: &mut Context<Self>) -> bool {
        let pin_changed = self.ensure_pin(cx);
        let (id, model, root) = {
            let wb = self.wb.read(cx);
            // 目录固定为 IDE 的项目;当前会话不属于它时(独立窗口里 Agent 切去了别的项目)不跟随
            let root = self.pinned.as_ref().map(|p| wb.work_root(p)).or_else(|| wb.active_project_path());
            let model = if self.active_is_foreign(wb) { None } else { wb.active_model().cloned() };
            (wb.active.clone(), model, root)
        };
        let key = (id, root.clone());
        if !pin_changed && self.synced.as_ref() == Some(&key) {
            return false;
        }
        self.synced = Some(key);
        if let Some(root) = root {
            // 编辑器的项目切换与标签记忆由工作台统一处理(Agent 与 IDE 共用同一个编辑器);这里只同步自己的面板
            let panel = self.panel.clone();
            self.wb.update(cx, |wb, cx| {
                wb.ide_tree = Some(panel);
                wb.sync_editor_root(&root, cx);
            });
            self.search.update(cx, |s, cx| s.set_root(root.clone(), cx));
            self.panel.update(cx, |p, cx| p.set_session(model, root.clone(), cx));
            if self.panel_root != root {
                self.panel_root = root.clone();
                // 文件树换根后会清掉展开状态,按记忆重新展开
                let dirs = self.wb.read(cx).restore_expanded.clone();
                self.panel.update(cx, |p, _| p.files.expand_dirs(&dirs));
            }
        }
        true
    }

    /// 按文件 / 行位置历史后退或前进,并在文件树里定位。
    pub(crate) fn ide_navigate(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self.ide.update(cx, |v, cx| v.navigate(forward, window, cx)) {
            self.panel.update(cx, |p, _| p.files.reveal(&path));
        }
        cx.notify();
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

    /// ⌘N:IDE 窗口自己没有会话,转到 Agent 窗口新建会话并切到前台。
    ///
    /// 融合模式下 IDE 嵌在工作台里,动作直接交给工作台处理。
    fn new_agent_session(&mut self, cx: &mut Context<Self>) {
        if self.wb.read(cx).settings.fused_ide {
            cx.propagate();
            return;
        }
        let wb = self.wb.clone();
        let Some(handle) = wb.read(cx).agent_window else { return };
        // 要更新另一个窗口里的工作台,推迟到当前事件之后,避免重复借用
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                window.activate_window();
                wb.update(cx, |wb, cx| wb.new_session(wb.target_project(), window, cx));
            });
        });
    }

    /// 标题栏:左侧侧栏开关 / 搜索 / 后退 / 前进、居中的项目名搜索框、右侧对话栏开关 + 回 Agent(与 Agent 窗口同位置)。
    fn title_bar(&self, project: String, fused: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let (can_back, can_fwd) = { let nav = &self.ide.read(cx).nav; (nav.can_back(), nav.can_forward()) };
        // 高度与 Agent 窗口的顶部行一致,切换按钮在两个窗口里位置不变
        TitleBar::new().h(px(crate::window_header::HEADER_H)).child(
            // flex_1 + min_w_0 而不是 w_full:标题栏自己在左边留了红绿灯的位置,w_full 会比剩余宽度多出这一段
            h_flex().flex_1().min_w_0().items_center().gap_1().pr_2()
                // 左侧按钮组与 Agent 窗口的 left_buttons 完全一致(同样的按钮、间距、起始位置),切换窗口时不跳动
                .child(h_flex().gap_0p5().flex_none()
                    .child(no_drag(Button::new("iw-left").small().ghost().icon(IconName::PanelLeft).tooltip("侧栏(⌘B)")
                        .on_click(cx.listener(|this, _, _, cx| { this.left_open = !this.left_open; cx.notify(); }))))
                    .child(no_drag(Button::new("iw-search").small().ghost().icon(IconName::Search).tooltip("搜索(⌘K)")
                        .on_click(cx.listener(|this, _, window, cx| this.toggle_quick(window, cx)))))
                    .child(no_drag(Button::new("iw-back-nav").small().ghost().icon(IconName::ArrowLeft).tooltip("后退(⌃-)").disabled(!can_back)
                        .on_click(cx.listener(|this, _, window, cx| this.ide_navigate(false, window, cx)))))
                    .child(no_drag(Button::new("iw-fwd-nav").small().ghost().icon(IconName::ArrowRight).tooltip("前进(⌃⇧-)").disabled(!can_fwd)
                        .on_click(cx.listener(|this, _, window, cx| this.ide_navigate(true, window, cx))))))
                // 中间的搜索框可以收窄(min_w_0),窗口窄时让出位置;两侧的按钮一律 flex_none,任何宽度下都不被挤走样
                .child(div().flex_1().min_w_0().flex().justify_center().child(
                    // 宽度限制要放在 no_drag 包的这一层(它才是中间容器的直接子元素),放在里面的搜索框上不起作用
                    no_drag(
                        h_flex().id("iw-quick").cursor_pointer().on_click(cx.listener(|this, _, window, cx| this.toggle_quick(window, cx)))
                            .w_full().h(px(26.)).px_3().gap_2().items_center().justify_center().rounded(px(7.))
                            .border_1().border_color(t.border).bg(t.muted).text_size(px(12.)).text_color(t.muted_foreground)
                            .child(icon(IconName::Search, 13., t.muted_foreground).flex_none())
                            .child(div().min_w_0().truncate().child(project)),
                    ).flex_1().min_w_0().max_w(px(420.)),
                ))
                .child(h_flex().gap_1().items_center().flex_none()
                    .child(no_drag(Button::new("iw-term").small().ghost().icon(IconName::PanelBottom).tooltip("终端(⌃`)").selected(self.term_open)
                        .on_click(cx.listener(|this, _, window, cx| this.toggle_term(window, cx)))))
                    .child(no_drag(Button::new("iw-chat").small().ghost().icon(IconName::PanelRight).tooltip("对话栏(⌥⌘B)")
                        .on_click(cx.listener(|this, _, _, cx| { this.chat_open = !this.chat_open; cx.notify(); }))))
                    .child(no_drag(crate::ui::mode_switch("iw-agent", "Agent", if fused { IconName::MessageSquare } else { IconName::ExternalLink }, true).tooltip("切换到 Agent(⇧⌘E)")
                        .on_click(cx.listener(|this, _, _, cx| this.back_to_agent(cx)))))),
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
        // 窗口里没有任何焦点时(刚切到 IDE、关掉了有焦点的终端…)把焦点放回根:否则 ⌃` 等快捷键找不到处理者
        if window.focused(cx).is_none() {
            window.focus(&self.focus, cx);
        }
        crate::notice::mark_ide(window, true, cx);
        self.sync_state(cx);
        self.sync_chat_tabs(cx);
        self.apply_responsive(f32::from(window.viewport_size().width));
        self.sync_layout(cx);
        let t = cx.theme().clone();
        let foreign = self.active_is_foreign(self.wb.read(cx));
        let (parts, project, project_name, mode, fused, branch, demo) = {
            let wb = self.wb.read(cx);
            let pinned = self.pinned_ix(wb);
            let project = pinned.and_then(|i| wb.projects.get(i)).map(|p| loom_core::project_name_from_path(&wb.work_root(&p.path))).or_else(|| wb.active_project_path().map(|p| loom_core::project_name_from_path(&p))).unwrap_or_else(|| "LoomCode".into());
            let view = wb.active.as_ref().and_then(|id| wb.views.get(id));
            let name = pinned.and_then(|i| wb.projects.get(i)).map(|p| p.name.clone());
            let mode = view.filter(|_| !foreign).map(|v| v.model.read(cx).state.mode.label().to_string());
            (if foreign { None } else { wb.chat_parts() }, project, name, mode, wb.settings.fused_ide, wb.active_branch(), wb.demo)
        };
        if let Some((model, _)) = &parts {
            crate::wizard_input::ensure(model, window, cx);
        }
        let chat_me = cx.entity().downgrade();
        let chat_col = div().id("ide-chat-host").size_full()
            .on_hover(move |on, window, _| crate::ide_chat_tabs::set_chat_hover(*on, window, &chat_me))
            .child(v_flex().size_full().child(self.chat_tabs_bar(cx)).child(div().flex_1().min_h_0().child(if self.chat_empty || foreign { self.chat_empty_view(cx) } else { chat_panel(parts, branch.clone(), demo, cx) })))
            .into_any_element();
        v_flex()
            .relative()
            .key_context("IdeWindow")
            // 平滑滚动拦截层:必须是第一个子元素
            .child(crate::smooth_scroll::layer())
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &SaveFile, _, cx| this.save(cx)))
            .on_action(cx.listener(|this, a: &OpenPath, window, cx| this.open_path(&a.path, window, cx)))
            // 工具卡「在终端面板查看」:命令的只读标签在 Agent 右栏的终端面板里,转给工作台打开并提示
            .on_action(cx.listener(|this, a: &crate::workbench::ViewAgentRun, window, cx| {
                let (wb, id) = (this.wb.clone(), a.id.clone());
                cx.defer(move |cx| wb.update(cx, |w, cx| w.show_agent_run(id, cx)));
                crate::notice::info(window, cx, "已在 Agent 窗口右栏的「终端」里打开这条命令");
            }))
            .on_action(cx.listener(|this, a: &crate::term_links::OpenTermPath, window, cx| {
                this.open_at(std::path::PathBuf::from(&a.path), a.line.max(1), window, cx);
            }))
            .on_action(cx.listener(|this, _: &QuickOpen, window, cx| this.toggle_quick(window, cx)))
            .on_action(cx.listener(|this, _: &GoToLine, window, cx| this.goto_line_dialog(window, cx)))
            .on_action(cx.listener(|this, _: &NavBack, window, cx| this.ide_navigate(false, window, cx)))
            .on_action(cx.listener(|this, _: &NavForward, window, cx| this.ide_navigate(true, window, cx)))
            .on_action(cx.listener(|this, _: &FindInProject, window, cx| this.show_search(window, cx)))
            .on_action(cx.listener(|this, _: &crate::ide_reference::QuoteSelection, window, cx| this.quote_selection(window, cx)))
            .on_action(cx.listener(|this, _: &ShowScm, window, cx| this.show_scm(window, cx)))
            .on_action(cx.listener(|this, _: &OpenCommandPalette, window, cx| this.toggle_quick(window, cx)))
            .on_action(cx.listener(|this, _: &ShowFileHistory, window, cx| {
                if let Some(path) = this.ide.read(cx).active_tab(cx).map(|p| crate::ide_diff::real_path(&p)) {
                    let scm = this.wb.read(cx).scm.clone();
                    scm.update(cx, |s, cx| s.show_file_history(&path, cx));
                    this.show_scm(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &TogglePanel, window, cx| this.toggle_term(window, cx)))
            .on_action(cx.listener(|this, _: &crate::term_actions::ToggleTerminal, window, cx| this.toggle_term(window, cx)))
            .on_action(cx.listener(|this, _: &crate::term_actions::NewTerminal, window, cx| this.open_new_term(window, cx)))
            .on_action(cx.listener(|this, _: &ToggleIde, _, cx| this.back_to_agent(cx)))
            .on_action(cx.listener(|this, _: &NewSession, _, cx| this.new_agent_session(cx)))
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
                        div().flex_1().min_w_0().h_full().child(if self.chat_wide {
                            // 展开:对话栏占满整个区域
                            div().size_full().child(chat_col).into_any_element()
                        } else {
                            h_resizable("ide-split")
                                .child(resizable_panel().size(px(260.)).size_range(px(160.)..px(520.)).flex_none().fixed().visible(self.left_open)
                                    .child(div().size_full().child(if self.search_open { self.search.clone().into_any_element() } else { self.panel.clone().into_any_element() })))
                                .child(resizable_panel().size_range(px(320.)..Pixels::MAX).child(
                                    v_resizable("ide-vsplit")
                                        .child(resizable_panel().size_range(px(120.)..Pixels::MAX).child(div().size_full().child(self.ide.clone())))
                                        .child(resizable_panel().size(px(260.)).size_range(px(120.)..px(600.)).flex_none().visible(self.term_open)
                                            .child(div().size_full().child(self.term.clone()))),
                                ))
                                .child(resizable_panel().size(px(420.)).size_range(px(crate::layout_rules::COMPOSER_MIN_W)..px(900.)).flex_none().fixed().visible(self.chat_open)
                                    .child(chat_col))
                                .into_any_element()
                        }),
                    ),
            )
            .when_some(self.quick.clone(), |d, p| d.child(p))
            .child(ide_status::status_bar(project_name, branch, self.ide.read(cx).cursor_info(cx), self.ide.read(cx).lsp_summary(), mode, demo, self.bell(cx), { let this = cx.entity(); move |window, cx| { this.update(cx, |t, cx| t.toggle_branch_picker(window, cx)); } }, self.wb.read(cx).update_badge(), cx))
            .when_some(self.branch_picker.clone(), |d, p| d.child(p))
            .when(self.notice_open, |d| d.child(self.notice_panel(cx)))
            .children(crate::diagram_viewer::current(window, cx))
    }
}

impl IdeWindow {
    /// 打开 / 关闭分支选择器。
    pub(crate) fn toggle_branch_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.branch_picker.take().is_some() {
            self._branch_sub = None;
            window.focus(&self.focus, cx);
            cx.notify();
            return;
        }
        let Some(dir) = self.wb.read(cx).active_git_dir() else {
            crate::notice::info(window, cx, "当前项目不是 Git 仓库");
            return;
        };
        let picker = cx.new(|cx| crate::branch_picker::BranchPicker::new(dir.clone(), window, cx));
        self._branch_sub = Some(cx.subscribe_in(&picker, window, move |this, _, ev: &crate::branch_picker::BranchEvent, window, cx| {
            use crate::branch_picker::BranchEvent;
            this.toggle_branch_picker(window, cx);
            let wb = this.wb.clone();
            match ev {
                BranchEvent::Close => {}
                BranchEvent::Switch { name, remote } => { wb.update(cx, |w, cx| w.switch_ref(&dir, name, *remote, window, cx)); }
                BranchEvent::Create(name) => { wb.update(cx, |w, cx| w.create_branch_named(&dir, name, window, cx)); }
            }
        }));
        self.branch_picker = Some(picker);
        cx.notify();
    }

    /// 状态栏最右的铃铛:有未读时带数字角标;点击展开 / 收起通知面板。
    fn bell(&self, cx: &mut Context<Self>) -> AnyElement {
        use gpui_kit::component::ActiveTheme as _;
        let t = cx.theme().clone();
        let unread = crate::notice::store(cx).map_or(0, |s| s.read(cx).unread());
        let open = self.notice_open;
        h_flex().id("status-bell").h_full().px_2().gap_1().items_center().cursor_pointer()
            .hover(|d| d.bg(t.list_hover))
            .when(open, |d| d.bg(t.list_hover))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, _, cx| {
                this.notice_open = !this.notice_open;
                if this.notice_open && let Some(s) = crate::notice::store(cx) {
                    s.update(cx, |s, cx| { s.mark_all_read(); cx.notify(); });
                }
                cx.notify();
            }))
            .child(icon(if unread > 0 { IconName::BellDot } else { IconName::Bell }, 12., if unread > 0 { t.foreground } else { t.muted_foreground }))
            .when(unread > 0, |d| d.child(div().text_size(px(11.)).text_color(t.foreground).child(unread.to_string())))
            .into_any_element()
    }

    /// 通知面板:贴着右下角,位于状态栏之上。
    fn notice_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(store) = crate::notice::store(cx) else { return div().into_any_element() };
        let this = cx.entity();
        // 点铃铛本身会先触发「点外部」再触发点击而反复开合:关闭只在点击落在面板外且不是铃铛时发生(铃铛 mouse_down 已拦截)
        let panel = crate::notice::panel(&store, move |_, cx| { this.update(cx, |t, cx| { t.notice_open = false; cx.notify(); }); }, cx);
        deferred(div().absolute().right(px(8.)).bottom(px(30.)).child(panel)).with_priority(5).into_any_element()
    }
}

/// 注册编辑器快捷键(⌘W 关闭标签、⌥⌘←/→ 切换标签);全局只注册一次。
pub(crate) fn bind_ide_keys(cx: &mut App) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        cx.bind_keys([
            KeyBinding::new("cmd-w", CloseTab, Some("IdeView")),
            KeyBinding::new("shift-alt-f", FormatDocument, Some("IdeView")),
            KeyBinding::new("cmd-f12", GoToImplementation, Some("IdeView")),
            KeyBinding::new("shift-f12", FindReferences, Some("IdeView")),
            KeyBinding::new("f2", RenameSymbol, Some("IdeView")),
            KeyBinding::new("cmd-f2", ChangeAllOccurrences, Some("IdeView")),
            KeyBinding::new("ctrl-shift-r", ShowRefactor, Some("IdeView")),
            KeyBinding::new("alt-z", ToggleWordWrap, Some("IdeView")),
            KeyBinding::new("cmd-shift-p", OpenCommandPalette, None),
            KeyBinding::new("cmd-p", QuickOpen, Some("IdeWindow")),
            KeyBinding::new("cmd-k", QuickOpen, Some("IdeWindow")),
            KeyBinding::new("ctrl-g", GoToLine, Some("IdeWindow")),
            KeyBinding::new("ctrl--", NavBack, Some("IdeWindow")),
            KeyBinding::new("ctrl-_", NavForward, Some("IdeWindow")),
            KeyBinding::new("cmd-shift-f", FindInProject, Some("IdeWindow")),
            KeyBinding::new("cmd-alt-c", crate::ide_search::ToggleCase, Some("IdeSearch")),
            KeyBinding::new("cmd-alt-w", crate::ide_search::ToggleWholeWord, Some("IdeSearch")),
            KeyBinding::new("cmd-alt-r", crate::ide_search::ToggleRegex, Some("IdeSearch")),
            KeyBinding::new("cmd-alt-p", crate::ide_search::TogglePreserveCase, Some("IdeSearch")),
            KeyBinding::new("cmd-alt-f", crate::ide_search::ToggleReplace, Some("IdeSearch")),
            KeyBinding::new("cmd-alt-enter", crate::ide_search::ReplaceAllMatches, Some("IdeSearch")),
            // 编辑器自带库把 ⌘⇧F 绑成「替换」(在更深的 Input 上下文里,会盖住上面这条):
            // 对齐 VS Code,编辑器里 ⌘F = 当前文件查找,⌘⇧F = 全局搜索,替换改成 ⌘⌥F
            KeyBinding::new("cmd-shift-f", FindInProject, Some("IdeWindow > Input")),
            KeyBinding::new("cmd-alt-f", gpui_kit::component::input::Replace, Some("Input")),
            KeyBinding::new("ctrl-shift-g", ShowScm, Some("IdeWindow")),
            KeyBinding::new("cmd-j", TogglePanel, Some("IdeWindow")),
            KeyBinding::new("cmd-l", crate::ide_reference::QuoteSelection, Some("IdeWindow")),
            KeyBinding::new("cmd-alt-right", NextTab, Some("IdeView")),
            KeyBinding::new("cmd-alt-left", PrevTab, Some("IdeView")),
        ]);
    });
}

#[cfg(test)]
mod tests {
    /// 回归:`IdeWindow::new` 在工作台更新期间被调用(切换 IDE 时),里面不能回读 / 更新工作台,
    /// 否则会「重复借用」panic,在 `extern "C"` 回调里直接 abort(见提交 4803294)。
    #[::core::prelude::v1::test]
    fn 创建窗口时不回读工作台() {
        let src = include_str!("ide_window.rs");
        let start = src.find("pub fn new(wb: Entity<Workbench>").expect("找不到 IdeWindow::new");
        let end = src[start..].find("/// 让面板与根目录跟随工作台当前会话。").expect("找不到 new 的结尾") + start;
        let body = &src[start..end];
        for bad in ["wb.read(", "wb.update(", "self.wb.read(", "self.wb.update("] {
            assert!(!body.contains(bad), "IdeWindow::new 里不能出现 `{bad}`:创建发生在工作台更新期间");
        }
    }
}
