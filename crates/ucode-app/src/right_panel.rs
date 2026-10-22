//! 右侧固定面板:改动 / 文件 / 终端 / 计划 / 输出,分段切换,同一时刻只显示一个。
//!
//! 面板是独立实体,持有当前会话模型的引用,所有内容都从会话状态推导,
//! 因此演示后端与真实 Claude 后端共用同一套界面。

use crate::ansi;
use crate::diff;
use crate::file_tree::FileTree;
use crate::model::SessionModel;
use crate::tool_card::tool_icon;
use crate::ui::{MONO, caption, empty_state, icon, pill};
use crate::workbench::RightTab;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::text::TextView;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::collections::HashSet;
use ucode_core::*;

/// 会话里累计的单个文件改动(同一文件多次编辑会合并:取最早的旧内容与最新的新内容)。
#[derive(Debug, Clone, PartialEq)]
pub struct FileChange {
    pub path: String,
    pub old: String,
    pub new: String,
}

/// 遍历时间线(含子 Agent 嵌套)。
pub fn walk_items<'a>(items: &'a [TimelineItem], f: &mut impl FnMut(&'a TimelineItem)) {
    for item in items {
        f(item);
        if let TimelineItem::Subagent(s) = item {
            walk_items(&s.children, f);
        }
    }
}

/// 汇总整个会话产生的文件改动,保持首次出现的顺序。
pub fn collect_changes(items: &[TimelineItem]) -> Vec<FileChange> {
    let mut out: Vec<FileChange> = Vec::new();
    walk_items(items, &mut |item| {
        let tool = match item {
            TimelineItem::Tool(t) => t,
            TimelineItem::Permission(p) => &p.tool,
            _ => return,
        };
        for c in &tool.content {
            if let ToolContent::Diff { path, old, new } = c {
                match out.iter_mut().find(|f| &f.path == path) {
                    Some(f) => f.new = new.clone(),
                    None => out.push(FileChange { path: path.clone(), old: old.clone(), new: new.clone() }),
                }
            }
        }
    });
    out
}

/// 一条终端记录。
#[derive(Debug, Clone, PartialEq)]
pub struct TerminalRun {
    pub command: String,
    pub output: String,
    pub exit_code: Option<i32>,
    pub running: bool,
}

/// 收集会话里所有命令执行记录。
pub fn collect_terminal(items: &[TimelineItem]) -> Vec<TerminalRun> {
    let mut out = Vec::new();
    walk_items(items, &mut |item| {
        if let TimelineItem::Tool(t) = item {
            for c in &t.content {
                if let ToolContent::Terminal { command, output, exit_code } = c {
                    out.push(TerminalRun {
                        command: command.clone(),
                        output: output.clone(),
                        exit_code: *exit_code,
                        running: t.status == ToolStatus::Running,
                    });
                }
            }
        }
    });
    out
}

/// 输出面板的一行日志。
#[derive(Debug, Clone, PartialEq)]
pub struct LogLine {
    pub level: NoticeLevel,
    pub kind: Option<ToolKind>,
    pub text: String,
    pub detail: Option<String>,
}

/// 把时间线整理成日志:工具调用 + 提示。
pub fn collect_log(items: &[TimelineItem]) -> Vec<LogLine> {
    let mut out = Vec::new();
    walk_items(items, &mut |item| match item {
        TimelineItem::Tool(t) => {
            let level = if t.status == ToolStatus::Failed { NoticeLevel::Error } else { NoticeLevel::Info };
            let detail = match (t.status, t.duration_ms) {
                (ToolStatus::Running, _) => Some("运行中".to_string()),
                (ToolStatus::Pending, _) => Some("等待".to_string()),
                (_, Some(ms)) => Some(format!("{ms} ms")),
                _ => None,
            };
            out.push(LogLine { level, kind: Some(t.kind), text: t.title.clone(), detail });
        }
        TimelineItem::Notice { level, text, .. } => {
            out.push(LogLine { level: *level, kind: None, text: text.clone(), detail: None })
        }
        TimelineItem::Subagent(s) => out.push(LogLine {
            level: NoticeLevel::Info,
            kind: Some(ToolKind::Subagent),
            text: format!("子 Agent · {}", s.description),
            detail: None,
        }),
        _ => {}
    });
    out
}

/// 右侧面板向工作台发出的事件。
pub enum PanelEvent {
    /// 用户在文件树里点了一个文件
    OpenFile(std::path::PathBuf),
}

impl EventEmitter<PanelEvent> for RightPanel {}

pub struct RightPanel {
    pub tab: RightTab,
    model: Option<Entity<SessionModel>>,
    _sub: Option<Subscription>,
    /// 「改动」里已展开 Diff 的文件
    open_changes: HashSet<String>,
    pub files: FileTree,
    /// 等待在渲染时打开的文件(没有 window 的调用方使用)
    pending_open: Option<String>,
    /// 当前项目根目录(终端命令的工作目录)
    root: String,
    /// 终端命令输入框(演示模式不显示)
    term_input: Entity<InputState>,
    /// 用户在终端里手动运行的命令(最新在后)
    user_runs: Vec<UserRun>,
    /// 演示模式:不允许运行真实命令
    demo: bool,
    _term_sub: Subscription,
}

/// 用户手动运行的一条命令。
struct UserRun {
    run: TerminalRun,
    pid: Option<u32>,
}

/// 命令运行线程发回的消息。
enum TermMsg {
    Started(u32),
    Output(String),
    Exited(Option<i32>),
}

impl RightPanel {
    pub fn new(root: String, demo: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let term_input = cx.new(|cx| InputState::new(window, cx).placeholder("在项目目录运行命令,回车执行"));
        let sub = cx.subscribe_in(&term_input, window, |this: &mut Self, input, ev: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { .. } = ev {
                let cmd = input.read(cx).value().trim().to_string();
                if !cmd.is_empty() {
                    input.update(cx, |i, cx| i.set_value(String::new(), window, cx));
                    this.run_command(cmd, cx);
                }
            }
        });
        Self {
            tab: RightTab::Changes,
            model: None,
            _sub: None,
            open_changes: HashSet::new(),
            files: FileTree::new(root.clone()),
            pending_open: None,
            root,
            term_input,
            user_runs: Vec::new(),
            demo,
            _term_sub: sub,
        }
    }

    /// 在项目目录里运行一条 shell 命令,输出实时流入终端记录。
    pub fn run_command(&mut self, command: String, cx: &mut Context<Self>) {
        if self.demo {
            return;
        }
        let ix = self.user_runs.len();
        self.user_runs.push(UserRun { run: TerminalRun { command: command.clone(), output: String::new(), exit_code: None, running: true }, pid: None });
        let (tx, rx) = async_channel::unbounded::<TermMsg>();
        let root = self.root.clone();
        let spawned = std::thread::Builder::new().name("ucode-terminal".into()).spawn(move || {
            use std::io::{BufRead, BufReader};
            use std::process::{Command, Stdio};
            // 标准错误并入标准输出,保持与终端一致的交错顺序
            let child = Command::new("/bin/sh").arg("-c").arg(format!("{{ {command}\n}} 2>&1")).current_dir(&root).stdin(Stdio::null()).stdout(Stdio::piped()).spawn();
            match child {
                Ok(mut child) => {
                    let _ = tx.send_blocking(TermMsg::Started(child.id()));
                    if let Some(out) = child.stdout.take() {
                        let mut reader = BufReader::new(out);
                        let mut buf = Vec::new();
                        while reader.read_until(b'\n', &mut buf).is_ok_and(|n| n > 0) {
                            let _ = tx.send_blocking(TermMsg::Output(String::from_utf8_lossy(&buf).into_owned()));
                            buf.clear();
                        }
                    }
                    let code = child.wait().ok().and_then(|s| s.code());
                    let _ = tx.send_blocking(TermMsg::Exited(code));
                }
                Err(e) => {
                    let _ = tx.send_blocking(TermMsg::Output(format!("无法启动命令:{e}\n")));
                    let _ = tx.send_blocking(TermMsg::Exited(Some(127)));
                }
            }
        });
        if spawned.is_err() {
            self.user_runs[ix].run.running = false;
            return;
        }
        cx.spawn(async move |this, cx| {
            while let Ok(msg) = rx.recv().await {
                let alive = this.update(cx, |this, cx| {
                    if let Some(u) = this.user_runs.get_mut(ix) {
                        match msg {
                            TermMsg::Started(pid) => u.pid = Some(pid),
                            TermMsg::Output(text) => u.run.output.push_str(&text),
                            TermMsg::Exited(code) => {
                                u.run.running = false;
                                // 被信号终止时没有退出码,按 130 展示
                                u.run.exit_code = Some(code.unwrap_or(130));
                            }
                        }
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    /// 终止一条仍在运行的命令。
    fn stop_command(&mut self, ix: usize) {
        if let Some(pid) = self.user_runs.get(ix).and_then(|u| u.pid) {
            let _ = std::process::Command::new("kill").arg(pid.to_string()).status();
        }
    }

    /// 切换到另一个会话。
    pub fn set_session(&mut self, model: Option<Entity<SessionModel>>, root: String, cx: &mut Context<Self>) {
        self._sub = model.as_ref().map(|m| cx.observe(m, |_, _, cx| cx.notify()));
        self.model = model;
        self.open_changes.clear();
        self.root = root.clone();
        self.files.set_root(root);
        cx.notify();
    }

    pub fn set_tab(&mut self, tab: RightTab, cx: &mut Context<Self>) {
        self.tab = tab;
        cx.notify();
    }

    /// 请求在「文件」里打开某个路径(相对或绝对)。
    pub fn request_open(&mut self, path: String, cx: &mut Context<Self>) {
        self.tab = RightTab::Files;
        self.pending_open = Some(path);
        cx.notify();
    }

    fn items(&self, cx: &App) -> Vec<TimelineItem> {
        self.model.as_ref().map(|m| m.read(cx).state.items.clone()).unwrap_or_default()
    }

    // ---------- 顶部分段 ----------

    fn tab_bar(&self, changes: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let tab = self.tab;
        h_flex().h(px(36.)).px_2().gap_0p5().items_center().border_b_1().border_color(t.border)
            .children(RightTab::ALL.into_iter().map(|rt| {
                let on = rt == tab;
                h_flex().id(SharedString::from(format!("rt-{}", rt.label()))).h(px(26.)).px_2().gap_1().items_center().rounded(px(6.)).cursor_pointer()
                    .text_size(px(12.))
                    .text_color(if on { t.foreground } else { t.muted_foreground })
                    .when(on, |d| d.bg(t.muted))
                    .hover(|d| d.bg(t.list_hover))
                    .on_click(cx.listener(move |this, _, _, cx| this.set_tab(rt, cx)))
                    .child(icon(rt.icon(), 12., if on { t.foreground } else { t.muted_foreground }))
                    .child(rt.label())
                    .when(rt == RightTab::Changes && changes > 0, |d| d.child(pill(changes.to_string(), t.primary_foreground, t.primary)))
            }))
    }

    // ---------- 改动 ----------

    fn changes_view(&self, items: &[TimelineItem], cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let changes = collect_changes(items);
        if changes.is_empty() {
            return empty_state(IconName::FileDiff, "还没有改动", "Claude 修改文件后会汇总在这里", cx).into_any_element();
        }
        let single = changes.len() == 1;
        let stats: Vec<(usize, Vec<diff::DiffLine>, usize)> = changes
            .iter()
            .map(|c| {
                let (lines, a, r) = diff::compute(&c.old, &c.new);
                (a, lines, r)
            })
            .collect();
        let (ta, tr) = stats.iter().fold((0, 0), |(a, r), (x, _, y)| (a + x, r + y));
        v_flex().size_full()
            .child(
                h_flex().h(px(34.)).px_3().gap_2().items_center().text_size(px(12.))
                    .child(div().text_color(t.muted_foreground).child(format!("{} 个文件", changes.len())))
                    .child(div().font_family(MONO).text_color(t.success).child(format!("+{ta}")))
                    .child(div().font_family(MONO).text_color(t.danger).child(format!("−{tr}"))),
            )
            .child(
                v_flex().id("changes-scroll").flex_1().min_h_0().overflow_y_scroll().px_2().pb_3().gap_1p5()
                    .children(changes.iter().zip(stats).enumerate().map(|(ix, (c, (a, lines, r)))| {
                        let open = single || self.open_changes.contains(&c.path);
                        let path = c.path.clone();
                        let (dir, name) = split_path(&c.path);
                        v_flex().w_full().rounded(px(8.)).border_1().border_color(t.border)
                            .child(
                                h_flex().id(SharedString::from(format!("chg-{ix}"))).h(px(30.)).px_2().gap_1p5().items_center().cursor_pointer()
                                    .hover(|d| d.bg(t.list_hover))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if !this.open_changes.remove(&path) {
                                            this.open_changes.insert(path.clone());
                                        }
                                        cx.notify();
                                    }))
                                    .child(icon(if open { IconName::ChevronDown } else { IconName::ChevronRight }, 12., t.muted_foreground))
                                    .child(div().text_size(px(12.5)).font_weight(FontWeight::MEDIUM).child(name))
                                    .child(div().flex_1().min_w_0().truncate().text_size(px(11.)).text_color(t.muted_foreground).child(dir))
                                    .child(div().text_size(px(11.)).font_family(MONO).text_color(t.success).child(format!("+{a}")))
                                    .child(div().text_size(px(11.)).font_family(MONO).text_color(t.danger).child(format!("−{r}"))),
                            )
                            .when(open, |d| d.child(div().p_1().child(diff::render(&lines, 400, cx))))
                    })),
            )
            .into_any_element()
    }

    // ---------- 终端 ----------

    fn terminal_view(&self, items: &[TimelineItem], cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let mut runs: Vec<(TerminalRun, Option<usize>)> = collect_terminal(items).into_iter().map(|r| (r, None)).collect();
        runs.extend(self.user_runs.iter().enumerate().map(|(i, u)| (u.run.clone(), Some(i))));
        let list = if runs.is_empty() {
            empty_state(IconName::Terminal, "还没有命令记录", "Claude 执行的命令与输出会显示在这里", cx).into_any_element()
        } else {
            v_flex().id("terminal-scroll").size_full().overflow_y_scroll().p_2().gap_2()
                .children(runs.into_iter().map(|(run, user_ix)| {
                    let shown: String = {
                        let lines: Vec<&str> = run.output.lines().collect();
                        let skip = lines.len().saturating_sub(200);
                        lines[skip..].join("\n")
                    };
                    let parsed = ansi::parse(&shown);
                    v_flex().w(relative(1.)).rounded(px(8.)).border_1().border_color(t.border).bg(t.muted).p_2().gap_1().font_family(MONO).text_size(px(12.))
                        .child(
                            h_flex().gap_2().items_center()
                                .child(div().text_color(t.primary).child("$"))
                                .child(div().flex_1().min_w_0().child(run.command.lines().next().unwrap_or("").to_string()))
                                .when(run.running, |d| d.child(pill("运行中", t.info, t.background)))
                                .when(run.running && user_ix.is_some(), |d| {
                                    let ix = user_ix.unwrap_or_default();
                                    d.child(Button::new(SharedString::from(format!("term-stop-{ix}"))).xsmall().ghost().icon(IconName::Square).tooltip("停止")
                                        .on_click(cx.listener(move |this, _, _, _| this.stop_command(ix))))
                                })
                                .when_some(run.exit_code, |d, code| d.child(pill(
                                    if code == 0 { "成功".to_string() } else { format!("退出码 {code}") },
                                    if code == 0 { t.success } else { t.danger },
                                    t.background,
                                ))),
                        )
                        .when(!parsed.text.is_empty(), |d| d.child(StyledText::new(parsed.text).with_highlights(parsed.runs)))
                }))
                .into_any_element()
        };
        v_flex().size_full()
            .child(div().flex_1().min_h_0().child(list))
            .when(!self.demo, |d| d.child(
                h_flex().px_2().py_1p5().gap_1p5().items_center().border_t_1().border_color(t.border)
                    .child(div().font_family(MONO).text_color(t.primary).child("$"))
                    .child(div().flex_1().child(Input::new(&self.term_input))),
            ))
            .into_any_element()
    }

    // ---------- 计划 ----------

    fn plan_view(&self, items: &[TimelineItem], cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let mut todos: Option<&Vec<TodoItem>> = None;
        let mut plan: Option<&PlanApproval> = None;
        walk_items(items, &mut |item| match item {
            TimelineItem::Todos { items, .. } => todos = Some(items),
            TimelineItem::PlanApproval(p) => plan = Some(p),
            _ => {}
        });
        if todos.is_none() && plan.is_none() {
            return empty_state(IconName::ListTodo, "还没有计划", "待办与计划文档会显示在这里", cx).into_any_element();
        }
        v_flex().id("plan-scroll").size_full().overflow_y_scroll().p_3().gap_3()
            .when_some(todos, |d, todos| {
                let done = todos.iter().filter(|i| i.status == TodoStatus::Completed).count();
                d.child(
                    v_flex().gap_1p5()
                        .child(h_flex().gap_2().items_center()
                            .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child("待办"))
                            .child(caption(format!("{done}/{}", todos.len()), cx)))
                        .children(todos.iter().map(|todo| {
                            let (ic, color) = match todo.status {
                                TodoStatus::Completed => (IconName::CircleCheck, t.success),
                                TodoStatus::InProgress => (IconName::CircleDot, t.primary),
                                TodoStatus::Pending => (IconName::Circle, t.muted_foreground),
                            };
                            h_flex().gap_2().items_start().text_size(px(13.))
                                .child(div().mt(px(2.)).child(icon(ic, 14., color)))
                                .child(div().flex_1().min_w_0()
                                    .text_color(if todo.status == TodoStatus::Completed { t.muted_foreground } else { t.foreground })
                                    .child(todo.content.clone()))
                        })),
                )
            })
            .when_some(plan, |d, plan| {
                d.child(
                    v_flex().gap_1p5()
                        .child(h_flex().gap_2().items_center()
                            .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child("计划文档"))
                            .when_some(plan.approved, |d, ok| d.child(pill(if ok { "已批准" } else { "已拒绝" }, if ok { t.success } else { t.danger }, t.muted))))
                        .child(TextView::markdown(SharedString::from(format!("plan-doc-{}", plan.id)), plan.markdown.clone()).selectable(true)),
                )
            })
            .into_any_element()
    }

    // ---------- 输出 ----------

    fn output_view(&self, items: &[TimelineItem], cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let log = collect_log(items);
        let usage = self.model.as_ref().map(|m| m.read(cx).state.usage.clone()).unwrap_or_default();
        if log.is_empty() && usage.context_size == 0 {
            return empty_state(IconName::ScrollText, "暂无输出", "工具调用与会话日志会显示在这里", cx).into_any_element();
        }
        v_flex().size_full()
            .when(usage.context_size > 0, |d| {
                d.child(
                    v_flex().px_3().py_2().gap_1().border_b_1().border_color(t.border).text_size(px(12.))
                        .child(h_flex().justify_between()
                            .child(div().text_color(t.muted_foreground).child("上下文"))
                            .child(div().font_family(MONO).child(format!("{} / {}  ({:.0}%)", usage.context_used, usage.context_size, usage.context_ratio() * 100.))))
                        .child(h_flex().justify_between()
                            .child(div().text_color(t.muted_foreground).child("本会话花费"))
                            .child(div().font_family(MONO).child(format!("${:.4}", usage.cost_usd))))
                        .when_some(usage.five_hour_pct, |d, p| d.child(h_flex().justify_between()
                            .child(div().text_color(t.muted_foreground).child("5 小时限额"))
                            .child(div().font_family(MONO).child(format!("{p:.0}%")))))
                        .when_some(usage.weekly_pct, |d, p| d.child(h_flex().justify_between()
                            .child(div().text_color(t.muted_foreground).child("每周限额"))
                            .child(div().font_family(MONO).child(format!("{p:.0}%"))))),
                )
            })
            .child(
                v_flex().id("output-scroll").flex_1().min_h_0().overflow_y_scroll().py_1()
                    .children(log.into_iter().enumerate().map(|(ix, line)| {
                        let color = match line.level {
                            NoticeLevel::Info => t.foreground,
                            NoticeLevel::Warning => t.warning,
                            NoticeLevel::Error => t.danger,
                        };
                        h_flex().w_full().px_3().h(px(24.)).gap_2().items_center().text_size(px(12.))
                            .child(div().w(px(26.)).flex_none().font_family(MONO).text_color(t.muted_foreground).child(format!("{:>3}", ix + 1)))
                            .child(icon(line.kind.map(tool_icon).unwrap_or(IconName::Info), 12., t.muted_foreground))
                            .child(div().flex_1().min_w_0().truncate().text_color(color).child(line.text))
                            .when_some(line.detail, |d, s| d.child(div().flex_none().font_family(MONO).text_color(t.muted_foreground).child(s)))
                    })),
            )
            .into_any_element()
    }
}

/// 把路径拆成 (目录, 文件名)。
fn split_path(path: &str) -> (String, String) {
    match path.rsplit_once('/') {
        Some((dir, name)) => (dir.to_string(), name.to_string()),
        None => (String::new(), path.to_string()),
    }
}

impl Render for RightPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        if let Some(path) = self.pending_open.take() {
            self.files.open(&path, window, cx);
        }
        let items = self.items(cx);
        let change_count = collect_changes(&items).len();
        let body = match self.tab {
            RightTab::Changes => self.changes_view(&items, cx),
            RightTab::Files => self.files.render(cx),
            RightTab::Terminal => self.terminal_view(&items, cx),
            RightTab::Plan => self.plan_view(&items, cx),
            RightTab::Output => self.output_view(&items, cx),
        };
        v_flex().size_full().bg(t.background)
            .child(self.tab_bar(change_count, cx))
            .child(div().flex_1().min_h_0().child(body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(id: &str, path: &str, old: &str, new: &str) -> TimelineItem {
        TimelineItem::Tool(ToolCall::new(id, "Edit", "编辑").with_content(ToolContent::Diff {
            path: path.into(),
            old: old.into(),
            new: new.into(),
        }))
    }

    #[::core::prelude::v1::test]
    fn 同一文件多次编辑合并() {
        let items = vec![edit("1", "a.rs", "x\n", "y\n"), edit("2", "b.rs", "", "n\n"), edit("3", "a.rs", "y\n", "z\n")];
        let c = collect_changes(&items);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].old, "x\n");
        assert_eq!(c[0].new, "z\n");
    }

    #[::core::prelude::v1::test]
    fn 路径拆分() {
        assert_eq!(split_path("src/a/b.rs"), ("src/a".into(), "b.rs".into()));
        assert_eq!(split_path("b.rs"), (String::new(), "b.rs".into()));
    }
}
