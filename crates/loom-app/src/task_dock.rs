//! 输入框上方的后台任务坞。
//!
//! 每个类别(子代理 / 后台命令 / 监听 / 工作流 / 其他 / 已结束)一个胶囊,排成一排,固定在输入框上方;
//! 点哪个胶囊,那一类的列表就在上方悬浮展开(不挤动布局);再点同一个胶囊、点列表以外、按 Esc 都会收起,点另一个胶囊直接切换;
//! 完整输出与历史在右侧「任务」标签里。
//!
//! @author ixxxxoooo

use crate::model::{SessionModel, UiRequest};
use crate::task_ui::{kind_icon, status_icon, task_color};
use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::*;

/// 「已结束」列表最多显示的行数。
const FINISHED_ROWS: usize = 20;
/// 面板列表的最大高度(对标 OhMyAgent 的 196)。
const PANEL_MAX_H: f32 = 196.;

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
        host = host.child(panel(session, sec, now, cx));
    }
    Some(div().w_full().flex().justify_center().px_4().pt_1().pb_1().child(host).into_any_element())
}

/// 一类任务的胶囊(对标 OhMyAgent `DockChip`):类别图标 + 类别名 + 数量(灰色);展开的那个高亮;
/// 子代理有疑似停滞时多一个警告。图标区分类别,不用转圈。
fn capsule(session: &Entity<SessionModel>, sec: &TaskSection<'_>, active: bool, now: u64, cx: &App) -> AnyElement {
    let t = cx.theme();
    let group = sec.group;
    let stalled = sec.tasks.iter().filter(|x| x.counts_as_running() && x.idle_ms(now) >= STALL_MS).count();
    let s = session.clone();
    let fg = if active { t.foreground } else { t.muted_foreground };
    h_flex().id(SharedString::from(format!("task-capsule-{group:?}"))).h(px(26.)).px(px(11.)).gap(px(5.)).items_center().rounded(px(13.)).cursor_pointer()
        .border_1().border_color(if active { t.primary.opacity(0.5) } else { t.border }).bg(if active { t.list_active } else { t.background }).hover(|d| d.bg(t.list_hover))
        .on_click(move |_, _, cx| { s.update(cx, |m, cx| m.toggle_tasks(group, cx)); })
        .child(icon(group_icon(group), 11., fg))
        .child(div().text_size(px(12.)).text_color(fg).child(if group == TaskGroup::Other { "其他" } else { group.label() }))
        .child(div().text_size(px(12.)).text_color(t.muted_foreground.opacity(0.75)).child(sec.tasks.len().to_string()))
        .when(stalled > 0, |d| d.child(h_flex().gap_0p5().items_center().child(icon(IconName::TriangleAlert, 11., t.warning)).child(div().text_size(px(11.)).text_color(t.warning).child(format!("{stalled} 停滞")))))
        .into_any_element()
}

/// 类别的图标:各类任务各一个,「已结束」是对勾。
fn group_icon(group: TaskGroup) -> IconName {
    group.kind().map_or(IconName::Check, kind_icon)
}

/// 悬浮的任务面板(对标 OhMyAgent `ComposerDockView.panel`):标题行(「N 个xx运行中」+ 全部停止 + 收起)+ 可滚动的行列表(最高 196)。
fn panel(session: &Entity<SessionModel>, sec: &TaskSection<'_>, now: u64, cx: &App) -> AnyElement {
    let t = cx.theme();
    let finished = sec.group == TaskGroup::Finished;
    let running: Vec<String> = sec.tasks.iter().filter(|x| x.counts_as_running()).map(|x| x.task_id.clone()).collect();
    let title = if finished { format!("{} 个已结束任务", sec.tasks.len()) } else { format!("{} 个{}运行中", running.len(), sec.group.label()) };
    let limit = if finished { FINISHED_ROWS } else { usize::MAX };
    let rows: Vec<AnyElement> = sec.tasks.iter().take(limit).map(|task| dock_row(session, task, now, cx)).collect();
    let (s_out, s_close, s_stop) = (session.clone(), session.clone(), session.clone());
    v_flex().id("task-dock-panel").absolute().bottom(px(32.)).left_0().w_full().occlude()
        .rounded(px(10.)).border_1().border_color(t.border).bg(t.popover).shadow_lg().overflow_hidden()
        .on_mouse_down_out(move |_, _, cx| { s_out.update(cx, |m, cx| m.dismiss_tasks_outside(cx)); })
        .child(
            h_flex().h(px(32.)).px_3().gap_2().items_center().flex_none()
                .child(div().flex_1().text_size(px(12.)).text_color(t.muted_foreground).child(title))
                .when(!running.is_empty(), |d| d.child(
                    div().id("task-dock-stop-all").cursor_pointer().text_size(px(12.)).text_color(t.muted_foreground).hover(|d| d.text_color(t.foreground)).child("全部停止")
                        .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new("停止这一类所有运行中的任务").build(window, cx))
                        .on_click(move |_, _, cx| { let ids = running.clone(); s_stop.update(cx, |m, cx| { for id in &ids { m.stop_task(id, cx); } }); }),
                ))
                .child(
                    div().id("task-dock-close").size(px(22.)).flex().items_center().justify_center().rounded(px(4.)).cursor_pointer().hover(|d| d.bg(t.list_hover))
                        .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new("收起").build(window, cx))
                        .on_click(move |_, _, cx| { s_close.update(cx, |m, cx| { m.close_tasks(cx); }); })
                        .child(icon(IconName::X, 12., t.muted_foreground)),
                ),
        )
        .child(v_flex().id("task-dock-list").w_full().max_h(px(PANEL_MAX_H)).overflow_y_scroll().px_1p5().pb_1p5().gap(px(1.)).children(rows))
        .into_any_element()
}

/// 面板里的一行(对标 OhMyAgent `DockRowShell`,高 30):类别图标(已结束的用状态图标)+ 标题 + 右侧灰色元信息;
/// 悬停时元信息换成操作按钮(查看详情、停止)。点整行查看详情并收起面板。
fn dock_row(session: &Entity<SessionModel>, task: &BackgroundTask, now: u64, cx: &App) -> AnyElement {
    let t = cx.theme();
    let running = task.counts_as_running();
    let stalled = running && task.idle_ms(now) >= STALL_MS;
    let elapsed = format_elapsed(task.elapsed_ms(now));
    let meta = if stalled {
        format!("已 {} 无进展 · {elapsed}", format_elapsed(task.idle_ms(now)))
    } else if !running {
        format!("{} · {elapsed}", task.status.label())
    } else if task.kind == TaskKind::Agent && !task.last_tool.is_empty() {
        format!("{} · {elapsed}", task.last_tool)
    } else {
        elapsed
    };
    let group_name = SharedString::from(format!("dock-row-{}", task.task_id));
    let leading = if running {
        icon(kind_icon(task.kind), 12., if task.kind == TaskKind::Agent { t.primary } else { t.muted_foreground }).into_any_element()
    } else {
        icon(status_icon(task.status), 12., task_color(task.status, cx)).into_any_element()
    };
    let (s_view, id_view) = (session.clone(), task.task_id.clone());
    let (s_open, id_open) = (session.clone(), task.task_id.clone());
    let (s_stop, id_stop) = (session.clone(), task.task_id.clone());
    h_flex().id(SharedString::from(format!("task-row-{}", task.task_id))).group(group_name.clone()).w_full().h(px(30.)).px_2().gap_2().items_center().rounded(px(6.)).cursor_pointer()
        .hover(|d| d.bg(t.list_hover))
        .on_click(move |_, _, cx| { s_view.update(cx, |m, cx| { m.close_tasks(cx); cx.emit(UiRequest::OpenTasks(Some(id_view.clone()))); }); })
        .child(div().w(px(16.)).flex().justify_center().flex_none().child(leading))
        .child(div().flex_1().min_w_0().truncate().text_size(px(12.5)).text_color(if running { t.foreground } else { t.muted_foreground }).child(task.description.clone()))
        .child(
            div().relative().flex_none().h_full().flex().items_center().justify_end()
                .child(div().max_w(px(260.)).truncate().text_size(px(11.)).text_color(if stalled { t.warning } else { t.muted_foreground.opacity(0.8) }).group_hover(group_name.clone(), |d| d.invisible()).child(meta))
                .child(
                    h_flex().absolute().right_0().gap_0p5().items_center().invisible().group_hover(group_name, |d| d.visible())
                        .child(row_button(format!("open-{}", task.task_id), IconName::ExternalLink, "查看详情", cx, move |cx| { s_open.update(cx, |m, cx| { m.close_tasks(cx); cx.emit(UiRequest::OpenTasks(Some(id_open.clone()))); }); }))
                        .when(running, |d| d.child(row_button(format!("stop-{}", task.task_id), IconName::Square, "停止", cx, move |cx| { s_stop.update(cx, |m, cx| m.stop_task(&id_stop, cx)); }))),
                ),
        )
        .into_any_element()
}

/// 行内小图标按钮(22×22,对标 OhMyAgent `RowIconButton`):点击不触发整行的点击。
fn row_button(id: String, name: IconName, tip: &'static str, cx: &App, on: impl Fn(&mut App) + 'static) -> AnyElement {
    let t = cx.theme();
    div().id(SharedString::from(id)).size(px(22.)).flex().items_center().justify_center().rounded(px(4.)).cursor_pointer().hover(|d| d.bg(t.list_active))
        .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tip).build(window, cx))
        .on_click(move |_, _, cx| { cx.stop_propagation(); on(cx); })
        .child(icon(name, 12., t.muted_foreground))
        .into_any_element()
}
