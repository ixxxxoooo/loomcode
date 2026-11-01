//! 子 Agent 卡片(对标 OhMyAgent / Cursor,无框):两行纯文字。
//! 第一行「类型图标 + 任务标题 + 类型名(灰)」;第二行是状态、待办计数与当前活动 / 用时。点击在右侧标签里打开完整对话,右侧箭头展开内联预览。
//!
//! @author ixxxxoooo

use crate::model::{SessionModel, UiRequest};
use crate::timeline;
use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::*;

/// 子 Agent 卡片。
///
/// 参数:`card_data` 子 Agent 数据;`depth` 嵌套层级。返回:卡片元素。
pub fn render(session: &Entity<SessionModel>, card_data: &SubagentCard, depth: usize, window: &mut Window, cx: &mut App) -> AnyElement {
    let t = cx.theme().clone();
    let expanded = session.read(cx).is_expanded(&card_data.id);
    let (id, s) = (card_data.id.clone(), session.clone());
    let (tid, ts) = (card_data.id.clone(), session.clone());
    let active = matches!(card_data.status, SubagentStatus::Starting | SubagentStatus::Running);
    let icon_color = match card_data.status {
        SubagentStatus::Failed => t.danger,
        _ if active => t.primary,
        _ => t.muted_foreground,
    };
    let (label, label_color) = match card_data.status {
        SubagentStatus::Starting => ("启动中", t.muted_foreground),
        SubagentStatus::Running => ("运行中", t.primary),
        SubagentStatus::Background => ("后台运行", t.primary),
        SubagentStatus::Completed => ("已完成", t.success),
        SubagentStatus::Failed => ("已失败", t.danger),
    };
    let tools = card_data.children.iter().filter(|c| matches!(c, TimelineItem::Tool(_))).count();
    let todo = card_data.children.iter().rev().find_map(|c| match c {
        TimelineItem::Todos { items, .. } => Some((items.iter().filter(|i| i.status == TodoStatus::Completed).count(), items.len())),
        _ => None,
    });
    // 进行中显示最新活动;结束后显示工具调用数
    let trailing = if active || card_data.status == SubagentStatus::Background {
        card_data.children.last().map(|c| match c {
            TimelineItem::Tool(x) => x.title.clone(),
            TimelineItem::Assistant { text, .. } => text.lines().next().unwrap_or("").to_string(),
            _ => String::new(),
        })
    } else if tools > 0 {
        Some(format!("{tools} 次工具调用"))
    } else {
        None
    };
    let title = if card_data.description.is_empty() { card_data.agent_type.clone() } else { card_data.description.clone() };
    let head = h_flex().id(SharedString::from(format!("sub-{}", card_data.id))).w_full().px_2().py(px(5.)).gap_2().items_start().cursor_pointer()
        .hover(|d| d.bg(t.list_hover)).rounded(px(6.))
        // 点击整块:在右侧标签里打开完整对话;右侧箭头只展开 / 收起这里的内联预览
        .on_click(move |_, _, cx| { s.update(cx, |_, cx| cx.emit(UiRequest::OpenSubagent(id.clone()))); })
        .child(div().w(px(16.)).h(px(20.)).flex().items_center().justify_center().child(icon(IconName::Bot, 14., icon_color)))
        .child(v_flex().flex_1().min_w_0().gap(px(2.))
            .child(h_flex().gap_1p5().items_baseline()
                .child(div().min_w_0().truncate().text_size(px(13.)).text_color(t.foreground).child(title))
                .child(div().flex_none().text_size(px(12.)).text_color(t.muted_foreground.opacity(0.7)).child(card_data.agent_type.clone())))
            .child(h_flex().gap_2().items_center().text_size(px(12.))
                .child(h_flex().gap_1p5().items_center().when(active, |d| d.child(Spinner::new().xsmall()))
                    .when(card_data.status == SubagentStatus::Completed, |d| d.child(icon(IconName::CircleCheck, 12., t.success)))
                    .when(card_data.status == SubagentStatus::Failed, |d| d.child(icon(IconName::CircleX, 12., t.danger)))
                    .child(div().text_color(label_color).child(label)))
                .when_some(todo.filter(|(_, n)| *n > 0), |d, (a, n)| d.child(div().text_size(px(11.)).text_color(t.muted_foreground).child(format!("待办 {a}/{n}"))))
                .when_some(trailing.filter(|x| !x.is_empty()), |d, x| d.child(div().flex_1().min_w_0().truncate().text_size(px(11.)).text_color(t.muted_foreground.opacity(0.8)).child(x)))));
    let head = head.child(
        div().id(SharedString::from(format!("sub-t-{}", card_data.id))).flex_none().size(px(20.)).flex().items_center().justify_center().rounded(px(4.)).hover(|d| d.bg(t.border))
            .on_click(move |_, _, cx| { cx.stop_propagation(); ts.update(cx, |m, cx| m.toggle(&tid, cx)); })
            .child(icon(if expanded { IconName::ChevronDown } else { IconName::ChevronRight }, 12., t.muted_foreground)),
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
