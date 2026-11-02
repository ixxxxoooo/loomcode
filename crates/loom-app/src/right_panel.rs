//! 右侧面板:改动 / 文件 / 终端 / 计划 / 输出,以可开关的标签呈现(标签条在 `right_tabs.rs`),同一时刻只显示一个。
//!
//! 面板是独立实体,持有当前会话模型的引用,所有内容都从会话状态推导,
//! 因此演示后端与真实 Claude 后端共用同一套界面。

use crate::diff;
use crate::file_tree::FileTree;
use crate::model::SessionModel;
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

/// 右侧面板向工作台发出的事件。
pub enum PanelEvent {
    /// 用户在文件树里点了一个文件
    OpenFile(std::path::PathBuf),
    /// 用户在文件树里双击了一个文件(IDE 里固定为正式标签)
    OpenFilePinned(std::path::PathBuf),
    /// 「文件」标签的占位上点了「切到 IDE 窗口」
    OpenIdeWindow,
    /// 面板的标签 / 预览文件 / 树收起状态变了(工作台据此记住,下次启动恢复)
    StateChanged,
}

impl EventEmitter<PanelEvent> for RightPanel {}

pub struct RightPanel {
    pub tab: RightTab,
    /// 已打开的标签(顺序即标签条顺序);为空时显示入口列表
    pub open: Vec<RightTab>,
    /// 「文件」标签左侧嵌入的编辑器(全工作台唯一的 `IdeView`,与 IDE 共用);IDE 里的文件面板没有它
    pub editor: Option<Entity<crate::ide::IdeView>>,
    /// 独立 IDE 窗口开着:编辑器显示在那边,这里只放占位
    pub editor_elsewhere: bool,
    /// 右侧文件树是否收起
    pub tree_collapsed: bool,
    /// 右栏被放大到整窗(标签条左侧要给红绿灯留位置),由工作台渲染时同步
    pub zoomed: bool,
    /// 资源管理器里「项目文件夹」区与「大纲」区是否展开(对标 VS Code 的可折叠区块)
    pub folder_open: bool,
    pub outline_open: bool,
    /// 大纲视图(有编辑器可读时才有)
    pub outline: Option<Entity<crate::outline::OutlineView>>,
    /// 文件树里复制 / 剪切的条目(路径, 是否剪切),「粘贴」用
    pub file_clip: Option<(Vec<std::path::PathBuf>, bool)>,
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
    /// 当前项目根目录(终端的工作目录)
    pub(crate) root: String,
    /// 终端集合变化时重绘面板
    _term_hub_sub: Subscription,
    /// 「改动」分段当前显示 Claude 改动还是 Git 更改
    pub(crate) changes_mode: crate::git_changes::ChangesMode,
    /// 源代码管理视图(全工作台唯一,Agent 的「Git 更改」与 IDE 的「源代码管理」共用)
    pub(crate) scm: Entity<crate::scm::ScmView>,
    /// 仓库状态变化(暂存 / 提交…)时刷新文件树的 Git 标记
    _scm_subs: Vec<Subscription>,
    /// 标签条的滚动句柄(标签放不下时横向滚动)与上次已滚到可见处的 (当前标签, 标签数)
    pub(crate) tabs_scroll: ScrollHandle,
    pub(crate) tabs_revealed: std::cell::Cell<Option<(RightTab, usize)>>,
}

impl RightPanel {
    pub fn new(root: String, _demo: bool, editor: Option<Entity<crate::ide::IdeView>>, scm: Entity<crate::scm::ScmView>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let hub = crate::term_store::ProjectTerminals::hub(cx);
        let sub = cx.observe(&hub, |_, _, cx| cx.notify());
        let scm_sub = cx.subscribe(&scm, |this, _, ev: &crate::scm::ScmEvent, cx| {
            if matches!(ev, crate::scm::ScmEvent::Changed) {
                this.reload_files(cx);
            }
        });
        // 角标(改动数)随源代码管理状态变化
        let scm_obs = cx.observe(&scm, |_, _, cx| cx.notify());
        let outline = editor.clone().map(|e| cx.new(|cx| crate::outline::OutlineView::new(e, cx)));
        Self {
            tab: RightTab::Changes,
            open: crate::right_tabs::DEFAULT_TABS.to_vec(),
            editor,
            editor_elsewhere: false,
            tree_collapsed: false,
            zoomed: false,
            folder_open: true,
            outline_open: true,
            outline,
            file_clip: None,
            run_tabs: Vec::new(),
            active_run: None,
            show_tabs: true,
            trailing: None,
            model: None,
            _sub: None,
            open_changes: HashSet::new(),
            files: FileTree::new(root.clone()),
            subagent: None,
            task_sel: None,
            root,
            _term_hub_sub: sub,
            changes_mode: crate::git_changes::ChangesMode::initial(),
            scm,
            _scm_subs: vec![scm_sub, scm_obs],
            tabs_scroll: ScrollHandle::new(),
            tabs_revealed: std::cell::Cell::new(None),
        }
    }

    /// 接上编辑器,资源管理器下方才有大纲区。
    pub fn attach_outline(&mut self, editor: Entity<crate::ide::IdeView>, cx: &mut Context<Self>) {
        self.outline = Some(cx.new(|cx| crate::outline::OutlineView::new(editor, cx)));
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
        self.run_tabs.clear();
        self.active_run = None;
        self.root = root.clone();
        self.scm.update(cx, |s, cx| s.set_root(&root, cx));
        self.files.set_root(root.clone());
        self.reload_files(cx);
        cx.notify();
    }

    pub fn set_tab(&mut self, tab: RightTab, cx: &mut Context<Self>) {
        self.open_tab(tab, cx);
    }

    /// 切到「文件」标签(文件本身由工作台在共享编辑器里打开)。
    pub fn show_files_tab(&mut self, cx: &mut Context<Self>) {
        self.open_tab(RightTab::Files, cx);
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
                            .when(open, |d| d.child(div().border_t_1().border_color(t.border).pb_1().child(diff::render_flat(&lines, 400, crate::file_tree::language_for(std::path::Path::new(&c.path)), cx))))
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
        let items = self.items(cx);
        let change_count = collect_changes(&items).len();
        let body = if self.open.is_empty() { self.empty_tabs_view(cx) } else { match self.tab {
            RightTab::Changes => self.changes_tab(&items, cx),
            RightTab::Files if self.show_tabs => self.files_workspace(cx),
            RightTab::Files => self.files_view(cx),
            RightTab::Terminal => self.terminal_tab(&items, cx),
            RightTab::Plan => self.plan_view(&items, cx),
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
