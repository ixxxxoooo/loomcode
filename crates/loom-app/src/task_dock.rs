//! 输入框上方的后台任务坞。
//!
//! 胶囊固定在输入框上方,点击后列表在胶囊**上方**悬浮展开(不挤动布局);再点胶囊、点列表以外、按 Esc 都会收起。
//! 列表按类别分组(子代理 / 后台命令 / 监听 / 工作流 / 其他),已结束的任务折叠在最后的「已结束」里;
//! 完整输出与历史在右侧「任务」标签里。
//!
//! @author ixxxxoooo

use crate::model::{SessionModel, UiRequest};
use crate::task_ui::{kind_icon, meta_text, status_icon, task_color};
use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::*;

/// 「已结束」分组展开后最多显示的行数。
const FINISHED_ROWS: usize = 20;
/// 悬浮列表的最大高度。
const PANEL_MAX_H: f32 = 380.;

/// 输入框上方的任务坞;没有可显示的任务(或没有运行中的任务且列表没展开)时返回 `None`。
///
/// 参数:`session` 会话模型。返回:与输入框同宽的计数胶囊,展开时带向上悬浮的分组列表。
pub fn dock(session: &Entity<SessionModel>, cx: &App) -> Option<AnyElement> {
    let m = session.read(cx);
    let running = m.state.running_task_count();
    if !m.state.tasks.iter().any(|t| t.visible()) || (running == 0 && !m.tasks_open) {
        return None;
    }
    let t = cx.theme();
    let open = m.tasks_open;
    let now = now_ms();
    let counts = |k: TaskKind| m.state.tasks.iter().filter(|x| x.counts_as_running() && x.kind == k).count();
    let parts: Vec<String> = [(TaskKind::Agent, "子代理"), (TaskKind::Shell, "命令"), (TaskKind::Monitor, "监听"), (TaskKind::Workflow, "工作流")]
        .into_iter().filter(|(k, _)| counts(*k) > 0).map(|(k, unit)| format!("{} {unit}", counts(k))).collect();
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
    // 胶囊始终占固定的一行高度;列表是绝对定位的悬浮层,不参与布局
    let mut host = div().relative().w_full().max_w(px(crate::timeline::content_max_w(cx))).h(px(28.)).child(capsule);
    if open {
        host = host.child(panel(session, now, cx));
    }
    Some(div().w_full().flex().justify_center().px_4().pt_1().pb_1().child(host).into_any_element())
}

/// 悬浮的分组列表。
fn panel(session: &Entity<SessionModel>, now: u64, cx: &App) -> AnyElement {
    let t = cx.theme();
    let m = session.read(cx);
    let sections = group_tasks(&m.state.tasks);
    let total: usize = sections.iter().map(|s| s.tasks.len()).sum();
    let finished_open = m.tasks_finished_open;
    let mut body = v_flex().w_full().py_1();
    for sec in &sections {
        let finished = sec.group == TaskGroup::Finished;
        let running = sec.tasks.iter().filter(|x| x.counts_as_running()).count();
        let title = if finished { format!("{} · {}", sec.group.label(), sec.tasks.len()) } else if running == sec.tasks.len() { format!("{} · {}", sec.group.label(), running) } else { format!("{} · {} 个运行中", sec.group.label(), running) };
        let s = session.clone();
        body = body.child(
            h_flex().id(SharedString::from(format!("dock-group-{:?}", sec.group))).h(px(26.)).px_3().gap_1p5().items_center().text_size(px(11.)).text_color(t.muted_foreground)
                .when(finished, |d| d.cursor_pointer().hover(|d| d.text_color(t.foreground)).on_click(move |_, _, cx| { s.update(cx, |m, cx| m.toggle_finished_tasks(cx)); }))
                .child(match sec.group.kind() {
                    Some(k) => icon(kind_icon(k), 12., t.muted_foreground).into_any_element(),
                    None => icon(if finished_open { IconName::ChevronDown } else { IconName::ChevronRight }, 12., t.muted_foreground).into_any_element(),
                })
                .child(title),
        );
        if finished && !finished_open {
            continue;
        }
        let limit = if finished { FINISHED_ROWS } else { usize::MAX };
        for task in sec.tasks.iter().take(limit) {
            body = body.child(if sec.group == TaskGroup::Agent { agent_row(session, task, now, cx) } else { command_row(session, task, now, cx) });
        }
    }
    let s2 = session.clone();
    let s_out = session.clone();
    v_flex().id("task-dock-panel").absolute().bottom(px(32.)).left_0().w_full().occlude()
        .rounded(px(10.)).border_1().border_color(t.border).bg(t.popover).shadow_lg().overflow_hidden()
        .on_mouse_down_out(move |_, _, cx| { s_out.update(cx, |m, cx| m.dismiss_tasks_outside(cx)); })
        .child(v_flex().id("task-dock-list").w_full().max_h(px(PANEL_MAX_H)).overflow_y_scroll().child(body))
        .child(
            h_flex().id("task-dock-all").h(px(28.)).px_3().gap_1().items_center().flex_none().border_t_1().border_color(t.border).cursor_pointer().text_size(px(11.)).text_color(t.muted_foreground)
                .hover(|d| d.text_color(t.foreground))
                .on_click(move |_, _, cx| { s2.update(cx, |m, cx| { m.close_tasks(cx); cx.emit(UiRequest::OpenTasks(None)); }); })
                .child(format!("在右侧「任务」标签查看全部 {total} 个"))
                .child(icon(IconName::ChevronRight, 11., t.muted_foreground)),
        )
        .into_any_element()
}

/// 行的公共外壳:点击查看(并收起列表)。
fn row_shell(session: &Entity<SessionModel>, task: &BackgroundTask, cx: &App) -> Stateful<Div> {
    let t = cx.theme();
    let (s, id) = (session.clone(), task.task_id.clone());
    h_flex().id(SharedString::from(format!("task-row-{}", task.task_id))).w_full().h(px(34.)).px_3().gap_2().items_center().cursor_pointer()
        .hover(|d| d.bg(t.list_hover))
        .on_click(move |_, _, cx| { s.update(cx, |m, cx| { m.close_tasks(cx); cx.emit(UiRequest::OpenTasks(Some(id.clone()))); }); })
}

/// 状态位:运行中是转圈,否则是终态图标。
fn status_mark(task: &BackgroundTask, cx: &App) -> AnyElement {
    if task.counts_as_running() { Spinner::new().xsmall().into_any_element() } else { icon(status_icon(task.status), 13., task_color(task.status, cx)).into_any_element() }
}

/// 停止按钮(只有运行中的任务才有)。
fn stop_button(session: &Entity<SessionModel>, task: &BackgroundTask) -> Option<AnyElement> {
    task.counts_as_running().then(|| {
        let (s, id) = (session.clone(), task.task_id.clone());
        Button::new(SharedString::from(format!("task-stop-{}", task.task_id))).xsmall().ghost().icon(IconName::Square).tooltip("停止任务")
            .on_click(move |_, _, cx| { cx.stop_propagation(); s.update(cx, |m, cx| m.stop_task(&id, cx)); })
            .into_any_element()
    })
}

/// 子代理行:名称 + 当前动作 / 摘要 / 停滞提示 + 用时、工具数、token。
fn agent_row(session: &Entity<SessionModel>, task: &BackgroundTask, now: u64, cx: &App) -> AnyElement {
    let t = cx.theme();
    let running = task.counts_as_running();
    let stalled = running && task.idle_ms(now) >= STALL_MS;
    let detail = if stalled {
        format!("已 {} 无进展", format_elapsed(task.idle_ms(now)))
    } else if !task.summary.is_empty() && (running || task.status != TaskStatus::Completed) {
        task.summary.clone()
    } else if running && !task.last_tool.is_empty() {
        format!("正在 {}", task.last_tool)
    } else {
        task.status.label().to_string()
    };
    let meta = meta_text(Some(task.elapsed_ms(now)), task.tool_uses, task.tokens);
    row_shell(session, task, cx)
        .child(status_mark(task, cx))
        .child(div().flex_none().max_w(px(200.)).truncate().text_size(px(13.)).text_color(if running { t.foreground } else { t.muted_foreground }).child(task.description.clone()))
        .child(div().flex_1().min_w_0().truncate().text_size(px(11.)).text_color(if stalled { t.warning } else { t.muted_foreground }).child(detail))
        .child(div().flex_none().text_size(px(11.)).text_color(t.muted_foreground.opacity(0.8)).child(meta))
        .children(stop_button(session, task))
        .into_any_element()
}

/// 命令 / 监听 / 工作流 / 其他行:描述 + 运行时长(已结束显示终态)。
fn command_row(session: &Entity<SessionModel>, task: &BackgroundTask, now: u64, cx: &App) -> AnyElement {
    let t = cx.theme();
    let running = task.counts_as_running();
    let timing = if running { format!("已运行 {}", format_elapsed(task.elapsed_ms(now))) } else { format!("{} · 用时 {}", task.status.label(), format_elapsed(task.elapsed_ms(now))) };
    row_shell(session, task, cx)
        .child(status_mark(task, cx))
        .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).text_color(if running { t.foreground } else { t.muted_foreground }).child(task.description.clone()))
        .child(div().flex_none().text_size(px(11.)).text_color(t.muted_foreground).child(timing))
        .children(stop_button(session, task))
        .into_any_element()
}
