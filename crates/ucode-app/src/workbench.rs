//! 工作台根视图:标题栏、左侧项目栏、中间会话、右侧固定面板、状态栏。

use crate::composer::Composer;
use crate::model::{SessionModel, UiRequest};
use crate::ui::{empty_state, icon};
use crate::{sidebar, theme, timeline};
use gpui_kit::assets::IconName;
use gpui_kit::component::TitleBar;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::message_scroller::MessageScroller;
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::collections::HashMap;
use std::path::PathBuf;
use ucode_core::*;
use ucode_demo::DemoBackend;

actions!(ucode, [TogglePalette, ToggleLeft, ToggleRight, OpenSettings, CycleMode, NewSession, Escape]);

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
    pub demo_root: PathBuf,
    pub projects: Vec<Project>,
    pub active: Option<String>,
    pub views: HashMap<String, SessionView>,
    pub left_open: bool,
    pub right_open: bool,
    pub left_w: f32,
    pub right_w: f32,
    pub right_tab: RightTab,
    pub page: Page,
    pub theme: theme::ThemeKind,
    pub palette_open: bool,
    focus: FocusHandle,
    new_counter: u32,
}

impl Workbench {
    pub fn new(demo: bool, session: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // 暂时只有演示后端,真实模式接入后再区分
        let demo_root = ucode_demo::workspace::default_root();
        if let Err(e) = ucode_demo::workspace::seed(&demo_root) {
            eprintln!("生成演示沙盒失败:{e}");
        }
        let projects = ucode_demo::workspace::demo_projects(&demo_root);
        let focus = cx.focus_handle();
        let mut wb = Self {
            demo,
            demo_root,
            projects,
            active: None,
            views: HashMap::new(),
            left_open: true,
            right_open: true,
            left_w: 264.,
            right_w: 400.,
            right_tab: RightTab::Changes,
            page: Page::Chat,
            theme: theme::ThemeKind::PaperLight,
            palette_open: false,
            focus,
            new_counter: 0,
        };
        let id = session.unwrap_or_else(|| "showcase-tour".into());
        if let Some(pi) = wb.projects.iter().position(|p| p.sessions.iter().any(|s| s.id == id)) {
            wb.open_session(pi, &id, window, cx);
        } else if let Some(sc) = ucode_demo::scenarios::find(&id) {
            // 不在侧栏里的场景:临时插入第一个项目并打开
            wb.projects[0].sessions.insert(0, SessionMeta { id: id.clone(), title: sc.title.into(), updated: "刚刚".into(), pinned: false, badge: SessionBadge::Idle });
            wb.open_session(0, &id, window, cx);
        }
        window.focus(&wb.focus, cx);
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

    pub fn toggle_palette(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.palette_open = !self.palette_open;
        cx.notify();
    }

    /// 打开(必要时创建)一个会话。
    pub fn open_session(&mut self, project_ix: usize, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.page = Page::Chat;
        if !self.views.contains_key(id) {
            let cwd = self.projects[project_ix].path.clone();
            let live = id == "showcase-tour";
            // 历史会话瞬时回放,只有「进行中」的会话保持真实节奏
            let backend = DemoBackend { speed: if live { 1.0 } else { 0.0 } };
            let handle = match backend.start(BackendConfig { cwd: cwd.clone(), ..Default::default() }) {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("启动后端失败:{e}");
                    return;
                }
            };
            let model = cx.new(|cx| SessionModel::new(handle, cwd, cx));
            let composer = cx.new(|cx| Composer::new(model.clone(), self.demo, window, cx));
            let subs = vec![
                cx.observe(&model, |_, _, cx| cx.notify()),
                cx.subscribe(&model, |this, _m, ev: &UiRequest, cx| this.on_ui_request(ev, cx)),
            ];
            if ucode_demo::scenarios::find(id).is_some() {
                model.update(cx, |m, _| m.handle.send(AgentCommand::RunScenario(id.to_string())));
            }
            self.views.insert(id.to_string(), SessionView { model, composer, project_ix, _subs: subs });
        }
        self.active = Some(id.to_string());
        cx.notify();
    }

    /// 在指定项目里新建会话。
    pub fn new_session(&mut self, project_ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.new_counter += 1;
        let id = format!("new-{}", self.new_counter);
        if let Some(p) = self.projects.get_mut(project_ix) {
            p.expanded = true;
            p.sessions.insert(0, SessionMeta { id: id.clone(), title: "新会话".into(), updated: "刚刚".into(), pinned: false, badge: SessionBadge::Idle });
        }
        self.open_session(project_ix, &id, window, cx);
        if let Some(v) = self.views.get(&id) {
            v.composer.update(cx, |c, cx| c.focus(window, cx));
        }
    }

    fn on_ui_request(&mut self, ev: &UiRequest, cx: &mut Context<Self>) {
        match ev {
            UiRequest::OpenChanges => {
                self.right_open = true;
                self.right_tab = RightTab::Changes;
            }
            UiRequest::OpenFile(_) => {
                self.right_open = true;
                self.right_tab = RightTab::Files;
            }
            UiRequest::OpenCanvas(_) => self.page = Page::Canvas,
            UiRequest::OpenPlan => {
                self.right_open = true;
                self.right_tab = RightTab::Plan;
            }
        }
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
            Page::Chat => self.chat_view(cx),
            other => empty_state(IconName::Construction, &format!("{other:?} 页面"), "稍后实现", cx).into_any_element(),
        };
        let _ = window;
        v_flex()
            .key_context("Workbench")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &ToggleLeft, _, cx| { this.left_open = !this.left_open; cx.notify(); }))
            .on_action(cx.listener(|this, _: &ToggleRight, _, cx| { this.right_open = !this.right_open; cx.notify(); }))
            .on_action(cx.listener(|this, _: &TogglePalette, window, cx| this.toggle_palette(window, cx)))
            .on_action(cx.listener(|this, _: &OpenSettings, _, cx| this.set_page(Page::Settings, cx)))
            .on_action(cx.listener(|this, _: &NewSession, window, cx| this.new_session(0, window, cx)))
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
                    .when(self.right_open, |d| d.child(crate::right_panel::render(self, cx))),
            )
            .child(self.status_bar(cx))
    }
}
