//! 子 Agent 卡片(对标 OhMyAgent / Cursor,无框):两行纯文字。
//! 第一行「类型图标 + 任务标题 + 类型名(灰)」;第二行是状态、待办计数与当前活动 / 用时。点击在右侧标签里打开完整对话,右侧箭头展开内联预览。
//!
//! @author ixxxxoooo

use crate::model::{SessionModel, UiRequest};
use crate::timeline;
use crate::ui::{icon, pill};
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::*;

/// 子 Agent 卡片。
///
/// 参数:`card_data` 子 Agent 数据;`depth` 嵌套层级。返回:卡片元素。
pub fn render(session: &Entity<SessionModel>, card_data: &SubagentCard, depth: usize, window: &mut Window, cx: &mut App) -> AnyElement {
    let t = cx.theme().clone();
    // 失败的卡片默认展开(能直接看到出错前做了什么);手动点击反转默认状态
    let by_default = card_data.status == SubagentStatus::Failed && !card_data.children.is_empty();
    let expanded = session.read(cx).is_expanded(&card_data.id) != by_default;
    let (id, s) = (card_data.id.clone(), session.clone());
    let (tid, ts) = (card_data.id.clone(), session.clone());
    let active = card_data.is_active();
    let now = now_ms();
    let stalled = card_data.stalled(now);
    let (label, label_color) = crate::task_ui::subagent_style(card_data.status, cx);
    let icon_color = match card_data.status {
        SubagentStatus::Failed => t.danger,
        _ if stalled => t.warning,
        _ if active => t.primary,
        _ => t.muted_foreground,
    };
    let todo = card_data.children.iter().rev().find_map(|c| match c {
        TimelineItem::Todos { items, .. } => Some((items.iter().filter(|i| i.status == TodoStatus::Completed).count(), items.len())),
        _ => None,
    });
    let meta = crate::task_ui::meta_text(card_data.elapsed_ms(now), card_data.tools(), card_data.tokens);
    // 进行中显示当前动作(协议上报的摘要优先,其次最近的工具,再次最新一条嵌套活动);结束后显示结果摘要
    let current = if active {
        Some(if !card_data.summary.is_empty() {
            card_data.summary.clone()
        } else if !card_data.last_tool.is_empty() {
            format!("正在 {}", card_data.last_tool)
        } else {
            card_data.children.last().map(|c| match c {
                TimelineItem::Tool(x) => x.title.clone(),
                TimelineItem::Assistant { text, .. } => text.lines().next().unwrap_or("").to_string(),
                _ => String::new(),
            }).unwrap_or_default()
        })
    } else {
        Some(card_data.summary.clone())
    };
    let stalled_text = stalled.then(|| format!("已 {} 无进展", format_elapsed(now.saturating_sub(card_data.last_activity_ms.unwrap_or(now)))));
    let title = if card_data.description.is_empty() { card_data.agent_type.clone() } else { card_data.description.clone() };
    let stop_target = card_data.task_id.clone().filter(|_| active);
    let to_bg = card_data.task_id.clone().filter(|_| active && !card_data.background).map(|_| card_data.id.clone());
    let head = h_flex().id(SharedString::from(format!("sub-{}", card_data.id))).w_full().px_2().py(px(5.)).gap_2().items_start().cursor_pointer()
        .hover(|d| d.bg(t.list_hover)).rounded(px(6.))
        // 点击整块:在右侧标签里打开完整对话;右侧箭头只展开 / 收起这里的内联预览
        .on_click(move |_, _, cx| { s.update(cx, |_, cx| cx.emit(UiRequest::OpenSubagent(id.clone()))); })
        .child(div().w(px(16.)).h(px(20.)).flex().items_center().justify_center().child(icon(IconName::Bot, 14., icon_color)))
        .child(v_flex().flex_1().min_w_0().gap(px(2.))
            .child(h_flex().gap_1p5().items_baseline()
                .child(div().min_w_0().truncate().text_size(px(13.)).text_color(t.foreground).child(title))
                .child(div().flex_none().text_size(px(12.)).text_color(t.muted_foreground.opacity(0.7)).child(card_data.agent_type.clone()))
                .when(card_data.background, |d| d.child(pill("后台", t.primary, t.muted))))
            .child(h_flex().gap_2().items_center().text_size(px(12.))
                .child(h_flex().gap_1p5().items_center().flex_none().when(active, |d| d.child(crate::appearance::row_spinner(&format!("sub-sp-{}", card_data.id), cx)))
                    .when(card_data.status == SubagentStatus::Completed, |d| d.child(icon(IconName::CircleCheck, 12., t.success)))
                    .when(card_data.status == SubagentStatus::Failed, |d| d.child(icon(IconName::CircleX, 12., t.danger)))
                    .when(card_data.status == SubagentStatus::Stopped, |d| d.child(icon(IconName::CircleStop, 12., t.muted_foreground)))
                    .child(div().text_color(label_color).child(label)))
                .when(!meta.is_empty(), |d| d.child(div().flex_none().text_size(px(11.)).text_color(t.muted_foreground).child(meta)))
                .when_some(todo.filter(|(_, n)| *n > 0), |d, (a, n)| d.child(div().flex_none().text_size(px(11.)).text_color(t.muted_foreground).child(format!("待办 {a}/{n}"))))
                .when_some(stalled_text, |d, x| d.child(h_flex().flex_none().gap_1().items_center().child(icon(IconName::TriangleAlert, 11., t.warning)).child(div().text_size(px(11.)).text_color(t.warning).child(x))))
                .when_some(current.filter(|x| !x.is_empty()), |d, x| d.child(div().flex_1().min_w_0().truncate().text_size(px(11.)).text_color(t.muted_foreground.opacity(0.8)).child(x)))));
    let head = head
        .when_some(to_bg, |d, cid| {
            let m = session.clone();
            d.child(
                div().id(SharedString::from(format!("sub-bg-{}", card_data.id))).flex_none().size(px(20.)).flex().items_center().justify_center().rounded(px(4.)).hover(|d| d.bg(t.border))
                    .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("转入后台继续运行").build(window, cx))
                    .on_click(move |_, _, cx| { cx.stop_propagation(); m.update(cx, |m, cx| { m.handle.send(AgentCommand::BackgroundCurrent { tool_use_id: Some(cid.clone()) }); cx.notify(); }); })
                    .child(icon(IconName::ArrowDownToLine, 12., t.muted_foreground)),
            )
        })
        .when_some(stop_target, |d, task| {
            let m = session.clone();
            d.child(
                div().id(SharedString::from(format!("sub-stop-{}", card_data.id))).flex_none().size(px(20.)).flex().items_center().justify_center().rounded(px(4.)).hover(|d| d.bg(t.border))
                    .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("停止这个子 Agent").build(window, cx))
                    .on_click(move |_, _, cx| { cx.stop_propagation(); m.update(cx, |m, cx| m.stop_task(&task, cx)); })
                    .child(icon(IconName::Square, 11., t.muted_foreground)),
            )
        })
        .child(
        div().id(SharedString::from(format!("sub-t-{}", card_data.id))).flex_none().size(px(20.)).flex().items_center().justify_center().rounded(px(4.)).hover(|d| d.bg(t.border))
            .on_click(move |_, _, cx| { cx.stop_propagation(); ts.update(cx, |m, cx| m.toggle(&tid, cx)); })
            .child(session.read(cx).motion.chevron(&card_data.id, expanded, 12., t.muted_foreground)),
    );
    let mut col = v_flex().w_full().child(head);
    if expanded {
        let mut body = v_flex().w_full().pl(px(26.)).pr_2().gap_1();
        for child in &card_data.children {
            body = body.child(timeline::render_item(session, child, depth + 1, window, cx));
        }
        if card_data.children.is_empty() {
            body = body.child(div().text_size(px(12.)).text_color(t.muted_foreground).child("暂无活动"));
        }
        col = col.child(body);
    }
    col.into_any_element()
}
