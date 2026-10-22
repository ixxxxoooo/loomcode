//! 工作台根视图:标题栏、左侧项目栏、中间会话、右侧固定面板、状态栏。

use crate::automation_page::AutomationPage;
use crate::browser_page::BrowserPage;
use crate::canvas_page::CanvasPage;
use crate::composer::Composer;
use crate::ide::{self, IdeDoc};
use crate::model::{SessionModel, UiRequest};
use crate::palette::{Group, Palette, PaletteAction, PaletteEvent, PaletteItem};
use crate::right_panel::{PanelEvent, RightPanel};
use crate::settings::{self, MemoryDoc, SettingsTab};
use crate::ui::{empty_state, icon};
use crate::{sidebar, theme, timeline};
use gpui_kit::assets::IconName;
use gpui_kit::component::TitleBar;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::message_scroller::MessageScroller;
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::{ActiveTheme, Selectable, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use ucode_automation::{AutomationStore, now_ts};
use ucode_canvas::{CanvasStore, demo_document};
use ucode_config::{AppSettings, ClaudeConfig, ThemeChoice};
use ucode_core::*;
use ucode_claude::{ClaudeBackend, LazyBackend};
use ucode_demo::DemoBackend;
use gpui_kit::component::input::{EditorState, InputState};

actions!(ucode, [TogglePalette, ToggleLeft, ToggleRight, OpenSettings, CycleMode, NewSession, Escape, SaveFile, ToggleIde]);

/// 主区域当前页面。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Chat,
    Settings,
    Canvas,
    Automation,
    Browser,
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
    pub views: HashMap<String, SessionView>,
    pub left_open: bool,
    pub right_open: bool,
    pub left_w: f32,
    pub right_w: f32,
    pub panel: Entity<RightPanel>,
    pub page: Page,
    pub settings: AppSettings,
    pub settings_path: PathBuf,
    pub claude_cfg: ClaudeConfig,
    pub settings_tab: SettingsTab,
    pub memory: Option<MemoryDoc>,
    pub rule_input: Entity<InputState>,
    pub claude_info: Option<String>,
    pub palette: Option<Entity<Palette>>,
    _palette_sub: Option<Subscription>,
    /// IDE 模式:中栏是单文件编辑器 + 对话
    pub ide: bool,
    pub ide_doc: Option<IdeDoc>,
    /// 编辑器打不开文件时的说明
    pub ide_note: Option<String>,
    pub canvas: Entity<CanvasPage>,
    pub automation: Entity<AutomationPage>,
    pub browser: Entity<BrowserPage>,
    _subs: Vec<Subscription>,
    focus: FocusHandle,
    new_counter: u32,
    /// 本次运行里新建、还没写进历史的会话 id(启动时用预分配 id,而不是恢复)
    new_sessions: HashSet<String>,
    /// 历史加载完成后要打开的会话(调试用 `UCODE_SESSION`)
    pending_open: Option<String>,
}

impl Workbench {
    pub fn new(demo: bool, session: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let demo_root = ucode_demo::workspace::default_root();
        // 演示沙盒与演示项目只在演示模式出现;真实模式的项目稍后从 Claude 历史里后台读取
        let projects = if demo {
            if let Err(e) = ucode_demo::workspace::seed(&demo_root) {
                eprintln!("生成演示沙盒失败:{e}");
            }
            ucode_demo::workspace::demo_projects(&demo_root)
        } else {
            Vec::new()
        };
        let settings_path = if demo { demo_root.join("settings.json") } else { AppSettings::default_path().unwrap_or_else(|| demo_root.join("settings.json")) };
        let settings = AppSettings::load(&settings_path);
        let claude_cfg = if demo {
            ucode_config::claude::parse_settings(ucode_demo::workspace::DEMO_CLAUDE_SETTINGS)
        } else {
            let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
            ucode_config::claude::load(&home, None)
        };
        let rule_input = settings::new_rule_input(window, cx);
        let focus = cx.focus_handle();
        let panel_root = if demo { demo_root.to_string_lossy().to_string() } else { std::env::var("HOME").unwrap_or_default() };
        let panel = cx.new(|cx| RightPanel::new(panel_root, demo, window, cx));
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
        let mut wb = Self {
            demo,
            projects,
            active: None,
            views: HashMap::new(),
            left_open: true,
            right_open: settings.right_open,
            left_w: 264.,
            right_w: 400.,
            panel,
            page: Page::Chat,
            settings,
            settings_path,
            claude_cfg,
            settings_tab: SettingsTab::General,
            memory: None,
            rule_input,
            claude_info: None,
            palette: None,
            _palette_sub: None,
            ide: false,
            ide_doc: None,
            ide_note: None,
            canvas,
            automation,
            browser,
            _subs: Vec::new(),
            focus,
            new_counter: 0,
            new_sessions: HashSet::new(),
            pending_open: None,
        };
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
        let kind = match std::env::var("UCODE_THEME").as_deref() {
            Ok("dark") => theme::ThemeKind::PaperDark,
            Ok("system") => theme::ThemeKind::System,
            Ok("light") => theme::ThemeKind::PaperLight,
            _ => theme::ThemeKind::from(wb.settings.theme),
        };
        theme::apply(kind, Some(window), cx);
        cx.set_global(crate::UiPrefs { expand_thinking: wb.settings.expand_thinking });
        if !demo {
            wb.detect_claude(cx);
        }
        if let Ok(tab) = std::env::var("UCODE_SETTINGS") {
            wb.page = Page::Settings;
            wb.settings_tab = SettingsTab::ALL.into_iter().find(|t| format!("{t:?}").to_lowercase() == tab).unwrap_or(SettingsTab::General);
        }
        if let Ok(q) = std::env::var("UCODE_PALETTE") {
            wb.toggle_palette(window, cx);
            if let Some(p) = wb.palette.clone() {
                p.update(cx, |p, cx| p.set_query(&q, window, cx));
            }
        }
        // 调试用:通过环境变量直接定位到某个右侧分段 / 文件
        if let Ok(tab) = std::env::var("UCODE_TAB") {
            let tab = RightTab::ALL.into_iter().find(|t| t.key() == tab);
            if let Some(tab) = tab {
                wb.panel.update(cx, |p, cx| p.set_tab(tab, cx));
            }
        }
        if std::env::var("UCODE_IDE").is_ok_and(|v| v == "1") {
            wb.toggle_ide(window, cx);
        }
        if let Ok(file) = std::env::var("UCODE_FILE") {
            wb.open_file(file, window, cx);
        }
        if let Ok(page) = std::env::var("UCODE_PAGE") {
            match page.as_str() {
                "canvas" => wb.page = Page::Canvas,
                "automation" => wb.page = Page::Automation,
                "browser" => wb.page = Page::Browser,
                _ => {}
            }
        }
        if let Ok(url) = std::env::var("UCODE_URL") {
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
                } else {
                    SessionBadge::Idle
                }
            }
            None => stored,
        }
    }

    pub fn active_model(&self) -> Option<&Entity<SessionModel>> {
        self.active.as_ref().and_then(|id| self.views.get(id)).map(|v| &v.model)
    }

    pub fn toggle_project(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(p) = self.projects.get_mut(ix) {
            p.expanded = !p.expanded;
            cx.notify();
        }
    }

    pub fn set_page(&mut self, page: Page, cx: &mut Context<Self>) {
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
        cmd("设置:权限规则", "", IconName::Shield, PaletteAction::OpenSettings(Some("permissions")));
        cmd("设置:MCP 服务器", "", IconName::Plug, PaletteAction::OpenSettings(Some("mcp")));
        cmd("设置:记忆 CLAUDE.md", "", IconName::Brain, PaletteAction::OpenSettings(Some("memory")));
        cmd("切换权限模式", "⇧Tab", IconName::Shield, PaletteAction::CycleMode);
        cmd("切换 IDE 模式", "⇧⌘E", IconName::Code, PaletteAction::ToggleIde);
        cmd("显示 / 隐藏左侧栏", "⌘B", IconName::PanelLeft, PaletteAction::ToggleLeft);
        cmd("显示 / 隐藏右侧面板", "⌥⌘B", IconName::PanelRight, PaletteAction::ToggleRight);
        cmd("主题:纸感浅色", "", IconName::Sun, PaletteAction::SetTheme(ThemeChoice::PaperLight));
        cmd("主题:纸感深色", "", IconName::Moon, PaletteAction::SetTheme(ThemeChoice::PaperDark));
        cmd("主题:跟随系统", "", IconName::Monitor, PaletteAction::SetTheme(ThemeChoice::System));
        cmd("打开 Canvas 画布", "", IconName::LayoutDashboard, PaletteAction::OpenPage("canvas"));
        cmd("打开自动化", "", IconName::Timer, PaletteAction::OpenPage("automation"));
        cmd("打开内置浏览器", "", IconName::Globe, PaletteAction::OpenPage("browser"));
        for (pi, p) in self.projects.iter().enumerate() {
            for s in &p.sessions {
                items.push(PaletteItem { title: s.title.clone(), subtitle: p.name.clone(), group: Group::Session, icon: IconName::MessageSquare, action: PaletteAction::OpenSession(pi, s.id.clone()) });
            }
        }
        if let Some(root) = self.active_project_path() {
            for f in crate::palette::collect_files(std::path::Path::new(&root), 3000) {
                items.push(PaletteItem { title: f.rsplit('/').next().unwrap_or(&f).to_string(), subtitle: f.clone(), group: Group::File, icon: IconName::FileText, action: PaletteAction::OpenFile(f) });
            }
        }
        if self.demo {
            for sc in ucode_demo::scenarios::all() {
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
            PaletteAction::OpenSettings(tab) => {
                if let Some(tab) = tab {
                    self.settings_tab = SettingsTab::ALL.into_iter().find(|t| format!("{t:?}").to_lowercase() == tab).unwrap_or(SettingsTab::General);
                }
                self.set_page(Page::Settings, cx);
            }
            PaletteAction::OpenPage(p) => self.set_page(match p { "canvas" => Page::Canvas, "automation" => Page::Automation, _ => Page::Browser }, cx),
            PaletteAction::SetTheme(c) => self.update_settings(cx, |s| s.theme = c),
            PaletteAction::OpenSession(pi, id) => self.open_session(pi, &id, window, cx),
            PaletteAction::OpenFile(path) => self.open_file(path, window, cx),
            PaletteAction::RunScenario(id) => self.open_scenario(&id, window, cx),
        }
    }

    /// 打开一个演示场景:已在侧栏则直接打开,否则临时插入第一个项目。
    pub fn open_scenario(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(pi) = self.projects.iter().position(|p| p.sessions.iter().any(|s| s.id == id)) {
            self.open_session(pi, id, window, cx);
        } else if let Some(sc) = ucode_demo::scenarios::find(id) {
            self.projects[0].sessions.insert(0, SessionMeta { id: id.into(), title: sc.title.into(), updated: "刚刚".into(), pinned: false, badge: SessionBadge::Idle });
            self.projects[0].expanded = true;
            self.open_session(0, id, window, cx);
        }
    }

    /// 打开(必要时创建)一个会话。
    pub fn open_session(&mut self, project_ix: usize, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.page = Page::Chat;
        if !self.views.contains_key(id) {
            let cwd = self.projects[project_ix].path.clone();
            let live = id == "showcase-tour";
            let is_new = self.new_sessions.contains(id);
            let s = &self.settings;
            let (backend, config): (Box<dyn AgentBackend>, BackendConfig) = if self.demo {
                // 历史会话瞬时回放,只有「进行中」的会话保持真实节奏
                (Box::new(DemoBackend { speed: if live { 1.0 } else { 0.0 } }), BackendConfig { cwd: cwd.clone(), ..Default::default() })
            } else {
                // 真实模式:惰性启动 claude;新会话预分配 id,历史会话 --resume
                let config = BackendConfig {
                    cwd: cwd.clone(),
                    resume: (!is_new).then(|| id.to_string()),
                    session_id: is_new.then(|| id.to_string()),
                    model: (s.default_model != "default").then(|| s.default_model.clone()),
                    mode: (s.default_mode != PermissionMode::Default).then_some(s.default_mode),
                    effort: (s.default_effort != Effort::Medium).then_some(s.default_effort),
                };
                (Box::new(LazyBackend::new(ClaudeBackend::new(s.claude_path.clone()))), config)
            };
            let handle = match backend.start(config) {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("启动后端失败:{e}");
                    return;
                }
            };
            let model = cx.new(|cx| SessionModel::new(handle, cx));
            let composer = cx.new(|cx| Composer::new(model.clone(), self.demo, window, cx));
            // 调试用:预填输入框,方便截图检查补全
            if let Ok(text) = std::env::var("UCODE_PREFILL") {
                composer.update(cx, |c, cx| c.set_text(&text, window, cx));
            }
            // 调试用:打开后自动发送一条提示,用于端到端验证真实后端
            if let Ok(text) = std::env::var("UCODE_AUTOSEND") {
                model.update(cx, |m, cx| m.send_prompt(text, cx));
            }
            let subs = vec![
                cx.observe(&model, |_, _, cx| cx.notify()),
                cx.subscribe(&model, |this, _m, ev: &UiRequest, cx| this.on_ui_request(ev, cx)),
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
                let (model, cwd, sid) = (model.clone(), cwd.clone(), id.to_string());
                cx.spawn(async move |_this, cx| {
                    let events = cx.background_spawn(async move { crate::real::replay(&cwd, &sid) }).await;
                    let _ = model.update(cx, |m, cx| m.load_history(events, cx));
                })
                .detach();
            }
            if ucode_demo::scenarios::find(id).is_some() {
                model.update(cx, |m, _| m.handle.send(AgentCommand::RunScenario(id.to_string())));
            }
            self.views.insert(id.to_string(), SessionView { model, composer, project_ix, _subs: subs });
        }
        self.active = Some(id.to_string());
        self.sync_panel(cx);
        // 调试用:打开会话后在终端里运行一条命令(只触发一次)
        if let Ok(cmd) = std::env::var("UCODE_TERM_CMD") {
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
        let old_theme = self.settings.theme;
        f(&mut self.settings);
        if let Err(e) = self.settings.save(&self.settings_path) {
            eprintln!("保存设置失败:{e}");
        }
        cx.set_global(crate::UiPrefs { expand_thinking: self.settings.expand_thinking });
        if self.settings.theme != old_theme {
            let kind = theme::ThemeKind::from(self.settings.theme);
            cx.defer(move |cx| theme::apply(kind, None, cx));
        }
        for v in self.views.values() {
            v.model.update(cx, |m, cx| m.remeasure_all(cx));
        }
        cx.notify();
    }

    /// 当前项目路径。
    pub fn active_project_path(&self) -> Option<String> {
        self.active.as_ref().and_then(|id| self.views.get(id)).and_then(|v| self.projects.get(v.project_ix)).map(|p| p.path.clone())
    }

    /// 权限规则文件:项目级 `.claude/settings.local.json`。
    fn rules_file(&self) -> Option<PathBuf> {
        self.active_project_path().map(|p| PathBuf::from(p).join(".claude/settings.local.json"))
    }

    /// 添加权限规则(演示模式只改内存)。
    pub fn add_rule(&mut self, kind: &str, window: &mut Window, cx: &mut Context<Self>) {
        let rule = self.rule_input.read(cx).value().trim().to_string();
        if rule.is_empty() {
            return;
        }
        let list = match kind {
            "allow" => &mut self.claude_cfg.permissions.allow,
            "deny" => &mut self.claude_cfg.permissions.deny,
            _ => &mut self.claude_cfg.permissions.ask,
        };
        if !list.contains(&rule) {
            list.push(rule.clone());
        }
        if !self.demo {
            if let Some(path) = self.rules_file() {
                if let Err(e) = ucode_config::claude::add_permission_rule(&path, kind, &rule) {
                    eprintln!("写入权限规则失败:{e}");
                }
            }
        }
        self.rule_input.update(cx, |i, cx| i.set_value(String::new(), window, cx));
        cx.notify();
    }

    /// 删除权限规则。
    pub fn remove_rule(&mut self, kind: &str, rule: &str, cx: &mut Context<Self>) {
        let list = match kind {
            "allow" => &mut self.claude_cfg.permissions.allow,
            "deny" => &mut self.claude_cfg.permissions.deny,
            _ => &mut self.claude_cfg.permissions.ask,
        };
        list.retain(|r| r != rule);
        if !self.demo {
            if let Some(path) = self.rules_file() {
                if let Err(e) = ucode_config::claude::remove_permission_rule(&path, kind, rule) {
                    eprintln!("删除权限规则失败:{e}");
                }
            }
        }
        cx.notify();
    }

    /// 确保记忆页的编辑器对应当前项目的 CLAUDE.md。
    pub fn ensure_memory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.active_project_path() else { return };
        let path = PathBuf::from(project).join("CLAUDE.md");
        if self.memory.as_ref().is_some_and(|m| m.path == path) {
            return;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let editor = cx.new(|cx| EditorState::new(window, cx).language("markdown").default_value(text));
        // 编辑后标记为未保存
        cx.subscribe(&editor, |this, _, ev: &gpui_kit::component::input::InputEvent, cx| {
            if matches!(ev, gpui_kit::component::input::InputEvent::Change) {
                if let Some(m) = this.memory.as_mut() {
                    m.saved = false;
                    cx.notify();
                }
            }
        })
        .detach();
        self.memory = Some(MemoryDoc { path, editor, saved: true });
    }

    /// 保存记忆文件。
    pub fn save_memory(&mut self, cx: &mut Context<Self>) {
        if let Some(doc) = self.memory.as_mut() {
            let text = doc.editor.read(cx).value().to_string();
            match std::fs::write(&doc.path, text) {
                Ok(()) => doc.saved = true,
                Err(e) => eprintln!("保存 CLAUDE.md 失败:{e}"),
            }
            cx.notify();
        }
    }

    /// 后台检测本机 claude 版本。
    fn detect_claude(&mut self, cx: &mut Context<Self>) {
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
        let root = self.active.as_ref().and_then(|id| self.views.get(id)).and_then(|v| self.projects.get(v.project_ix)).map(|p| p.path.clone());
        if let Some(root) = root {
            self.panel.update(cx, |p, cx| p.set_session(model, root, cx));
        }
    }

    /// 新会话默认落在哪个项目:当前会话所在项目,否则第一个。
    pub fn target_project(&self) -> usize {
        self.active.as_ref().and_then(|id| self.views.get(id)).map(|v| v.project_ix).unwrap_or(0)
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
            p.sessions.insert(0, SessionMeta { id: id.clone(), title: "新会话".into(), updated: "刚刚".into(), pinned: false, badge: SessionBadge::Idle });
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
        self.projects = projects;
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
            p.sessions.insert(0, SessionMeta { id: id.clone(), title, updated: "刚刚".into(), pinned: false, badge: SessionBadge::Idle });
        }
    }

    /// 一个回合结束后,后台刷新该项目的会话列表与分支。
    fn refresh_project(&mut self, path: String, cx: &mut Context<Self>) {
        let query = path.clone();
        cx.spawn(async move |this, cx| {
            let (sessions, branch) = cx.background_spawn(async move { crate::real::load_project_sessions(&query) }).await;
            let _ = this.update(cx, |this, cx| {
                if let Some(p) = this.projects.iter_mut().find(|p| p.path == path) {
                    p.sessions = sessions;
                    p.branch = branch;
                }
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

    fn register_project(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
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

    fn on_ui_request(&mut self, ev: &UiRequest, cx: &mut Context<Self>) {
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
                    if let Some(path) = self.active_project_path() {
                        self.refresh_project(path, cx);
                    }
                }
            }
        }
        cx.notify();
    }

    fn on_panel_event(&mut self, ev: &PanelEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev {
            PanelEvent::OpenFile(path) => {
                if self.ide {
                    self.open_in_ide(path.clone(), window, cx);
                } else {
                    let path = path.clone();
                    self.panel.update(cx, |p, cx| p.files.load_preview(path, window, cx));
                }
            }
        }
    }

    // ---------- IDE 模式 ----------

    /// 切换 IDE 模式:开启时右侧面板自动切到「文件」。
    pub fn toggle_ide(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.ide = !self.ide;
        if self.ide {
            self.page = Page::Chat;
            self.right_open = true;
            self.panel.update(cx, |p, cx| p.set_tab(RightTab::Files, cx));
        }
        cx.notify();
    }

    /// 相对路径按当前项目补全。
    fn resolve_path(&self, path: &str) -> PathBuf {
        let p = PathBuf::from(path);
        if p.is_absolute() { p } else { PathBuf::from(self.active_project_path().unwrap_or_default()).join(p) }
    }

    /// 统一的「打开文件」入口:IDE 模式开编辑器,否则在右侧面板预览。
    pub fn open_file(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        self.page = Page::Chat;
        self.right_open = true;
        if self.ide {
            let full = self.resolve_path(&path);
            self.open_in_ide(full, window, cx);
        } else {
            self.panel.update(cx, |p, cx| p.request_open(path, cx));
        }
        cx.notify();
    }

    pub fn open_in_ide(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.panel.update(cx, |p, _| p.files.reveal(&path));
        match ide::load(&path, window, cx) {
            Ok(doc) => {
                self.ide_doc = Some(doc);
                self.ide_note = None;
            }
            Err(note) => {
                self.ide_doc = None;
                self.ide_note = Some(note);
            }
        }
        cx.notify();
    }

    pub fn save_ide(&mut self, cx: &mut Context<Self>) {
        let Some(doc) = self.ide_doc.as_mut() else { return };
        let text = doc.editor.read(cx).value().to_string();
        match std::fs::write(&doc.path, text) {
            Ok(()) => {
                doc.saved = true;
                self.ide_note = None;
            }
            Err(e) => self.ide_note = Some(format!("保存失败:{e}")),
        }
        cx.notify();
    }

    pub fn reload_ide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self.ide_doc.as_ref().map(|d| d.path.clone()) {
            self.open_in_ide(path, window, cx);
        }
    }

    pub fn close_ide(&mut self, cx: &mut Context<Self>) {
        self.ide_doc = None;
        cx.notify();
    }

    fn title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let title = self.active_model().map(|m| m.read(cx).state.title.clone()).unwrap_or_else(|| "UCode".into());
        TitleBar::new().child(
            h_flex().w_full().items_center().gap_1()
                .child(Button::new("tb-left").small().ghost().icon(if self.left_open { IconName::PanelLeftClose } else { IconName::PanelLeftOpen })
                    .on_click(cx.listener(|this, _, _, cx| { this.left_open = !this.left_open; cx.notify(); })))
                .child(div().flex_1().flex().justify_center().text_size(px(13.)).text_color(t.muted_foreground).child(title))
                .child(Button::new("tb-ide").small().ghost().icon(IconName::Code).selected(self.ide).tooltip("IDE 模式(⇧⌘E)")
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_ide(window, cx))))
                .child(Button::new("tb-right").small().ghost().icon(if self.right_open { IconName::PanelRightClose } else { IconName::PanelRightOpen })
                    .on_click(cx.listener(|this, _, _, cx| { this.right_open = !this.right_open; cx.notify(); }))),
        )
    }

    fn chat_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let Some(view) = self.active.as_ref().and_then(|id| self.views.get(id)) else {
            return empty_state(IconName::MessageSquare, "选择或新建一个会话", "左侧选择会话,或点击「新会话」", cx).into_any_element();
        };
        let model = view.model.clone();
        let scroller_state = model.read(cx).scroller.clone();
        let session = model.clone();
        v_flex().size_full().bg(t.background)
            .child(
                div().flex_1().min_h_0().w_full().child(
                    MessageScroller::new("timeline", scroller_state, move |ix, window, cx| timeline::render_row(&session, ix, window, cx))
                        .size_full()
                        .jump_button(true)
                        .with_row_style(StyleRefinement::default().pb(px(0.)))
                        .with_jump_button_label("跳到最新"),
                ),
            )
            .child(view.composer.clone())
            .into_any_element()
    }

    fn status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let (branch, mode, usage) = match self.active.as_ref().and_then(|id| self.views.get(id)) {
            Some(v) => {
                let st = &v.model.read(cx).state;
                (self.projects.get(v.project_ix).and_then(|p| p.branch.clone()), Some(st.mode), Some(st.usage.clone()))
            }
            None => (None, None, None),
        };
        div().h(px(24.)).w_full().border_t_1().border_color(t.border).bg(t.sidebar).text_size(px(11.)).text_color(t.muted_foreground)
            .child(
                StatusBar::new()
                    .left(h_flex().gap_3().px_2()
                        .child(div().child(if self.demo { "演示模式" } else { "Claude Code" }))
                        .when_some(branch, |d, b| d.child(h_flex().gap_1().items_center().child(icon(IconName::GitBranch, 11., t.muted_foreground)).child(b))))
                    .right(h_flex().gap_3().px_2()
                        .when_some(mode, |d, m| d.child(div().child(format!("权限:{}", m.label()))))
                        .when_some(usage.filter(|u| u.context_size > 0), |d, u| d.child(div().child(format!("上下文 {:.0}% · ${:.2}", u.context_ratio() * 100., u.cost_usd))))),
            )
    }
}

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let center = match self.page {
            Page::Chat if self.ide => h_flex().size_full()
                .child(div().flex_1().min_w_0().h_full().child(ide::render(self, cx)))
                .child(div().w(px(440.)).h_full().flex_none().border_l_1().border_color(t.border).child(self.chat_view(cx)))
                .into_any_element(),
            Page::Chat => self.chat_view(cx),
            Page::Settings => settings::render(self, window, cx),
            Page::Canvas => self.canvas.clone().into_any_element(),
            Page::Automation => self.automation.clone().into_any_element(),
            Page::Browser => self.browser.clone().into_any_element(),
        };
        v_flex()
            .key_context("Workbench")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &ToggleLeft, _, cx| { this.left_open = !this.left_open; cx.notify(); }))
            .on_action(cx.listener(|this, _: &ToggleRight, _, cx| { this.right_open = !this.right_open; cx.notify(); }))
            .on_action(cx.listener(|this, _: &TogglePalette, window, cx| this.toggle_palette(window, cx)))
            .on_action(cx.listener(|this, _: &OpenSettings, _, cx| this.set_page(Page::Settings, cx)))
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
            .child(self.title_bar(cx))
            .child(
                h_flex().flex_1().min_h_0().w_full()
                    .when(self.left_open, |d| d.child(sidebar::render(self, cx)))
                    .child(div().flex_1().min_w_0().h_full().child(center))
                    .when(self.right_open && self.page == Page::Chat, |d| d.child(div().w(px(self.right_w)).h_full().flex_none().border_l_1().border_color(t.border).child(self.panel.clone()))),
            )
            .child(self.status_bar(cx))
            .when_some(self.palette.clone(), |d, p| d.child(p))
    }
}
