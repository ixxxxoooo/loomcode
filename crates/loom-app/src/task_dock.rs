//! 输入框上方的后台任务坞。
//!
//! 每个类别(子代理 / 后台命令 / 监听 / 工作流 / 其他 / 已结束)一个胶囊,排成一排,固定在输入框上方;
//! 点哪个胶囊,那一类的列表就在上方悬浮展开(不挤动布局);再点同一个胶囊、点列表以外、按 Esc 都会收起,点另一个胶囊直接切换;
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

/// 「已结束」列表最多显示的行数。
const FINISHED_ROWS: usize = 20;
/// 悬浮列表的最大高度。
const PANEL_MAX_H: f32 = 380.;

/// 输入框上方的任务坞:每个类别一个胶囊,排成一排;点哪个胶囊,列表就在上方悬浮展开那一类。
///
/// 参数:`session` 会话模型。返回:胶囊行(含向上悬浮的列表);没有可显示的任务,或没有运行中的任务且没展开时返回 `None`。
pub fn dock(session: &Entity<SessionModel>, cx: &App) -> Option<AnyElement> {
    let m = session.read(cx);
    let sections = group_tasks(&m.state.tasks);
    if sections.is_empty() || (m.state.running_task_count() == 0 && m.tasks_open.is_none()) {
        return None;
    }
    let now = now_ms();
    let capsules: Vec<AnyElement> = sections.iter().map(|sec| capsule(session, sec, m.tasks_open == Some(sec.group), now, cx)).collect();
    // 胶囊行始终占固定的一行高度;列表是绝对定位的悬浮层,不参与布局
    let mut host = div().relative().w_full().max_w(px(crate::timeline::content_max_w(cx))).child(h_flex().w_full().min_h(px(28.)).gap_2().children(capsules));
    if let Some(sec) = m.tasks_open.and_then(|g| sections.iter().find(|s| s.group == g)) {
        host = host.child(panel(session, sec, group_total(&sections), now, cx));
    }
    Some(div().w_full().flex().justify_center().px_4().pt_1().pb_1().child(host).into_any_element())
}

/// 全部任务数(「查看全部」文案用)。
fn group_total(sections: &[TaskSection<'_>]) -> usize {
    sections.iter().map(|s| s.tasks.len()).sum()
}

/// 一类任务的胶囊:图标(有运行中的转圈)+ 类别名 + 数量,子代理有疑似停滞时带警告;展开的那个高亮。
fn capsule(session: &Entity<SessionModel>, sec: &TaskSection<'_>, active: bool, now: u64, cx: &App) -> AnyElement {
    let t = cx.theme();
    let group = sec.group;
    let running = sec.tasks.iter().filter(|x| x.counts_as_running()).count();
    let stalled = sec.tasks.iter().filter(|x| x.counts_as_running() && x.idle_ms(now) >= STALL_MS).count();
    let finished = group == TaskGroup::Finished;
    let s = session.clone();
    h_flex().id(SharedString::from(format!("task-capsule-{group:?}"))).h(px(28.)).px_2p5().gap_1p5().items_center().rounded(px(14.)).cursor_pointer()
        .border_1().border_color(if active { t.primary } else { t.border }).bg(if active { t.list_active } else { t.background }).hover(|d| d.bg(t.list_hover))
        .on_click(move |_, _, cx| { s.update(cx, |m, cx| m.toggle_tasks(group, cx)); })
        .child(if running > 0 { Spinner::new().xsmall().into_any_element() } else { icon(group.kind().map_or(IconName::Check, kind_icon), 12., t.muted_foreground).into_any_element() })
        .child(div().text_size(px(12.)).text_color(if finished { t.muted_foreground } else { t.foreground }).child(format!("{} {}", if group == TaskGroup::Other { "其他" } else { group.label() }, if finished { sec.tasks.len() } else { running.max(sec.tasks.len()) })))
        .when(stalled > 0, |d| d.child(h_flex().gap_0p5().items_center().child(icon(IconName::TriangleAlert, 11., t.warning)).child(div().text_size(px(11.)).text_color(t.warning).child(format!("{stalled} 停滞")))))
        .into_any_element()
}

/// 悬浮的任务列表(只含被点开的那一类)。
fn panel(session: &Entity<SessionModel>, sec: &TaskSection<'_>, total: usize, now: u64, cx: &App) -> AnyElement {
    let t = cx.theme();
    let limit = if sec.group == TaskGroup::Finished { FINISHED_ROWS } else { usize::MAX };
    let rows = sec.tasks.iter().take(limit).map(|task| if sec.group == TaskGroup::Agent { agent_row(session, task, now, cx) } else { command_row(session, task, now, cx) });
    let s2 = session.clone();
    let s_out = session.clone();
    v_flex().id("task-dock-panel").absolute().bottom(px(32.)).left_0().w_full().occlude()
        .rounded(px(10.)).border_1().border_color(t.border).bg(t.popover).shadow_lg().overflow_hidden()
        .on_mouse_down_out(move |_, _, cx| { s_out.update(cx, |m, cx| m.dismiss_tasks_outside(cx)); })
        .child(v_flex().id("task-dock-list").w_full().max_h(px(PANEL_MAX_H)).overflow_y_scroll().py_1().children(rows))
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
