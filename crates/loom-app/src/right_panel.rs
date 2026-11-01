//! 右侧面板:改动 / 文件 / 终端 / 计划 / 输出,以可开关的标签呈现(标签条在 `right_tabs.rs`),同一时刻只显示一个。
//!
//! 面板是独立实体,持有当前会话模型的引用,所有内容都从会话状态推导,
//! 因此演示后端与真实 Claude 后端共用同一套界面。

use crate::diff;
use crate::file_tree::FileTree;
use crate::model::SessionModel;
use crate::tool_card::tool_icon;
use crate::ui::{MONO, caption, empty_state, icon, pill};
use crate::workbench::RightTab;
use gpui_kit::assets::IconName;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::collections::HashSet;
use loom_core::*;

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
        // 被拒绝、被中断的编辑没有真正落盘,不算改动
        if matches!(tool.status, ToolStatus::Denied | ToolStatus::Interrupted) {
            return;
        }
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
    /// 产生它的工具调用 id(「在终端面板查看」用它定位)
    pub id: String,
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
                        id: t.id.clone(),
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
    /// 用户在文件树里双击了一个文件(IDE 里固定为正式标签)
    OpenFilePinned(std::path::PathBuf),
    /// 面板的标签 / 预览文件 / 树收起状态变了(工作台据此记住,下次启动恢复)
    StateChanged,
}

impl EventEmitter<PanelEvent> for RightPanel {}

pub struct RightPanel {
    pub tab: RightTab,
    /// 已打开的标签(顺序即标签条顺序);为空时显示入口列表
    pub open: Vec<RightTab>,
    /// 「文件」标签左侧正在预览的文件、← → 导航栈(文件路径 + 当前位置)、右侧树是否收起
    pub current: Option<crate::file_preview::FilePreview>,
    pub nav: Vec<std::path::PathBuf>,
    pub nav_ix: usize,
    pub tree_collapsed: bool,
    /// 启动时要恢复的预览文件:第一次切换会话时(项目目录确定后)才打开
    pub restore_file: Option<String>,
    /// 「终端」标签里为单条 Claude 命令打开的只读标签(工具调用 id),以及当前选中的一个
    pub run_tabs: Vec<String>,
    pub active_run: Option<String>,
    /// 是否显示顶部分段栏(IDE 窗口用活动栏切换,不显示)
    pub show_tabs: bool,
    /// 标签行末尾的额外控件(Agent 窗口放右面板开关与 IDE 入口),由工作台注入
    pub trailing: Option<std::rc::Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>>,
    pub(crate) model: Option<Entity<SessionModel>>,
    /// 「子 Agent」标签正在看的子 Agent(条目 id)
    pub(crate) subagent: Option<String>,
    /// 「任务」标签正在查看输出的任务(task_id)
    pub(crate) task_sel: Option<String>,
    _sub: Option<Subscription>,
    /// 「改动」里已展开 Diff 的文件
    open_changes: HashSet<String>,
    pub files: FileTree,
    /// 等待在渲染时打开的文件(没有 window 的调用方使用)
    pending_open: Option<String>,
    /// 当前项目根目录(终端的工作目录)
    pub(crate) root: String,
    /// 终端集合变化时重绘面板
    _term_hub_sub: Subscription,
    /// 「改动」分段里的 Git 更改
    pub(crate) git: crate::git_changes::GitPane,
}

impl RightPanel {
    pub fn new(root: String, _demo: bool, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let hub = crate::term_store::ProjectTerminals::hub(cx);
        let sub = cx.observe(&hub, |_, _, cx| cx.notify());
        Self {
            tab: RightTab::Changes,
            open: crate::right_tabs::DEFAULT_TABS.to_vec(),
            current: None,
            nav: Vec::new(),
            nav_ix: 0,
            tree_collapsed: false,
            restore_file: None,
            run_tabs: Vec::new(),
            active_run: None,
            show_tabs: true,
            trailing: None,
            model: None,
            _sub: None,
            open_changes: HashSet::new(),
            files: FileTree::new(root.clone()),
            pending_open: None,
            subagent: None,
            task_sel: None,
            root,
            _term_hub_sub: sub,
            git: crate::git_changes::GitPane::new(),
        }
    }

    /// 当前会话里 Claude 改动过的文件数(快捷竖条的圆点用)。
    pub fn change_count(&self, cx: &App) -> usize {
        collect_changes(&self.items(cx)).len()
    }

    /// 切换到另一个会话。
    pub fn set_session(&mut self, model: Option<Entity<SessionModel>>, root: String, cx: &mut Context<Self>) {
        self._sub = model.as_ref().map(|m| cx.observe(m, |_, _, cx| cx.notify()));
        self.model = model;
        self.open_changes.clear();
        self.clear_files_workspace();
        self.run_tabs.clear();
        self.active_run = None;
        self.git.reset();
        self.root = root.clone();
        self.files.set_root(root.clone());
        // 启动恢复:上次预览的文件还在这个项目里就重新打开
        if let Some(f) = self.restore_file.take().filter(|f| std::path::Path::new(f).starts_with(&root) && std::path::Path::new(f).is_file()) {
            self.pending_open = Some(f);
        }
        self.reload_files(cx);
        cx.notify();
    }

    pub fn set_tab(&mut self, tab: RightTab, cx: &mut Context<Self>) {
        self.open_tab(tab, cx);
    }

    /// 请求在「文件」里打开某个路径(相对或绝对)。
    pub fn request_open(&mut self, path: String, cx: &mut Context<Self>) {
        self.open_tab(RightTab::Files, cx);
        self.pending_open = Some(path);
        cx.notify();
    }

    /// 打开「改动」标签并只展开某个文件的 Diff(时间线卡片点进来)。
    ///
    /// 参数:`path` 文件路径(相对或绝对,按后缀匹配改动列表)。
    pub fn focus_change_file(&mut self, path: String, cx: &mut Context<Self>) {
        let changes = collect_changes(&self.items(cx));
        let hit = changes.iter().find(|c| c.path == path).or_else(|| changes.iter().find(|c| c.path.ends_with(&path) || path.ends_with(&c.path)));
        if let Some(c) = hit {
            self.open_changes.clear();
            self.open_changes.insert(c.path.clone());
        }
        self.open_tab(RightTab::Changes, cx);
    }

    pub(crate) fn items(&self, cx: &App) -> Vec<TimelineItem> {
        self.model.as_ref().map(|m| m.read(cx).state.items.clone()).unwrap_or_default()
    }

    // ---------- 改动 ----------

    pub(crate) fn changes_view(&self, items: &[TimelineItem], cx: &mut Context<Self>) -> AnyElement {
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
pub(crate) fn split_path(path: &str) -> (String, String) {
    match path.rsplit_once('/') {
        Some((dir, name)) => (dir.to_string(), name.to_string()),
        None => (String::new(), path.to_string()),
    }
}

impl Render for RightPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        if let Some(path) = self.pending_open.take() {
            let full = self.files.open(&path);
            self.open_in_place(full, window, cx);
        }
        let items = self.items(cx);
        if self.tab == RightTab::Changes && self.git.mode == crate::git_changes::ChangesMode::Git {
            self.git_auto_refresh(items.len(), cx);
        }
        let change_count = collect_changes(&items).len();
        let body = if self.open.is_empty() { self.empty_tabs_view(cx) } else { match self.tab {
            RightTab::Changes => self.changes_tab(&items, cx),
            RightTab::Files if self.show_tabs => self.files_workspace(cx),
            RightTab::Files => self.files_view(cx),
            RightTab::Terminal => self.terminal_tab(&items, cx),
            RightTab::Plan => self.plan_view(&items, cx),
            RightTab::Output => self.output_view(&items, cx),
            RightTab::Subagent => self.subagent_view(&items, window, cx),
            RightTab::Tasks => self.tasks_view(cx),
        } };
        v_flex().size_full().bg(t.background)
            .when(self.show_tabs, |d| d.child(self.tab_bar(change_count, window, cx)))
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
    fn 被拒绝的编辑不算改动() {
        let mut denied = ToolCall::new("1", "Edit", "编辑").with_content(ToolContent::Diff { path: "a.rs".into(), old: "x".into(), new: "y".into() });
        denied.status = ToolStatus::Denied;
        assert!(collect_changes(&[TimelineItem::Tool(denied)]).is_empty());
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
