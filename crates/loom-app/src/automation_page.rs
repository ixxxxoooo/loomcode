//! 自动化页:按计划在项目里无人值守地运行一段提示词。
//!
//! 左侧是自动化列表(带停用开关与下次运行时间),右侧是编辑表单与运行记录。
//! 真实模式下,页面常驻一个 30 秒一次的调度节拍,到期的自动化会调用 `claude -p` 执行;
//! 演示模式只写入示意性的运行记录,不会启动任何进程。

use crate::ui::{MONO, caption, empty_state, icon, pill};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Disableable, Selectable, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::collections::HashSet;
use std::time::Duration;
use loom_automation::*;

/// 调度节拍间隔。
const TICK: Duration = Duration::from_secs(30);

pub struct AutomationPage {
    store: AutomationStore,
    items: Vec<Automation>,
    selected: Option<String>,
    /// 正在编辑的副本;保存后才写盘
    draft: Option<Automation>,
    runs: Vec<RunRecord>,
    running: HashSet<String>,
    demo: bool,
    claude_path: Option<String>,
    /// (名称, 路径)
    projects: Vec<(String, String)>,
    name: Entity<InputState>,
    prompt: Entity<TextareaState>,
    /// 保存校验或读写失败的提示
    message: Option<String>,
}

impl AutomationPage {
    /// 项目列表变化(真实模式下历史加载完成后)时更新可选项目。
    pub fn set_projects(&mut self, projects: Vec<(String, String)>, cx: &mut Context<Self>) {
        self.projects = projects;
        cx.notify();
    }

    pub fn new(store: AutomationStore, demo: bool, claude_path: Option<String>, projects: Vec<(String, String)>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("例如:每日 TODO 巡检"));
        let prompt = cx.new(|cx| TextareaState::new(window, cx).placeholder("要让 Claude 做什么?例如:检查今天新增的 TODO 并汇总"));
        let mut page = Self { store, items: Vec::new(), selected: None, draft: None, runs: Vec::new(), running: HashSet::new(), demo, claude_path, projects, name, prompt, message: None };
        if demo {
            page.seed_demo();
        }
        page.reload();
        if let Some(first) = page.items.first().map(|a| a.id.clone()) {
            page.select(&first, window, cx);
        }
        if !demo {
            // 常驻调度节拍:只要窗口还在,到期的自动化就会被执行
            cx.spawn(async move |this, cx| loop {
                cx.background_executor().timer(TICK).await;
                if this.update(cx, |this, cx| this.tick(cx)).is_err() {
                    break;
                }
            })
            .detach();
        }
        page
    }

    /// 演示数据:三条自动化与若干运行记录(仅在存储为空时写入)。
    fn seed_demo(&mut self) {
        if self.store.load_all().map(|v| !v.is_empty()).unwrap_or(true) {
            return;
        }
        let now = now_ts();
        let project = self.projects.first().map(|p| p.1.clone()).unwrap_or_default();
        let mk = |id: &str, name: &str, prompt: &str, schedule, enabled, mode: &str| Automation {
            id: id.into(),
            name: name.into(),
            prompt: prompt.into(),
            project_path: project.clone(),
            schedule,
            enabled,
            mode: mode.into(),
            model: None,
            created_at: now - 7 * 86_400,
            last_run: Some(now - 3_600 * 5),
        };
        let items = [
            mk("auto-todo", "每日 TODO 巡检", "检查今天新增的 TODO / FIXME 注释,按模块汇总成清单", Schedule::Daily { hour: 9, minute: 0 }, true, "acceptEdits"),
            mk("auto-deps", "每周依赖更新检查", "检查 Cargo 依赖是否有安全更新,只汇报不修改", Schedule::Weekly { weekday: 0, hour: 10, minute: 30 }, true, "dontAsk"),
            mk("auto-build", "每小时构建健康检查", "运行 cargo check,汇报新增的警告", Schedule::Interval { minutes: 60 }, false, "acceptEdits"),
        ];
        for a in items {
            let _ = self.store.upsert(a);
        }
        let run = |ago: i64, status, summary: &str, cost| RunRecord { started_at: now - ago, finished_at: Some(now - ago + 48), status, summary: summary.into(), cost_usd: cost, session_id: None };
        // 按时间从旧到新写入,最新的最终排在最前
        let _ = self.store.record_run("auto-todo", run(3_600 * 53, RunStatus::Failed, "无法启动 claude:鉴权已过期,请重新登录。", 0.0));
        let _ = self.store.record_run("auto-todo", run(3_600 * 29, RunStatus::Succeeded, "今天没有新增 TODO。", 0.012));
        let _ = self.store.record_run("auto-todo", run(3_600 * 5, RunStatus::Succeeded, "发现 6 处新增 TODO:Router 3 处、Metrics 2 处、CLI 1 处。其中 2 处标注了负责人。", 0.041));
        let _ = self.store.record_run("auto-deps", run(86_400 * 2, RunStatus::Succeeded, "serde 与 tokio 有补丁版本可用,均为兼容更新,无安全公告。", 0.067));
    }

    pub fn reload(&mut self) {
        match self.store.load_all() {
            Ok(items) => {
                self.items = items;
                self.message = None;
            }
            Err(e) => self.message = Some(e.to_string()),
        }
    }

    pub fn select(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(a) = self.items.iter().find(|a| a.id == id).cloned() else { return };
        self.selected = Some(a.id.clone());
        self.runs = self.store.runs(&a.id);
        self.name.update(cx, |i, cx| i.set_value(a.name.clone(), window, cx));
        self.prompt.update(cx, |i, cx| i.set_value(a.prompt.clone(), window, cx));
        self.draft = Some(a);
        self.message = None;
        cx.notify();
    }

    fn new_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = now_ts();
        let a = Automation {
            id: format!("auto-{now}"),
            name: String::new(),
            prompt: String::new(),
            project_path: self.projects.first().map(|p| p.1.clone()).unwrap_or_default(),
            schedule: Schedule::Daily { hour: 9, minute: 0 },
            enabled: true,
            mode: "acceptEdits".into(),
            model: None,
            created_at: now,
            last_run: None,
        };
        self.name.update(cx, |i, cx| i.set_value(String::new(), window, cx));
        self.prompt.update(cx, |i, cx| i.set_value(String::new(), window, cx));
        self.selected = Some(a.id.clone());
        self.runs.clear();
        self.draft = Some(a);
        self.message = None;
        cx.notify();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut d) = self.draft.clone() else { return };
        d.name = self.name.read(cx).value().trim().to_string();
        d.prompt = self.prompt.read(cx).value().trim().to_string();
        if d.name.is_empty() || d.prompt.is_empty() {
            self.message = Some("名称和提示词都不能为空".into());
        } else if !d.schedule.is_valid() {
            self.message = Some("运行计划的取值不合法".into());
        } else if d.project_path.is_empty() {
            self.message = Some("请选择一个项目".into());
        } else {
            match self.store.upsert(d.clone()) {
                Ok(()) => {
                    self.reload();
                    self.select(&d.id, window, cx);
                    self.message = Some("已保存".into());
                }
                Err(e) => self.message = Some(e.to_string()),
            }
        }
        cx.notify();
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.selected.take() {
            let _ = self.store.remove(&id);
            self.draft = None;
            self.runs.clear();
            self.reload();
            if let Some(first) = self.items.first().map(|a| a.id.clone()) {
                self.select(&first, window, cx);
            }
            cx.notify();
        }
    }

    fn set_enabled(&mut self, id: &str, enabled: bool, cx: &mut Context<Self>) {
        if let Some(mut a) = self.items.iter().find(|a| a.id == id).cloned() {
            a.enabled = enabled;
            if let Err(e) = self.store.upsert(a) {
                self.message = Some(e.to_string());
            }
            if let Some(d) = self.draft.as_mut().filter(|d| d.id == id) {
                d.enabled = enabled;
            }
            self.reload();
            cx.notify();
        }
    }

    /// 调度节拍:执行所有到期且未在运行的自动化。
    fn tick(&mut self, cx: &mut Context<Self>) {
        let now = now_ts();
        let due: Vec<String> = self.items.iter().filter(|a| a.is_due(now) && !self.running.contains(&a.id)).map(|a| a.id.clone()).collect();
        for id in due {
            self.run_now(&id, cx);
        }
    }

    /// 立即运行一次。
    pub fn run_now(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(auto) = self.items.iter().find(|a| a.id == id).cloned() else { return };
        if self.running.contains(id) {
            return;
        }
        let started = now_ts();
        // 无论成败都先记下开始时间,避免失败后在下个节拍被反复触发
        let mut updated = auto.clone();
        updated.last_run = Some(started);
        let _ = self.store.upsert(updated);

        if self.demo {
            let rec = RunRecord { started_at: started, finished_at: Some(started), status: RunStatus::Succeeded, summary: "演示模式:未真正运行。真实模式下这里会显示 Claude 的最终回复。".into(), cost_usd: 0.0, session_id: None };
            let _ = self.store.record_run(id, rec);
            self.refresh_after_run(id, cx);
            return;
        }
        let Some(claude) = loom_claude::find_claude(self.claude_path.as_deref()) else {
            let rec = RunRecord { started_at: started, finished_at: Some(started), status: RunStatus::Failed, summary: "找不到 claude 可执行文件,请先安装 Claude Code".into(), cost_usd: 0.0, session_id: None };
            let _ = self.store.record_run(id, rec);
            self.refresh_after_run(id, cx);
            return;
        };
        let _ = self.store.record_run(id, RunRecord { started_at: started, finished_at: None, status: RunStatus::Running, summary: String::new(), cost_usd: 0.0, session_id: None });
        self.running.insert(id.to_string());
        self.refresh_after_run(id, cx);

        let (store, run_id) = (self.store.clone(), id.to_string());
        cx.spawn(async move |this, cx| {
            let rec = cx.background_spawn(async move { run_once(&claude, &auto) }).await;
            let _ = store.record_run(&run_id, RunRecord { started_at: started, ..rec });
            let _ = this.update(cx, |this, cx| {
                this.running.remove(&run_id);
                this.refresh_after_run(&run_id, cx);
            });
        })
        .detach();
    }

    fn refresh_after_run(&mut self, id: &str, cx: &mut Context<Self>) {
        self.reload();
        if self.selected.as_deref() == Some(id) {
            self.runs = self.store.runs(id);
        }
        cx.notify();
    }

    // ---------- 渲染 ----------

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        v_flex().w(px(280.)).h_full().flex_none().border_r_1().border_color(t.border).bg(t.sidebar)
            .child(
                h_flex().h(px(36.)).px_3().items_center().text_size(px(12.)).text_color(t.muted_foreground)
                    .child(div().flex_1().child("自动化"))
                    .child(Button::new("auto-new").small().ghost().icon(IconName::Plus).on_click(cx.listener(|this, _, window, cx| this.new_draft(window, cx)))),
            )
            .child(v_flex().id("auto-list").flex_1().min_h_0().overflow_y_scroll().px_2().gap_0p5().children(self.items.iter().map(|a| {
                let on = self.selected.as_deref() == Some(a.id.as_str());
                let (id, id2) = (a.id.clone(), a.id.clone());
                let running = self.running.contains(&a.id);
                let next = match (a.enabled, a.next_run_ts()) {
                    (true, Some(ts)) => format!("下次 {}", format_ts(ts)),
                    _ => "已停用".to_string(),
                };
                h_flex().id(SharedString::from(format!("au-{}", a.id))).gap_2().px_2().py_1p5().rounded(px(6.)).cursor_pointer().items_center()
                    .when(on, |d| d.bg(t.list_active))
                    .hover(|d| d.bg(t.list_hover))
                    .on_click(cx.listener(move |this, _, window, cx| this.select(&id, window, cx)))
                    .child(
                        v_flex().flex_1().min_w_0().gap_0p5()
                            .child(h_flex().gap_1p5().items_center()
                                .child(div().min_w_0().text_size(px(12.5)).truncate().child(a.name.clone()))
                                .when(running, |d| d.child(pill("运行中", t.primary_foreground, t.primary))))
                            .child(caption(format!("{} · {next}", a.schedule.describe()), cx)),
                    )
                    .child(Switch::new(SharedString::from(format!("au-sw-{}", a.id))).checked(a.enabled).on_click(cx.listener(move |this, checked: &bool, _, cx| this.set_enabled(&id2, *checked, cx))))
            })))
    }

    /// 一行分段按钮。
    fn seg<T: PartialEq + Clone + 'static>(&self, key: &str, items: Vec<(T, String)>, current: T, cx: &mut Context<Self>, apply: impl Fn(&mut Automation, T) + Copy + 'static) -> Div {
        h_flex().gap_1().flex_wrap().children(items.into_iter().enumerate().map(|(i, (v, label))| {
            Button::new(SharedString::from(format!("{key}-{i}"))).small().ghost().label(label).selected(v == current).on_click(cx.listener(move |this, _, _, cx| {
                if let Some(d) = this.draft.as_mut() {
                    apply(d, v.clone());
                }
                cx.notify();
            }))
        }))
    }

    fn form(&self, d: &Automation, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();

        // 计划编辑
        #[derive(Clone, Copy, PartialEq)]
        enum Kind {
            Interval,
            Daily,
            Weekly,
        }
        let kind = match d.schedule {
            Schedule::Interval { .. } => Kind::Interval,
            Schedule::Daily { .. } => Kind::Daily,
            Schedule::Weekly { .. } => Kind::Weekly,
        };
        let (hour, minute) = match d.schedule {
            Schedule::Daily { hour, minute } | Schedule::Weekly { hour, minute, .. } => (hour, minute),
            _ => (9, 0),
        };
        let kind_bar = self.seg("au-kind", vec![(Kind::Interval, "间隔".into()), (Kind::Daily, "每天".into()), (Kind::Weekly, "每周".into())], kind, cx, |d, k| {
            d.schedule = match k {
                Kind::Interval => Schedule::Interval { minutes: 60 },
                Kind::Daily => Schedule::Daily { hour: 9, minute: 0 },
                Kind::Weekly => Schedule::Weekly { weekday: 0, hour: 9, minute: 0 },
            }
        });
        let detail: AnyElement = match d.schedule {
            Schedule::Interval { minutes } => self
                .seg("au-int", [15u32, 30, 60, 120, 360].iter().map(|m| (*m, Schedule::Interval { minutes: *m }.describe())).collect(), minutes, cx, |d, m| d.schedule = Schedule::Interval { minutes: m })
                .into_any_element(),
            Schedule::Daily { .. } | Schedule::Weekly { .. } => {
                let weekday = if let Schedule::Weekly { weekday, .. } = d.schedule { Some(weekday) } else { None };
                let set_time = move |d: &mut Automation, h: u32, m: u32, w: Option<u32>| {
                    d.schedule = match w.or(weekday) {
                        Some(wd) if matches!(d.schedule, Schedule::Weekly { .. }) || w.is_some() => Schedule::Weekly { weekday: wd, hour: h, minute: m },
                        _ => Schedule::Daily { hour: h, minute: m },
                    }
                };
                let wd_bar = weekday.map(|wd| {
                    self.seg("au-wd", ["一", "二", "三", "四", "五", "六", "日"].iter().enumerate().map(|(i, n)| (i as u32, format!("周{n}"))).collect(), wd, cx, move |d, w| set_time(d, hour, minute, Some(w)))
                        .into_any_element()
                });
                v_flex().gap_2()
                    .children(wd_bar)
                    .child(
                        h_flex().gap_2().items_center()
                            .child(Button::new("au-h-").small().ghost().icon(IconName::Minus).on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(d) = this.draft.as_mut() { set_time(d, (hour + 23) % 24, minute, None); }
                                cx.notify();
                            })))
                            .child(div().w(px(56.)).flex().justify_center().font_family(MONO).text_size(px(15.)).child(format!("{hour:02}:{minute:02}")))
                            .child(Button::new("au-h+").small().ghost().icon(IconName::Plus).on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(d) = this.draft.as_mut() { set_time(d, (hour + 1) % 24, minute, None); }
                                cx.notify();
                            })))
                            .child(self.seg("au-min", [0u32, 15, 30, 45].iter().map(|m| (*m, format!(":{m:02}"))).collect(), minute, cx, move |d, m| set_time(d, hour, m, None))),
                    )
                    .into_any_element()
            }
        };

        let project_bar = self.seg("au-proj", self.projects.iter().map(|p| (p.1.clone(), p.0.clone())).collect(), d.project_path.clone(), cx, |d, p| d.project_path = p);
        let mode_bar = self.seg(
            "au-mode",
            vec![("acceptEdits".to_string(), "接受编辑".to_string()), ("dontAsk".to_string(), "只读不问".to_string()), ("bypassPermissions".to_string(), "绕过权限".to_string())],
            d.mode.clone(),
            cx,
            |d, m| d.mode = m,
        );

        v_flex().w_full().gap_4()
            .child(field("名称", Input::new(&self.name).into_any_element(), cx))
            .child(field("提示词", Textarea::new(&self.prompt).h(px(120.)).into_any_element(), cx))
            .child(field("项目", project_bar.into_any_element(), cx))
            .child(field("运行计划", v_flex().gap_2().child(kind_bar).child(detail).into_any_element(), cx))
            .child(field("权限模式(无人值守时无法询问)", mode_bar.into_any_element(), cx))
            .child(
                h_flex().gap_2().items_center()
                    .child(Button::new("au-save").small().primary().label("保存").on_click(cx.listener(|this, _, window, cx| this.save(window, cx))))
                    .child({
                        let id = d.id.clone();
                        let busy = self.running.contains(&d.id);
                        let saved = self.items.iter().any(|a| a.id == d.id);
                        Button::new("au-run").small().ghost().icon(IconName::Play).label(if busy { "运行中…" } else { "立即运行" }).disabled(busy || !saved)
                            .on_click(cx.listener(move |this, _, _, cx| this.run_now(&id, cx)))
                    })
                    .child(Button::new("au-del").small().ghost().icon(IconName::Trash).label("删除").on_click(cx.listener(|this, _, window, cx| this.delete(window, cx))))
                    .when_some(self.message.clone(), |d, m| d.child(div().text_size(px(12.)).text_color(t.muted_foreground).child(m))),
            )
            .into_any_element()
    }

    fn runs_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        if self.runs.is_empty() {
            return caption("还没有运行记录", cx).into_any_element();
        }
        v_flex().w_full().gap_2().children(self.runs.iter().map(|r| {
            let (label, color) = match r.status {
                RunStatus::Running => ("运行中", t.primary),
                RunStatus::Succeeded => ("成功", t.success),
                RunStatus::Failed => ("失败", t.danger),
            };
            let secs = r.finished_at.map(|f| (f - r.started_at).max(0));
            v_flex().w_full().gap_1().p_3().rounded(px(8.)).border_1().border_color(t.border)
                .child(
                    h_flex().gap_2().items_center().text_size(px(12.))
                        .child(pill(label, color, color.opacity(0.12)))
                        .child(div().font_family(MONO).text_color(t.muted_foreground).child(format_ts(r.started_at)))
                        .when_some(secs, |d, s| d.child(div().text_color(t.muted_foreground).child(format!("{s} 秒"))))
                        .when(r.cost_usd > 0.0, |d| d.child(div().text_color(t.muted_foreground).child(format!("${:.3}", r.cost_usd)))),
                )
                .when(!r.summary.is_empty(), |d| d.child(div().text_size(px(12.5)).child(r.summary.clone())))
        }))
        .into_any_element()
    }
}

/// 带小标题的表单项。
fn field(label: &str, body: AnyElement, cx: &App) -> Div {
    v_flex().w_full().gap_1().child(caption(label.to_string(), cx)).child(body)
}

impl Render for AutomationPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let body: AnyElement = match self.draft.clone() {
            None => empty_state(IconName::Timer, "还没有自动化", "点左上角「+」创建一条按计划运行的任务", cx).into_any_element(),
            Some(d) => div().id("auto-scroll").size_full().overflow_y_scroll().child(
                v_flex().w_full().max_w(px(720.)).mx_auto().px_6().py_5().gap_5()
                    .child(h_flex().gap_2().items_center().child(icon(IconName::Timer, 16., t.primary)).child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child(if d.name.is_empty() { "新自动化".to_string() } else { d.name.clone() })))
                    .child(self.form(&d, cx))
                    .child(v_flex().w_full().gap_2().child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child("运行记录")).child(self.runs_view(cx))),
            ).into_any_element(),
        };
        h_flex().size_full().bg(t.background).child(self.sidebar(cx)).child(div().flex_1().min_w_0().h_full().child(body))
    }
}
