//! 工作台根视图:标题栏、左侧项目栏、中间会话、右侧固定面板、状态栏。

use crate::automation_page::AutomationPage;
use crate::browser_page::BrowserPage;
use crate::canvas_page::CanvasPage;
use crate::composer::Composer;
use crate::ide_window::IdeWindow;
use crate::model::{SessionModel, UiRequest};
use crate::palette::{Group, Palette, PaletteAction, PaletteEvent, PaletteItem};
use crate::right_panel::{PanelEvent, RightPanel};
use crate::settings::{self, SettingsTab};
use crate::ui::{empty_state, icon};
use crate::{sidebar, theme, timeline};
use gpui_kit::assets::IconName;
use gpui_kit::component::TitleBar;
use crate::layout_rules::{CENTER_MIN, LEFT_MAX, LEFT_MIN, RIGHT_MAX, RIGHT_MIN};
use gpui_kit::component::{ActiveTheme, h_flex, h_resizable, resizable_panel, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use loom_automation::{AutomationStore, now_ts};
use loom_canvas::{CanvasStore, demo_document};
use loom_config::{AppSettings, ClaudeConfig, ThemeChoice, UiState};
use loom_core::*;
use loom_claude::{ClaudeBackend, LazyBackend};
use loom_demo::DemoBackend;

actions!(loomcode, [TogglePalette, ToggleLeft, ToggleRight, OpenSettings, CycleMode, NewSession, Escape, SaveFile, ToggleIde]);

/// 主区域当前页面。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Chat,
    Settings,
    Canvas,
    Automation,
    Browser,
    /// 自定义:插件 / MCP / 技能 / 命令 / 子 Agent / 规则 / Hooks
    Customize,
}

/// 右侧面板的分段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RightTab {
    Changes,
    Files,
    Terminal,
    Plan,
    Output,
}

impl RightTab {
    pub const ALL: [RightTab; 5] = [Self::Changes, Self::Files, Self::Terminal, Self::Plan, Self::Output];

    /// 英文键(环境变量、持久化用)。
    pub fn key(self) -> &'static str {
        match self {
            Self::Changes => "changes",
            Self::Files => "files",
            Self::Terminal => "terminal",
            Self::Plan => "plan",
            Self::Output => "output",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Changes => "改动",
            Self::Files => "文件",
            Self::Terminal => "终端",
            Self::Plan => "计划",
            Self::Output => "输出",
        }
    }

    pub fn icon(self) -> IconName {
        match self {
            Self::Changes => IconName::FileDiff,
            Self::Files => IconName::Folder,
            Self::Terminal => IconName::Terminal,
            Self::Plan => IconName::ListTodo,
            Self::Output => IconName::ScrollText,
        }
    }
}

/// 一个已打开会话的界面状态。
pub struct SessionView {
    pub model: Entity<SessionModel>,
    pub composer: Entity<Composer>,
    pub project_ix: usize,
    _subs: Vec<Subscription>,
}

pub struct Workbench {
    pub demo: bool,
    pub projects: Vec<Project>,
    pub active: Option<String>,
    /// 会话访问历史(前进 / 后退箭头)
    pub nav: crate::nav_history::NavHistory<String>,
    /// 侧边栏里最近点选的项目(没有会话时,Agent 与 IDE 都以它为当前项目)
    pub focus_project: Option<String>,
    /// 正在按前进 / 后退切换会话:这时打开会话不应再写入历史
    pub nav_navigating: bool,
    pub views: HashMap<String, SessionView>,
    pub left_open: bool,
    pub right_open: bool,
    pub left_w: f32,
    pub right_w: f32,
    /// 三栏拖动调宽的状态(侧栏 | 对话 | 右面板)
    pub split: Entity<gpui_kit::component::resizable::ResizableState>,
    /// 窗口当前是否窄到需要自动收起侧栏
    pub layout_narrow: bool,
    /// 变窄之前侧栏是不是开着(变宽后恢复)
    pub left_before_narrow: bool,
    /// 窗口当前是否窄到需要自动收起右侧面板,以及收起前它是否开着
    pub right_narrow: bool,
    pub right_before_narrow: bool,
    /// 栏宽保存任务已排队
    pub width_save_pending: bool,
    pub panel: Entity<RightPanel>,
    pub page: Page,
    pub settings: AppSettings,
    pub settings_path: PathBuf,
    /// 侧栏的用户选择(置顶 / 归档 / 改名),落盘在 `state.json`
    pub ui_state: UiState,
    /// 应用更新流程状态
    pub update: crate::updater::UpdateState,
    pub ui_state_path: PathBuf,
    /// 侧栏是否显示已归档会话
    pub show_archived: bool,
    /// 后台跑完、用户还没看过的会话
    pub unread: HashSet<String>,
    /// 各目录的 Git 快照(分支、worktree),后台刷新后供分支菜单渲染
    pub git: HashMap<String, loom_core::git::GitSnapshot>,
    /// 侧栏顶部的过滤输入框
    /// 已打开会话的使用顺序(队首最久未用),用于淘汰空闲会话
    pub view_order: Vec<String>,
    pub claude_cfg: ClaudeConfig,
    /// `claude_cfg` 是按哪个项目根加载的(`None` 表示只有用户层)
    pub claude_cfg_root: Option<String>,
    /// Claude 配置所在的家目录(演示模式指向沙盒)
    pub claude_home: PathBuf,
    /// 权限 / MCP / 钩子页当前选中的配置层
    pub cfg_scope: loom_config::Scope,
    /// 设置页表单的输入框状态(首次进入设置页时创建)
    pub settings_forms: Option<settings::Forms>,
    /// 自定义页当前标签
    pub customize_tab: settings::CustomizeTab,
    /// 用量页当前显示的趋势指标
    pub usage_metric: settings::UsageMetric,
    /// 用量页统计范围(天):7 / 14 / 30
    pub usage_days: usize,
    pub settings_tab: SettingsTab,
    pub claude_info: Option<String>,
    pub palette: Option<Entity<Palette>>,
    _palette_sub: Option<Subscription>,
    /// 融合模式下当前是否整窗显示 IDE(原地切换)
    pub ide: bool,
    /// 融合模式的 IDE 外壳:与独立 IDE 窗口是同一个视图,整窗替换 Agent 布局
    pub ide_shell: Option<Entity<IdeWindow>>,
    /// Agent 窗口句柄(IDE 窗口据此切回)
    pub agent_window: Option<AnyWindowHandle>,
    /// 独立 IDE 窗口句柄
    pub ide_window: Option<AnyWindowHandle>,
    pub canvas: Entity<CanvasPage>,
    pub automation: Entity<AutomationPage>,
    pub browser: Entity<BrowserPage>,
    _subs: Vec<Subscription>,
    pub(crate) focus: FocusHandle,
    new_counter: u32,
    /// 本次运行里新建、还没写进历史的会话 id(启动时用预分配 id,而不是恢复)
    pub new_sessions: HashSet<String>,
    /// 分叉出来、还没写进历史的会话:新会话 id → 源会话 id(首次发消息时用 --fork-session 启动)
    pub forks: HashMap<String, String>,
    /// 历史加载完成后要打开的会话(调试用 `LOOM_SESSION`)
    pending_open: Option<String>,
}

impl Workbench {
    pub fn new(demo: bool, session: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        crate::window_memory::track("main", window, cx);
        let demo_root = loom_demo::workspace::default_root();
        // 演示沙盒与演示项目只在演示模式出现;真实模式的项目稍后从 Claude 历史里后台读取
        let projects = if demo {
            if let Err(e) = loom_demo::workspace::seed(&demo_root) {
                eprintln!("生成演示沙盒失败:{e}");
            }
            loom_demo::workspace::demo_projects(&demo_root)
        } else {
            Vec::new()
        };
        let settings_path = if demo { demo_root.join("settings.json") } else { AppSettings::default_path().unwrap_or_else(|| demo_root.join("settings.json")) };
        let settings = AppSettings::load(&settings_path);
        let last_customize = settings.last_customize_tab.clone();
        let last_tab = SettingsTab::from_name(&settings.last_settings_tab).unwrap_or(SettingsTab::General);
        let ui_state_path = settings_path.with_file_name("state.json");
        let ui_state = UiState::load(&ui_state_path);
        // 演示模式的 ~/.claude 在沙盒里(seed 时生成),读写走同一套代码
        let claude_home = if demo { loom_demo::workspace::claude_home(&demo_root) } else { std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default() };
        let claude_cfg = loom_config::claude::load(&claude_home, None);
        let focus = cx.focus_handle();
        let panel_root = if demo { demo_root.to_string_lossy().to_string() } else { std::env::var("HOME").unwrap_or_default() };
        let panel = cx.new(|cx| RightPanel::new(panel_root, demo, window, cx));
        // 右面板标签行末尾放「面板开关 + IDE 入口」(没有整条标题栏后它们跟着右栏走)
        {
            let weak = cx.entity().downgrade();
            panel.update(cx, |p, _| {
                p.trailing = Some(std::rc::Rc::new(move |_, cx| {
                    weak.update(cx, |wb, cx| wb.right_buttons(cx).into_any_element()).unwrap_or_else(|_| div().into_any_element())
                }));
            });
        }
        // 画布 / 自动化 / 浏览器三个页面常驻:自动化需要在后台持续调度
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let (canvas_dir, auto_dir) = if demo {
            (demo_root.join("canvases"), demo_root.join("automations"))
        } else {
            (CanvasStore::default_dir(&home), AutomationStore::default_dir(&home))
        };
        if demo {
            let project = projects.first().map(|p| p.path.clone()).unwrap_or_default();
            if let Err(e) = CanvasStore::new(&canvas_dir).save(&demo_document(&project, now_ts())) {
                eprintln!("写入演示画布失败:{e}");
            }
        }
        let canvas = cx.new(|_| CanvasPage::new(CanvasStore::new(canvas_dir)));
        let project_list: Vec<(String, String)> = projects.iter().map(|p| (p.name.clone(), p.path.clone())).collect();
        let claude_path = settings.claude_path.clone();
        let automation = cx.new(|cx| AutomationPage::new(AutomationStore::new(auto_dir), demo, claude_path, project_list, window, cx));
        let browser = cx.new(|cx| BrowserPage::new(window, cx));
        let settings_widths = (settings.left_width, settings.right_width);
        let mut wb = Self {
            demo,
            projects,
            active: None,
            nav: Default::default(),
            focus_project: None,
            nav_navigating: false,
            views: HashMap::new(),
            left_open: true,
            right_open: settings.right_open,
            left_w: crate::layout_rules::clamp_left(settings_widths.0),
            right_w: crate::layout_rules::clamp_right(settings_widths.1),
            split: cx.new(|_| gpui_kit::component::resizable::ResizableState::default()),
            layout_narrow: false,
            left_before_narrow: true,
            right_narrow: false,
            right_before_narrow: true,
            width_save_pending: false,
            panel,
            page: Page::Chat,
            settings,
            settings_path,
            ui_state,
            update: Default::default(),
            ui_state_path,
            show_archived: false,
            unread: HashSet::new(),
            git: HashMap::new(),
            view_order: Vec::new(),
            claude_cfg,
            claude_cfg_root: None,
            claude_home,
            cfg_scope: loom_config::Scope::User,
            settings_forms: None,
            customize_tab: settings::CustomizeTab::from_name(&last_customize).unwrap_or(settings::CustomizeTab::Plugins),
            usage_metric: settings::UsageMetric::Tokens,
            usage_days: 30,
            settings_tab: last_tab,
            claude_info: None,
            palette: None,
            _palette_sub: None,
            ide: false,
            ide_shell: None,
            agent_window: Some(window.window_handle()),
            ide_window: None,
            canvas,
            automation,
            browser,
            _subs: Vec::new(),
            focus,
            new_counter: 0,
            new_sessions: HashSet::new(),
            forks: HashMap::new(),
            pending_open: None,
        };
        wb.apply_ui_state();
        let panel_sub = cx.subscribe_in(&wb.panel, window, |this, _, ev: &PanelEvent, window, cx| this.on_panel_event(ev, window, cx));
        wb._subs.push(panel_sub);
        if demo {
            let id = session.unwrap_or_else(|| "showcase-tour".into());
            if let Some(pi) = wb.projects.iter().position(|p| p.sessions.iter().any(|s| s.id == id)) {
                wb.open_session(pi, &id, window, cx);
            } else {
                wb.open_scenario(&id, window, cx);
            }
        } else {
            wb.pending_open = session;
            wb.reload_projects(window, cx);
        }
        window.focus(&wb.focus, cx);
        let kind = match std::env::var("LOOM_THEME").as_deref() {
            Ok("dark") => theme::ThemeKind::PaperDark,
            Ok("system") => theme::ThemeKind::System,
            Ok("light") => theme::ThemeKind::PaperLight,
            Ok("one-light") => theme::ThemeKind::OneLight,
            Ok("one-dark") => theme::ThemeKind::OneDark,
            _ => theme::ThemeKind::from(wb.settings.theme),
        };
        cx.set_global(theme::Accent(wb.settings.accent.as_deref().and_then(theme::parse_hex)));
        theme::apply(kind, Some(window), cx);
        crate::app_icon::set_tray_visible(wb.settings.tray_icon);
        cx.set_global(crate::UiPrefs::from_settings(&wb.settings));
        crate::appearance::apply_scrollbar(wb.settings.appearance.scrollbar, cx);
        cx.set_global(crate::Prefs(wb.settings.clone()));
        if !demo {
            wb.detect_claude(cx);
            wb.startup_update(cx);
        }
        if let Ok(tab) = std::env::var("LOOM_SETTINGS") {
            wb.page = Page::Settings;
            wb.settings_tab = SettingsTab::from_name(&tab).unwrap_or(SettingsTab::General);
        }
        // 调试用:启动后直接打开对话框(rename / workspace),方便截图检查
        if let Ok(kind) = std::env::var("LOOM_DIALOG") {
            // 等窗口根层就绪后再开,否则对话框会丢
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(std::time::Duration::from_millis(800)).await;
                let _ = this.update_in(cx, |this, window, cx| this.debug_open_dialog(&kind, window, cx));
            })
            .detach();
        }
        if let Ok(q) = std::env::var("LOOM_PALETTE") {
            wb.toggle_palette(window, cx);
            if let Some(p) = wb.palette.clone() {
                p.update(cx, |p, cx| p.set_query(&q, window, cx));
            }
        }
        // 调试用:通过环境变量直接定位到某个右侧分段 / 文件
        if let Ok(tab) = std::env::var("LOOM_TAB") {
            let tab = RightTab::ALL.into_iter().find(|t| t.key() == tab);
            if let Some(tab) = tab {
                wb.panel.update(cx, |p, cx| p.set_tab(tab, cx));
            }
        }
        if std::env::var("LOOM_IDE").is_ok_and(|v| v == "1") {
            // 独立窗口要新开窗口,推迟到构造完成后
            cx.defer_in(window, |this, window, cx| this.toggle_ide(window, cx));
        }
        if let Ok(file) = std::env::var("LOOM_FILE") {
            if std::env::var("LOOM_IDE").is_ok_and(|v| v == "1") {
                // IDE 窗口在上面的 defer 里才创建,文件要等它建好再打开(连续 defer 两次保证顺序)
                cx.defer_in(window, move |_, window, cx| cx.defer_in(window, move |this, window, cx| this.open_file(file, window, cx)));
            } else {
                wb.open_file(file, window, cx);
            }
        }
        if let Ok(page) = std::env::var("LOOM_PAGE") {
            match page.as_str() {
                "canvas" => wb.page = Page::Canvas,
                "automation" => wb.page = Page::Automation,
                "browser" => wb.page = Page::Browser,
                "customize" => wb.page = Page::Customize,
                _ => {}
            }
        }
        // 调试用:LOOM_CUSTOMIZE=plugins|mcp|skills|commands|agents|rules|hooks 选中自定义页的标签
        if let Some(t) = std::env::var("LOOM_CUSTOMIZE").ok().and_then(|n| settings::CustomizeTab::from_name(&n)) {
            wb.page = Page::Customize;
            wb.customize_tab = t;
        }
        if let Ok(url) = std::env::var("LOOM_URL") {
            wb.page = Page::Browser;
            wb.browser.update(cx, |b, cx| b.open(&url, window, cx));
        }
        wb
    }

    /// 会话行的实时状态点。
    pub fn live_badge(&self, id: &str, stored: SessionBadge, cx: &App) -> SessionBadge {
        match self.views.get(id) {
            Some(v) => {
                let st = &v.model.read(cx).state;
                if st.awaiting_user() {
                    SessionBadge::Waiting
                } else if st.phase == Phase::Running {
                    SessionBadge::Running
                } else if self.unread.contains(id) {
                    SessionBadge::Unread
                } else {
                    SessionBadge::Idle
                }
            }
            None if self.unread.contains(id) => SessionBadge::Unread,
            None => stored,
        }
    }

    pub fn active_model(&self) -> Option<&Entity<SessionModel>> {
        self.active.as_ref().and_then(|id| self.views.get(id)).map(|v| &v.model)
    }

    pub fn toggle_project(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(p) = self.projects.get_mut(ix) {
            p.expanded = !p.expanded;
            // 点选项目即把它设为当前项目,没有会话时右侧文件树与 IDE 也跟着它
            self.focus_project = Some(p.path.clone());
            self.sync_panel(cx);
            cx.notify();
        }
    }

    pub fn set_page(&mut self, page: Page, cx: &mut Context<Self>) {
        if matches!(page, Page::Settings | Page::Customize) && self.page != page {
            // 进入设置 / 自定义时重读 Claude 配置,外部修改(如 claude mcp add)能立刻看到
            self.reload_claude_cfg();
            self.refresh_plugins();
        }
        self.page = page;
        cx.notify();
    }

    /// 打开 / 关闭命令面板。
    pub fn toggle_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.take().is_some() {
            self._palette_sub = None;
            window.focus(&self.focus, cx);
            cx.notify();
            return;
        }
        let items = self.palette_items(cx);
        let palette = cx.new(|cx| Palette::new(items, window, cx));
        self._palette_sub = Some(cx.subscribe_in(&palette, window, |this, _, ev: &PaletteEvent, window, cx| match ev {
            PaletteEvent::Close => this.toggle_palette(window, cx),
            PaletteEvent::Run(action) => {
                let action = action.clone();
                this.toggle_palette(window, cx);
                this.run_palette_action(action, window, cx);
            }
        }));
        self.palette = Some(palette);
        cx.notify();
    }

    /// 汇总面板条目:命令、会话、当前项目文件、演示场景。
    fn palette_items(&self, cx: &App) -> Vec<PaletteItem> {
        let mut items = Vec::new();
        let mut cmd = |title: &str, sub: &str, ic: IconName, action: PaletteAction| {
            items.push(PaletteItem { title: title.into(), subtitle: sub.into(), group: Group::Command, icon: ic, action });
        };
        cmd("新会话", "⌘N", IconName::Plus, PaletteAction::NewSession);
        cmd("打开设置", "⌘,", IconName::Settings, PaletteAction::OpenSettings(None));
        for tab in SettingsTab::all().filter(|t| *t != SettingsTab::General) {
            cmd(&format!("设置:{}", tab.label()), "", tab.icon(), PaletteAction::OpenSettings(Some(tab.key())));
        }
        cmd("自定义:插件", "", IconName::Plug, PaletteAction::OpenCustomize("plugins"));
        cmd("自定义:MCP 服务器", "", IconName::Plug, PaletteAction::OpenCustomize("mcp"));
        cmd("自定义:技能", "", IconName::Sparkles, PaletteAction::OpenCustomize("skills"));
        cmd("自定义:斜杠命令", "", IconName::SquareTerminal, PaletteAction::OpenCustomize("commands"));
        cmd("自定义:子 Agent", "", IconName::Bot, PaletteAction::OpenCustomize("agents"));
        cmd("自定义:规则与记忆 CLAUDE.md", "", IconName::Brain, PaletteAction::OpenCustomize("rules"));
        cmd("自定义:钩子", "", IconName::Webhook, PaletteAction::OpenCustomize("hooks"));
        cmd("切换权限模式", "⇧Tab", IconName::Shield, PaletteAction::CycleMode);
        cmd("切换 IDE 模式", "⇧⌘E", IconName::Code, PaletteAction::ToggleIde);
        cmd("显示 / 隐藏左侧栏", "⌘B", IconName::PanelLeft, PaletteAction::ToggleLeft);
        cmd("显示 / 隐藏右侧面板", "⌥⌘B", IconName::PanelRight, PaletteAction::ToggleRight);
        cmd("主题:纸感浅色", "", IconName::Sun, PaletteAction::SetTheme(ThemeChoice::PaperLight));
        cmd("主题:纸感深色", "", IconName::Moon, PaletteAction::SetTheme(ThemeChoice::PaperDark));
        cmd("主题:One 浅色", "", IconName::Sun, PaletteAction::SetTheme(ThemeChoice::OneLight));
        cmd("主题:One 深色", "", IconName::Moon, PaletteAction::SetTheme(ThemeChoice::OneDark));
        cmd("主题:跟随系统", "", IconName::Monitor, PaletteAction::SetTheme(ThemeChoice::System));
        cmd("打开 Canvas 画布", "", IconName::LayoutDashboard, PaletteAction::OpenPage("canvas"));
        cmd("打开自动化", "", IconName::Timer, PaletteAction::OpenPage("automation"));
        cmd("打开自定义(插件 / MCP / 技能 / 命令…)", "", IconName::SlidersHorizontal, PaletteAction::OpenPage("customize"));
        cmd("打开内置浏览器", "", IconName::Globe, PaletteAction::OpenPage("browser"));
        for (pi, p) in self.projects.iter().enumerate() {
            items.push(PaletteItem { title: p.name.clone(), subtitle: p.path.clone(), group: Group::Project, icon: IconName::Folder, action: PaletteAction::OpenProject(pi) });
        }
        for (pi, p) in self.projects.iter().enumerate() {
            for s in &p.sessions {
                items.push(PaletteItem { title: s.title.clone(), subtitle: p.name.clone(), group: Group::Session, icon: IconName::MessageSquare, action: PaletteAction::OpenSession(pi, s.id.clone()) });
            }
        }
        if let Some(root) = self.active_project_path() {
            for f in crate::palette_search::collect_files(std::path::Path::new(&root), 3000, &self.settings.intel) {
                items.push(PaletteItem { title: f.rsplit('/').next().unwrap_or(&f).to_string(), subtitle: f.clone(), group: Group::File, icon: IconName::FileText, action: PaletteAction::OpenFile(f) });
            }
        }
        if self.demo {
            for sc in loom_demo::scenarios::all() {
                items.push(PaletteItem { title: sc.title.into(), subtitle: format!("{} · {}", sc.group, sc.id), group: Group::Scenario, icon: IconName::Play, action: PaletteAction::RunScenario(sc.id.into()) });
            }
        }
        let _ = cx;
        items
    }

    /// 执行面板动作。
    fn run_palette_action(&mut self, action: PaletteAction, window: &mut Window, cx: &mut Context<Self>) {
        match action {
            PaletteAction::NewSession => self.new_session(self.target_project(), window, cx),
            PaletteAction::ToggleLeft => { self.left_open = !self.left_open; cx.notify(); }
            PaletteAction::ToggleRight => { self.right_open = !self.right_open; cx.notify(); }
            PaletteAction::ToggleIde => self.toggle_ide(window, cx),
            PaletteAction::CycleMode => {
                if let Some(m) = self.active_model().cloned() {
                    m.update(cx, |m, cx| { let next = m.state.mode.next(); m.set_mode(next, cx); });
                }
            }
            PaletteAction::OpenCustomize(tab) => {
                if let Some(t) = settings::CustomizeTab::from_name(tab) {
                    self.customize_tab = t;
                }
                self.set_page(Page::Customize, cx);
            }
            PaletteAction::OpenSettings(tab) => {
                if let Some(tab) = tab {
                    self.settings_tab = SettingsTab::from_name(&tab).unwrap_or(SettingsTab::General);
                }
                self.set_page(Page::Settings, cx);
            }
            PaletteAction::OpenPage(p) => self.set_page(match p { "canvas" => Page::Canvas, "automation" => Page::Automation, "customize" => Page::Customize, _ => Page::Browser }, cx),
            PaletteAction::SetTheme(c) => self.update_settings(cx, |s| s.theme = c),
            PaletteAction::OpenSession(pi, id) => self.open_session(pi, &id, window, cx),
            PaletteAction::OpenProject(pi) => self.open_project_from_palette(pi, window, cx),
            PaletteAction::OpenFile(path) => self.open_file(path, window, cx),
            PaletteAction::RunScenario(id) => self.open_scenario(&id, window, cx),
        }
    }

    /// 从面板打开项目:展开侧栏分组,有会话则打开最近一个(非归档),否则新建会话。
    fn open_project_from_palette(&mut self, pi: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = self.projects.get_mut(pi) else { return };
        p.expanded = true;
        match p.sessions.iter().find(|s| !s.archived).map(|s| s.id.clone()) {
            Some(id) => self.open_session(pi, &id, window, cx),
            None => self.new_session(pi, window, cx),
        }
    }

    /// 打开一个演示场景:已在侧栏则直接打开,否则临时插入第一个项目。
    pub fn open_scenario(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(pi) = self.projects.iter().position(|p| p.sessions.iter().any(|s| s.id == id)) {
            self.open_session(pi, id, window, cx);
        } else if let Some(sc) = loom_demo::scenarios::find(id) {
            self.projects[0].sessions.insert(0, SessionMeta { id: id.into(), title: sc.title.into(), updated: "刚刚".into(), pinned: false, archived: false, badge: SessionBadge::Idle });
            self.projects[0].expanded = true;
            self.open_session(0, id, window, cx);
        }
    }

    /// 打开(必要时创建)一个会话。
    pub fn open_session(&mut self, project_ix: usize, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.page = Page::Chat;
        if !self.views.contains_key(id) {
            let cwd = self.projects[project_ix].path.clone();
            let live = id == "showcase-tour" || id == "thinking-time";
            let is_new = self.new_sessions.contains(id);
            // 分叉会话:恢复源会话并分叉,界面先回放源会话的历史
            let fork_src = self.forks.get(id).cloned();
            // 多根工作区:其余文件夹授权访问,并告诉 claude 它们在哪
            let workspace = self.ui_state.workspace_by_dir(&cwd).cloned();
            let s = &self.settings;
            let (backend, config): (Box<dyn AgentBackend>, BackendConfig) = if self.demo {
                // 历史会话瞬时回放,只有「进行中」的会话保持真实节奏
                (Box::new(DemoBackend { speed: if live { 1.0 } else { 0.0 } }), BackendConfig { cwd: cwd.clone(), ..Default::default() })
            } else {
                // 真实模式:惰性启动 claude;新会话预分配 id,历史会话 --resume
                let config = BackendConfig {
                    cwd: cwd.clone(),
                    resume: fork_src.clone().or_else(|| (!is_new).then(|| id.to_string())),
                    session_id: (is_new || fork_src.is_some()).then(|| id.to_string()),
                    fork_session: fork_src.is_some(),
                    model: (s.default_model != "default").then(|| s.default_model.clone()),
                    mode: (s.default_mode != PermissionMode::Default).then_some(s.default_mode),
                    effort: (s.default_effort != Effort::Medium).then_some(s.default_effort),
                    add_dirs: workspace.as_ref().map(|w| w.folders.clone()).unwrap_or_default(),
                    // 多根工作区说明 + 设置里的「追加系统提示词」
                    append_system_prompt: Some([workspace.as_ref().map(|w| w.system_prompt()).unwrap_or_default(), s.append_prompt.clone()].into_iter().filter(|p| !p.trim().is_empty()).collect::<Vec<_>>().join("\n\n")).filter(|p| !p.is_empty()),
                    extra_env: self.session_extra_env(),
                };
                (Box::new(LazyBackend::new(ClaudeBackend::new(s.claude_path.clone()))), config)
            };
            let handle = match backend.start(config) {
                Ok(h) => h,
                Err(e) => {
                    loom_config::diag::error(&format!("启动后端失败:{e}"));
                    return;
                }
            };
            let model = cx.new(|cx| SessionModel::new(handle, cx));
            let composer = cx.new(|cx| Composer::new(model.clone(), self.demo, window, cx));
            self.push_extensions(&composer, &self.work_root(&cwd), cx);
            if !self.demo {
                let ctx = crate::composer_send::RewriteCtx { claude_path: self.settings.claude_path.clone(), cwd: cwd.clone().into(), env: self.session_extra_env() };
                composer.update(cx, |c, _| c.set_rewrite_ctx(ctx));
            }
            // 调试用:预填输入框,方便截图检查补全
            if let Ok(text) = std::env::var("LOOM_PREFILL") {
                composer.update(cx, |c, cx| c.set_text(&text, window, cx));
            }
            // 调试用:打开后自动发送一条提示,用于端到端验证真实后端
            if let Ok(text) = std::env::var("LOOM_AUTOSEND") {
                model.update(cx, |m, cx| m.send_prompt(text, cx));
            }
            let subs = vec![
                cx.observe(&model, |_, _, cx| cx.notify()),
                cx.subscribe(&model, |this, m, ev: &UiRequest, cx| this.on_ui_request(Some(m.entity_id()), ev, cx)),
            ];
            // 新会话套用偏好里的默认模型 / 模式 / 强度
            let (dm, dmode, deff) = (self.settings.default_model.clone(), self.settings.default_mode, self.settings.default_effort);
            let demo = self.demo;
            model.update(cx, |m, cx| {
                // 演示后端要靠命令切换;真实后端已经通过启动参数带上,这里只同步界面状态
                if dm != "default" {
                    if demo { m.set_model(dm, cx) } else { m.state.model = dm }
                }
                if dmode != PermissionMode::Default {
                    if demo { m.set_mode(dmode, cx) } else { m.state.mode = dmode }
                }
                if deff != Effort::Medium {
                    if demo { m.set_effort(deff, cx) } else { m.state.effort = deff }
                }
            });
            // 恢复历史会话:后台解析磁盘记录,回放进时间线
            if !self.demo && !is_new {
                // 分叉会话回放的是源会话的历史(它自己的文件还不存在)
                let (model, cwd, sid) = (model.clone(), cwd.clone(), fork_src.clone().unwrap_or_else(|| id.to_string()));
                cx.spawn(async move |_this, cx| {
                    let events = cx.background_spawn(async move { crate::real::replay(&cwd, &sid) }).await;
                    let _ = model.update(cx, |m, cx| m.load_history(events, cx));
                })
                .detach();
            }
            if loom_demo::scenarios::find(id).is_some() {
                model.update(cx, |m, _| m.handle.send(AgentCommand::RunScenario(id.to_string())));
            }
            self.views.insert(id.to_string(), SessionView { model, composer, project_ix, _subs: subs });
        }
        self.unread.remove(id);
        self.active = Some(id.to_string());
        self.focus_project = self.projects.get(project_ix).map(|p| p.path.clone());
        if !self.nav_navigating {
            self.nav.visit(id.to_string());
        }
        crate::view_cache::touch(&mut self.view_order, id);
        self.evict_idle_views(cx);
        if let Some(dir) = self.active_git_dir() {
            self.refresh_git(dir, cx);
        }
        self.sync_panel(cx);
        // 调试用:打开会话后在终端里运行一条命令(只触发一次)
        if let Ok(cmd) = std::env::var("LOOM_TERM_CMD") {
            static ONCE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if !ONCE.swap(true, std::sync::atomic::Ordering::SeqCst) {
                self.panel.update(cx, |p, cx| {
                    p.set_tab(RightTab::Terminal, cx);
                    p.run_command(cmd, cx);
                });
            }
        }
        cx.notify();
    }

    /// 修改偏好:立即生效并落盘。
    pub fn update_settings(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut AppSettings)) {
        let (old_theme, old_accent, old_expand) = (self.settings.theme, self.settings.accent.clone(), self.settings.expand_thinking);
        let old_layout = (self.settings.appearance.chat_width, self.settings.appearance.timeline_font_pct, self.settings.appearance.scrollbar);
        f(&mut self.settings);
        if let Err(e) = self.settings.save(&self.settings_path) {
            eprintln!("保存设置失败:{e}");
        }
        cx.set_global(crate::UiPrefs::from_settings(&self.settings));
        cx.set_global(crate::Prefs(self.settings.clone()));
        if self.settings.accent != old_accent {
            cx.set_global(theme::Accent(self.settings.accent.as_deref().and_then(theme::parse_hex)));
        }
        if self.settings.theme != old_theme || self.settings.accent != old_accent {
            let kind = theme::ThemeKind::from(self.settings.theme);
            cx.defer(move |cx| theme::apply(kind, None, cx));
        }
        // 只有「默认展开思考」会改变对话流行高,其它偏好不必重新测量全部会话
        let layout = (self.settings.appearance.chat_width, self.settings.appearance.timeline_font_pct, self.settings.appearance.scrollbar);
        if layout.2 != old_layout.2 {
            crate::appearance::apply_scrollbar(layout.2, cx);
        }
        // 内容宽度与正文字号会改变折行,需要重新测量
        if self.settings.expand_thinking != old_expand || layout.0 != old_layout.0 || layout.1 != old_layout.1 {
            for v in self.views.values() {
                v.model.update(cx, |m, cx| m.remeasure_all(cx));
            }
        }
        cx.notify();
    }

    /// 当前项目路径。
    ///
    /// 优先取当前会话所在项目;没有会话时取侧边栏点选的项目;再没有取第一个(最近的)项目。
    pub fn active_project_path(&self) -> Option<String> {
        self.current_project_ix().and_then(|ix| self.projects.get(ix)).map(|p| self.work_root(&p.path))
    }

    /// 当前项目的下标(规则见 `active_project_path`)。
    pub fn current_project_ix(&self) -> Option<usize> {
        self.active.as_ref().and_then(|id| self.views.get(id)).map(|v| v.project_ix)
            .or_else(|| self.focus_project.as_ref().and_then(|f| self.projects.iter().position(|p| &p.path == f)))
            .or_else(|| (!self.projects.is_empty()).then_some(0))
    }

    /// 后台检测本机 claude 版本。
    pub fn detect_claude(&mut self, cx: &mut Context<Self>) {
        let custom = self.settings.claude_path.clone();
        cx.spawn(async move |this, cx| {
            let info = cx
                .background_spawn(async move {
                    let bin = custom.unwrap_or_else(|| "claude".into());
                    std::process::Command::new(&bin).arg("--version").output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.claude_info = Some(info.unwrap_or_else(|| "未找到 claude 命令".into()));
                cx.notify();
            });
        })
        .detach();
    }

    /// 让右侧面板跟随当前会话。
    fn sync_panel(&mut self, cx: &mut Context<Self>) {
        let model = self.active_model().cloned();
        if let Some(root) = self.active_project_path() {
            self.panel.update(cx, |p, cx| p.set_session(model, root, cx));
        }
    }

    /// 新会话默认落在哪个项目:当前会话所在项目,否则第一个。
    pub fn target_project(&self) -> usize {
        self.current_project_ix().unwrap_or(0)
    }

    /// 在指定项目里新建会话。真实模式没有任何项目时,先让用户选一个目录。
    pub fn new_session(&mut self, project_ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.projects.is_empty() && !self.demo {
            self.add_project(window, cx);
            return;
        }
        self.new_counter += 1;
        // 真实模式的会话 id 同时是 claude 的 session id,必须是 UUID
        let id = if self.demo { format!("new-{}", self.new_counter) } else { uuid::Uuid::new_v4().to_string() };
        if !self.demo {
            self.new_sessions.insert(id.clone());
        }
        if let Some(p) = self.projects.get_mut(project_ix) {
            p.expanded = true;
            p.sessions.insert(0, SessionMeta { id: id.clone(), title: "新会话".into(), updated: "刚刚".into(), pinned: false, archived: false, badge: SessionBadge::Idle });
        }
        self.open_session(project_ix, &id, window, cx);
        if let Some(v) = self.views.get(&id) {
            v.composer.update(cx, |c, cx| c.focus(window, cx));
        }
    }

    // ---------- 真实模式:项目与历史 ----------

    /// 后台读取全部历史项目,完成后合并进侧栏。
    pub fn reload_projects(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let recent = self.settings.recent_projects.clone();
        cx.spawn_in(window, async move |this, cx| {
            let projects = cx.background_spawn(async move { crate::real::load_projects(&recent) }).await;
            let _ = this.update_in(cx, |this, window, cx| this.apply_projects(projects, window, cx));
        })
        .detach();
    }

    /// 用新读到的项目列表替换侧栏,同时保住已打开会话的归属与展开状态。
    fn apply_projects(&mut self, mut projects: Vec<Project>, window: &mut Window, cx: &mut Context<Self>) {
        // 已打开会话所在项目的路径(按会话 id),用于重排后重新定位
        let owner: HashMap<String, String> = self.views.iter().filter_map(|(id, v)| Some((id.clone(), self.projects.get(v.project_ix)?.path.clone()))).collect();
        for (id, path) in &owner {
            if !projects.iter().any(|p| &p.path == path) {
                let name = project_name_from_path(path);
                projects.push(Project { name, path: path.clone(), branch: None, sessions: Vec::new(), expanded: true });
            }
            let _ = id;
        }
        for p in projects.iter_mut() {
            if self.projects.iter().any(|old| old.path == p.path && old.expanded) {
                p.expanded = true;
            }
        }
        // 工作区先并进项目列表,再让置顶项目排前;之后下面的循环按路径恢复已打开会话的项目下标
        self.ui_state.apply_workspaces(&mut projects);
        self.ui_state.apply_projects(&mut projects);
        self.projects = projects;
        self.apply_ui_state();
        for (id, v) in self.views.iter_mut() {
            if let Some(ix) = owner.get(id).and_then(|path| self.projects.iter().position(|p| &p.path == path)) {
                v.project_ix = ix;
            }
        }
        self.merge_live_sessions(cx);
        let list: Vec<(String, String)> = self.projects.iter().map(|p| (p.name.clone(), p.path.clone())).collect();
        self.automation.update(cx, |a, cx| a.set_projects(list, cx));
        if let Some(id) = self.pending_open.take() {
            if let Some(pi) = self.projects.iter().position(|p| p.sessions.iter().any(|s| s.id == id)) {
                self.open_session(pi, &id, window, cx);
            }
        }
        cx.notify();
    }

    /// 已打开但还没写进历史的新会话,补回侧栏最上面。
    fn merge_live_sessions(&mut self, cx: &mut Context<Self>) {
        for (id, v) in &self.views {
            let Some(p) = self.projects.get_mut(v.project_ix) else { continue };
            if p.sessions.iter().any(|s| &s.id == id) {
                continue;
            }
            let title = v.model.read(cx).state.title.clone();
            let title = if title.is_empty() { "新会话".to_string() } else { title };
            p.sessions.insert(0, SessionMeta { id: id.clone(), title, updated: "刚刚".into(), pinned: false, archived: false, badge: SessionBadge::Idle });
        }
        // 新插入的会话(例如分叉出来、还没写盘的)也要套上自定义名
        self.apply_ui_state();
    }

    /// 一个回合结束后,后台刷新该项目的会话列表与分支。
    pub fn refresh_project(&mut self, path: String, cx: &mut Context<Self>) {
        let query = path.clone();
        cx.spawn(async move |this, cx| {
            let (sessions, branch) = cx.background_spawn(async move { crate::real::load_project_sessions(&query) }).await;
            let _ = this.update(cx, |this, cx| {
                if let Some(p) = this.projects.iter_mut().find(|p| p.path == path) {
                    p.sessions = sessions;
                    p.branch = branch;
                }
                this.apply_ui_state();
                this.merge_live_sessions(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// 选一个目录加入项目列表,并在其中新建会话。
    pub fn add_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("选择项目目录".into()) });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let _ = this.update_in(cx, |this, window, cx| this.register_project(path.to_string_lossy().to_string(), window, cx));
        })
        .detach();
    }

    pub fn register_project(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        // 之前被移除的项目重新添加时恢复显示
        if self.ui_state.hidden_projects.contains(&path) {
            self.ui_state.unhide_project(&path);
            self.persist_ui_state();
        }
        self.update_settings(cx, |s| s.touch_project(&path));
        let ix = match self.projects.iter().position(|p| p.path == path) {
            Some(ix) => ix,
            None => {
                self.projects.insert(0, Project { name: project_name_from_path(&path), branch: crate::real::git_branch(&path), path: path.clone(), sessions: Vec::new(), expanded: true });
                // 插在最前,已打开会话的项目下标整体后移
                for v in self.views.values_mut() {
                    v.project_ix += 1;
                }
                0
            }
        };
        let list: Vec<(String, String)> = self.projects.iter().map(|p| (p.name.clone(), p.path.clone())).collect();
        self.automation.update(cx, |a, cx| a.set_projects(list, cx));
        self.new_session(ix, window, cx);
    }

    /// 处理会话模型发来的界面请求;`source` 是发出请求的会话模型实体。
    fn on_ui_request(&mut self, source: Option<EntityId>, ev: &UiRequest, cx: &mut Context<Self>) {
        match ev {
            UiRequest::OpenChanges => {
                self.right_open = true;
                self.panel.update(cx, |p, cx| p.set_tab(RightTab::Changes, cx));
            }
            UiRequest::OpenCanvas(id) => {
                self.page = Page::Canvas;
                let id = id.clone();
                self.canvas.update(cx, |c, cx| c.select(&id, cx));
            }
            UiRequest::SessionSettled => {
                if !self.demo {
                    // 刷新发出事件的会话所属项目,而不是当前激活会话的项目
                    let owner = source.and_then(|sid| self.views.iter().find(|(_, v)| v.model.entity_id() == sid));
                    // 不是当前会话跑完的 → 标记未读
                    if let Some((id, _)) = owner.filter(|(id, _)| self.active.as_ref() != Some(*id)) {
                        self.unread.insert(id.clone());
                    }
                    // 窗口不在前台时发系统通知
                    if self.settings.notify_on_finish && cx.active_window().is_none() {
                        let title = owner.and_then(|(id, v)| self.projects.get(v.project_ix).and_then(|p| p.sessions.iter().find(|s| &s.id == id)).map(|s| s.title.clone()));
                        crate::app_icon::notify("Claude 已完成", &title.unwrap_or_else(|| "会话回合已结束".into()));
                    }
                    let path = owner.and_then(|(_, v)| self.projects.get(v.project_ix)).map(|p| p.path.clone());
                    // 一个回合结束,会话已经写进磁盘:之后再打开就是普通会话,不能再从源会话分叉
                    if let Some(id) = owner.map(|(id, _)| id.clone()) {
                        self.forks.remove(&id);
                    }
                    if let Some(path) = path {
                        let root = self.work_root(&path);
                        self.refresh_git(root, cx);
                        self.refresh_project(path, cx);
                    }
                }
            }
        }
        cx.notify();
    }

    fn on_panel_event(&mut self, ev: &PanelEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev {
            PanelEvent::OpenFile(path) | PanelEvent::OpenFilePinned(path) => {
                let path = path.clone();
                self.panel.update(cx, |p, cx| p.files.load_preview(path, window, cx));
            }
        }
    }

    // ---------- IDE ----------

    /// 切换到 IDE。融合模式:本窗口原地切换(右侧面板自动切到「文件」);
    /// 非融合模式:打开或聚焦独立的 IDE 窗口。
    pub fn toggle_ide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.settings.fused_ide {
            self.open_ide_window(window, cx);
            return;
        }
        self.ide = !self.ide;
        if self.ide {
            self.page = Page::Chat;
            if self.ide_shell.is_none() {
                let wb = cx.entity();
                let root = self.active_project_path().unwrap_or_default();
                let demo = self.demo;
                self.ide_shell = Some(cx.new(|cx| IdeWindow::new(wb, demo, root, window, cx)));
            }
        } else {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    /// 打开独立 IDE 窗口;已存在则直接聚焦。
    fn open_ide_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.agent_window = Some(window.window_handle());
        if let Some(handle) = self.ide_window
            && handle.update(cx, |_, w, _| w.activate_window()).is_ok()
        {
            return;
        }
        let wb = cx.entity();
        let demo = self.demo;
        let root = self.active_project_path().unwrap_or_default();
        // 新窗口首帧会回读本工作台,必须等当前更新结束后再创建
        cx.defer(move |cx| {
            let mut options = TitleBar::window_options();
            // 红绿灯位置与 Agent 窗口一致
            crate::window_header::apply_traffic_light(&mut options);
            options.window_bounds = Some(crate::window_memory::restore("ide", (1280., 820.), cx));
            options.window_min_size = Some(size(px(640.), px(480.)));
            let owner = wb.clone();
            match open_window(options, cx, move |window, cx| {
                let base = root.clone();
                let ide = cx.new(|cx| IdeWindow::new(wb, demo, root, window, cx));
                // 调试用:LOOM_FILE 在独立 IDE 窗口里直接打开该文件(相对路径按项目根补全)
                if let Ok(f) = std::env::var("LOOM_FILE") {
                    let p = PathBuf::from(&f);
                    let full = if p.is_absolute() { p } else { PathBuf::from(base).join(p) };
                    ide.update(cx, |w, cx| w.open_file(full, window, cx));
                }
                ide
            }) {
                Ok((handle, _)) => owner.update(cx, |this, _| this.ide_window = Some(handle)),
                Err(e) => eprintln!("打开 IDE 窗口失败:{e}"),
            }
        });
    }

    /// 融合模式开关变化:开启时收起已打开的独立 IDE 窗口。
    pub fn on_fused_changed(&mut self, cx: &mut Context<Self>) {
        if self.settings.fused_ide
            && let Some(handle) = self.ide_window.take()
        {
            let _ = handle.update(cx, |_, w, _| w.remove_window());
        }
        if !self.settings.fused_ide {
            self.ide = false;
        }
        cx.notify();
    }

    /// 相对路径按当前项目补全。
    fn resolve_path(&self, path: &str) -> PathBuf {
        let p = PathBuf::from(path);
        if p.is_absolute() { p } else { PathBuf::from(self.active_project_path().unwrap_or_default()).join(p) }
    }

    /// 统一的「打开文件」入口:融合 IDE 视图开编辑器,否则在右侧面板预览。
    pub fn open_file(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        self.page = Page::Chat;
        self.right_open = true;
        if let Some(shell) = self.ide_shell.clone().filter(|_| self.ide) {
            let full = self.resolve_path(&path);
            shell.update(cx, |w, cx| w.open_file(full, window, cx));
        } else {
            self.panel.update(cx, |p, cx| p.request_open(path, cx));
        }
        cx.notify();
    }

    /// 保存融合 IDE 里的当前文件。
    pub fn save_ide(&mut self, cx: &mut Context<Self>) {
        if let Some(shell) = self.ide_shell.clone().filter(|_| self.ide) {
            shell.update(cx, |w, cx| w.save(cx));
        }
    }

    /// 当前会话的时间线模型与输入框(供对话栏渲染)。
    pub fn chat_parts(&self) -> Option<(Entity<SessionModel>, Entity<Composer>)> {
        self.active.as_ref().and_then(|id| self.views.get(id)).map(|v| (v.model.clone(), v.composer.clone()))
    }

    /// 当前会话所在项目的 Git 分支。
    pub fn active_branch(&self) -> Option<String> {
        let v = self.active.as_ref().and_then(|id| self.views.get(id))?;
        self.projects.get(v.project_ix).and_then(|p| p.branch.clone())
    }

    fn chat_view(&self, cx: &mut Context<Self>) -> AnyElement {
        chat_panel_with_footer(self.chat_parts(), crate::branch_bar::footer(self, cx), cx)
    }

}

/// 渲染对话区(时间线 + 输入框 + 静态页脚),IDE 窗口使用。
pub fn chat_panel(parts: Option<(Entity<SessionModel>, Entity<Composer>)>, branch: Option<String>, demo: bool, cx: &App) -> AnyElement {
    let t = cx.theme().clone();
    let footer = h_flex().h(px(26.)).px_4().items_center().text_size(px(12.)).text_color(t.muted_foreground)
        .child(h_flex().gap_1().items_center().when_some(branch, |d, b| d.child(icon(IconName::GitBranch, 12., t.muted_foreground)).child(b)))
        .child(div().flex_1())
        .child(if demo { "" } else { "Claude Code" })
        .into_any_element();
    chat_panel_with_footer(parts, footer, cx)
}

/// 渲染对话区(时间线 + 输入框),页脚由调用方提供。Agent 窗口用它放可点击的分支菜单。
pub fn chat_panel_with_footer(parts: Option<(Entity<SessionModel>, Entity<Composer>)>, footer: AnyElement, cx: &App) -> AnyElement {
    let t = cx.theme().clone();
    let Some((model, composer)) = parts else {
        return empty_state(IconName::MessageSquare, "选择或新建一个会话", "左侧选择会话,或点击「新会话」", cx).into_any_element();
    };
    let scroller_state = model.read(cx).scroller.clone();
    let wheel_model = model.clone();
    v_flex().size_full().bg(t.background)
        .child(
            div().flex_1().min_h_0().w_full()
                .on_scroll_wheel(move |ev, _, cx| {
                    let dy = f32::from(ev.delta.pixel_delta(px(20.)).y);
                    wheel_model.update(cx, |m, cx| m.on_wheel(dy, cx));
                })
                .child(
                crate::chat_scroll::chat_scroller("timeline", scroller_state, move |ix, window, cx| timeline::render_row(&model, ix, window, cx), cx),
            ),
        )
        .child(composer)
        .child(footer)
        .into_any_element()
}

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        // 窗口太窄时自动收起侧栏(变宽后恢复)
        self.apply_responsive(window.viewport_size().width);
        self.apply_responsive_right(window.viewport_size().width);
        // 设置是整窗页面:隐藏侧栏、右侧面板与状态栏
        let full_settings = self.page == Page::Settings;
        // 融合模式的 IDE:整窗替换 Agent 布局(标题栏、侧栏、状态栏都是 IDE 的)
        let ide_shell = self.ide_shell.clone().filter(|_| self.ide && !full_settings);
        let show_ide = ide_shell.is_some();
        let center = match self.page {
            Page::Chat => self.chat_view(cx),
            Page::Settings => settings::render(self, window, cx),
            Page::Canvas => self.canvas.clone().into_any_element(),
            Page::Automation => self.automation.clone().into_any_element(),
            Page::Browser => self.browser.clone().into_any_element(),
            Page::Customize => settings::render_customize(self, window, cx),
        };
        v_flex()
            .relative()
            .key_context("Workbench")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &ToggleLeft, _, cx| { this.left_open = !this.left_open; cx.notify(); }))
            .on_action(cx.listener(|this, _: &ToggleRight, _, cx| { this.right_open = !this.right_open; cx.notify(); }))
            .on_action(cx.listener(|this, _: &TogglePalette, window, cx| this.toggle_palette(window, cx)))
            .on_action(cx.listener(|this, _: &crate::term_actions::ToggleTerminal, window, cx| this.toggle_terminal(window, cx)))
            .on_action(cx.listener(|this, _: &crate::term_actions::NewTerminal, window, cx| this.open_new_terminal(window, cx)))
            .on_action(cx.listener(|this, _: &OpenSettings, _, cx| {
                let next = if this.page == Page::Settings { Page::Chat } else { Page::Settings };
                this.set_page(next, cx)
            }))
            .on_action(cx.listener(|this, _: &NewSession, window, cx| this.new_session(this.target_project(), window, cx)))
            .on_action(cx.listener(|this, _: &SaveFile, _, cx| this.save_ide(cx)))
            .on_action(cx.listener(|this, _: &ToggleIde, window, cx| this.toggle_ide(window, cx)))
            .on_action(cx.listener(|this, _: &CycleMode, _, cx| {
                if let Some(m) = this.active_model().cloned() {
                    m.update(cx, |m, cx| { let next = m.state.mode.next(); m.set_mode(next, cx); });
                }
            }))
            .size_full()
            .bg(t.background)
            .text_color(t.foreground)
            // 设置页:上下键切换分类、Esc 清搜索 / 返回
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                if this.page == Page::Settings && this.settings_forms.is_some() && this.settings_key(ev, window, cx) {
                    cx.stop_propagation();
                }
            }))
            // 平滑滚动拦截层:必须是第一个子元素
            .child(crate::smooth_scroll::layer())
            .when_some(ide_shell, |d, shell| d.child(div().flex_1().min_h_0().w_full().child(shell)))
            .when(!show_ide, |d| d
                .child(
                    h_flex().flex_1().min_h_0().w_full().child(
                        h_resizable("agent-split").with_state(&self.split)
                            .on_resize({
                                let wb = cx.entity().downgrade();
                                move |state, _, cx| {
                                    let sizes = state.read(cx).sizes().clone();
                                    let _ = wb.update(cx, |this, cx| this.on_split_resized(&sizes, cx));
                                }
                            })
                            .child(resizable_panel().size(px(self.left_w)).size_range(px(LEFT_MIN)..px(LEFT_MAX)).flex_none().visible(self.left_visible())
                                .child(sidebar::render(self, cx)))
                            .child(resizable_panel().size_range(px(CENTER_MIN)..Pixels::MAX).child(div().size_full().child(v_flex().size_full().when(!full_settings, |d| d.child(self.center_header(cx))).child(div().flex_1().min_h_0().w_full().child(center)))))
                            .child(resizable_panel().size(px(self.right_w)).size_range(px(RIGHT_MIN)..px(RIGHT_MAX)).flex_none().visible(self.right_visible())
                                .child(div().size_full().child(self.panel.clone()))),
                    ),
                ))
            .when(!show_ide && !self.right_open && self.page == Page::Chat, |d| {
                let changes = self.panel.read(cx).change_count(cx);
                d.child(self.right_rail(changes, cx))
            })
            .when_some(self.palette.clone(), |d, p| d.child(p))
    }
}
