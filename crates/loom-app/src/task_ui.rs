//! 后台任务与子 Agent 的公共展示:状态样式、完成通知行、输入框上方的「任务坞」。
//!
//! 任务坞对标 OhMyAgent 的 `ComposerDockView` 与 baocode 的 `ActivityStrip`:
//! 有后台任务在跑时,输入框上方出现一个计数胶囊(「3 个后台任务运行中」),点开是任务列表,
//! 每行可查看输出、停止;完整输出与历史在右侧「任务」标签里。
//!
//! @author ixxxxoooo

use crate::model::{SessionModel, UiRequest};
use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::*;

/// 子 Agent 状态的文案与颜色。
pub fn subagent_style(status: SubagentStatus, cx: &App) -> (&'static str, Hsla) {
    let t = cx.theme();
    match status {
        SubagentStatus::Starting => ("启动中", t.muted_foreground),
        SubagentStatus::Running => ("运行中", t.primary),
        SubagentStatus::Background => ("后台运行", t.primary),
        SubagentStatus::Completed => ("已完成", t.success),
        SubagentStatus::Failed => ("已失败", t.danger),
        SubagentStatus::Stopped => ("已停止", t.muted_foreground),
    }
}

/// 后台任务状态的颜色。
pub fn task_color(status: TaskStatus, cx: &App) -> Hsla {
    let t = cx.theme();
    match status {
        TaskStatus::Running => t.primary,
        TaskStatus::Completed => t.success,
        TaskStatus::Failed => t.danger,
        TaskStatus::Stopped => t.muted_foreground,
    }
}

/// 后台任务类别的图标。
pub fn kind_icon(kind: TaskKind) -> IconName {
    match kind {
        TaskKind::Shell => IconName::Terminal,
        TaskKind::Agent => IconName::Bot,
        TaskKind::Monitor => IconName::Eye,
        TaskKind::Workflow => IconName::Workflow,
        TaskKind::Other => IconName::Zap,
    }
}

/// 后台任务状态的图标(运行中用转圈,另行处理)。
pub fn status_icon(status: TaskStatus) -> IconName {
    match status {
        TaskStatus::Running => IconName::Activity,
        TaskStatus::Completed => IconName::CircleCheck,
        TaskStatus::Failed => IconName::CircleX,
        TaskStatus::Stopped => IconName::CircleStop,
    }
}

/// 一行紧凑的任务统计:`用时 · 工具数 · token`,没有的项跳过。
pub fn meta_text(elapsed_ms: Option<u64>, tools: u32, tokens: u64) -> String {
    let mut parts = Vec::new();
    if let Some(ms) = elapsed_ms {
        parts.push(format_elapsed(ms));
    }
    if tools > 0 {
        parts.push(format!("{tools} 次工具"));
    }
    if tokens > 0 {
        parts.push(format!("{} token", format_tokens(tokens)));
    }
    parts.join(" · ")
}

/// 时间线里的任务结束通知行:状态图标 + 摘要,点击在右侧「任务」标签里查看。
pub fn render_note(session: &Entity<SessionModel>, task_id: &str, kind: TaskKind, status: TaskStatus, text: &str, cx: &App) -> AnyElement {
    let t = cx.theme();
    let color = match status {
        TaskStatus::Failed => t.danger,
        TaskStatus::Completed => t.muted_foreground,
        _ => t.muted_foreground,
    };
    let (s, id) = (session.clone(), task_id.to_string());
    h_flex().id(SharedString::from(format!("tnote-{task_id}-{}", status.label()))).self_start().max_w_full().px_2().py(px(3.)).gap_1p5().items_center().rounded(px(6.)).cursor_pointer()
        .hover(|d| d.bg(t.list_hover))
        .on_click(move |_, _, cx| { s.update(cx, |_, cx| cx.emit(UiRequest::OpenTasks(Some(id.clone())))); })
        .child(icon(kind_icon(kind), 12., t.muted_foreground))
        .child(div().min_w_0().truncate().text_size(px(12.)).text_color(color).child(text.to_string()))
        .child(icon(status_icon(status), 12., task_color(status, cx)))
        .into_any_element()
}

/// 任务坞里最多直接列出几行,其余在右侧「任务」标签里看。
const DOCK_ROWS: usize = 6;

/// 输入框上方的后台任务坞;没有运行中的任务(且列表没展开)时返回 `None`。
///
/// 参数:`session` 会话模型。返回:与输入框同宽的计数胶囊,展开时带任务列表。
pub fn dock(session: &Entity<SessionModel>, cx: &App) -> Option<AnyElement> {
    let m = session.read(cx);
    let running = m.state.running_task_count();
    let visible: Vec<&BackgroundTask> = m.state.tasks.iter().filter(|t| t.visible()).collect();
    if visible.is_empty() || (running == 0 && !m.tasks_open) {
        return None;
    }
    let t = cx.theme();
    let open = m.tasks_open;
    let now = now_ms();
    let kinds = |k: TaskKind| m.state.tasks.iter().filter(|x| x.counts_as_running() && x.kind == k).count();
    let mut parts = Vec::new();
    for (k, unit) in [(TaskKind::Shell, "命令"), (TaskKind::Agent, "子 Agent"), (TaskKind::Monitor, "监听"), (TaskKind::Workflow, "工作流")] {
        if kinds(k) > 0 {
            parts.push(format!("{} {unit}", kinds(k)));
        }
    }
    let stalled = m.state.tasks.iter().filter(|x| x.counts_as_running() && x.idle_ms(now) >= STALL_MS && x.kind == TaskKind::Agent).count();
    let label = if running > 0 { format!("{running} 个后台任务运行中") } else { "后台任务已全部结束".to_string() };
    let s = session.clone();
    let capsule = h_flex().id("task-dock-capsule").self_start().h(px(28.)).px_2p5().gap_2().items_center().rounded(px(14.)).cursor_pointer()
        .border_1().border_color(t.border).bg(t.background).hover(|d| d.bg(t.list_hover))
        .on_click(move |_, _, cx| { s.update(cx, |m, cx| m.toggle_tasks(cx)); })
        .child(if running > 0 { Spinner::new().xsmall().into_any_element() } else { icon(IconName::Activity, 12., t.muted_foreground).into_any_element() })
        .child(div().text_size(px(12.)).text_color(t.foreground).child(label))
        .when(!parts.is_empty(), |d| d.child(div().text_size(px(11.)).text_color(t.muted_foreground).child(parts.join(" · "))))
        .when(stalled > 0, |d| d.child(h_flex().gap_1().items_center().child(icon(IconName::TriangleAlert, 11., t.warning)).child(div().text_size(px(11.)).text_color(t.warning).child(format!("{stalled} 个疑似停滞")))))
        .child(icon(if open { IconName::ChevronDown } else { IconName::ChevronUp }, 12., t.muted_foreground));
    let mut col = v_flex().w_full().max_w(px(crate::timeline::content_max_w(cx))).gap_1().child(capsule);
    if open {
        // 运行中的排前面,同组内最新的在前
        let mut list = visible.clone();
        list.sort_by_key(|x| (!x.counts_as_running(), std::cmp::Reverse(x.started_ms)));
        let total = list.len();
        let rows: Vec<AnyElement> = list.into_iter().take(DOCK_ROWS).map(|x| task_row(session, x, now, cx)).collect();
        let s2 = session.clone();
        col = col.child(
            v_flex().w_full().rounded(px(10.)).border_1().border_color(t.border).bg(t.background).shadow_sm().py_1()
                .children(rows)
                .child(
                    h_flex().id("task-dock-all").h(px(26.)).px_3().gap_1().items_center().cursor_pointer().text_size(px(11.)).text_color(t.muted_foreground)
                        .hover(|d| d.text_color(t.foreground))
                        .on_click(move |_, _, cx| { s2.update(cx, |_, cx| cx.emit(UiRequest::OpenTasks(None))); })
                        .child(format!("在右侧「任务」标签查看全部 {total} 个"))
                        .child(icon(IconName::ChevronRight, 11., t.muted_foreground)),
                ),
        );
    }
    Some(div().w_full().flex().justify_center().px_4().pt_1().pb_1().child(col).into_any_element())
}

/// 任务坞里的一行:类别图标 + 描述 + 统计 + 「查看」「停止」。
fn task_row(session: &Entity<SessionModel>, task: &BackgroundTask, now: u64, cx: &App) -> AnyElement {
    let t = cx.theme();
    let running = task.counts_as_running();
    let stalled = running && task.idle_ms(now) >= STALL_MS;
    let meta = meta_text(Some(task.elapsed_ms(now)), task.tool_uses, task.tokens);
    let detail = if stalled {
        format!("已 {} 无进展", format_elapsed(task.idle_ms(now)))
    } else if running && !task.summary.is_empty() {
        task.summary.clone()
    } else if running && !task.last_tool.is_empty() {
        format!("正在 {}", task.last_tool)
    } else {
        task.status.label().to_string()
    };
    let (s_view, id_view) = (session.clone(), task.task_id.clone());
    let (s_stop, id_stop) = (session.clone(), task.task_id.clone());
    h_flex().id(SharedString::from(format!("task-row-{}", task.task_id))).w_full().h(px(34.)).px_3().gap_2().items_center().cursor_pointer()
        .hover(|d| d.bg(t.list_hover))
        .on_click(move |_, _, cx| { s_view.update(cx, |_, cx| cx.emit(UiRequest::OpenTasks(Some(id_view.clone())))); })
        .child(if running { Spinner::new().xsmall().into_any_element() } else { icon(status_icon(task.status), 13., task_color(task.status, cx)).into_any_element() })
        .child(icon(kind_icon(task.kind), 12., t.muted_foreground))
        .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).text_color(if running { t.foreground } else { t.muted_foreground }).child(task.description.clone()))
        .child(div().flex_none().max_w(px(180.)).truncate().text_size(px(11.)).text_color(if stalled { t.warning } else { t.muted_foreground }).child(detail))
        .child(div().flex_none().text_size(px(11.)).text_color(t.muted_foreground.opacity(0.8)).child(meta))
        .when(running, |d| {
            d.child(
                Button::new(SharedString::from(format!("task-stop-{}", task.task_id))).xsmall().ghost().icon(IconName::Square).tooltip("停止任务")
                    .on_click(move |_, _, cx| { cx.stop_propagation(); s_stop.update(cx, |m, cx| m.stop_task(&id_stop, cx)); }),
            )
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 统计文案() {
        assert_eq!(meta_text(Some(65_000), 3, 12_300), "1 分 05 秒 · 3 次工具 · 12.3k token");
        assert_eq!(meta_text(None, 0, 0), "");
        assert_eq!(meta_text(Some(5_000), 0, 800), "5 秒 · 800 token");
    }
}
