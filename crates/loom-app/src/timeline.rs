//! 会话时间线:把条目分发到各自的渲染函数。

use crate::cards;
use crate::model::SessionModel;
use crate::tool_card;
use crate::ui::{icon, pill};
use gpui_kit::assets::IconName;
use gpui_kit::component::shimmer::ShimmerText;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::*;

/// 时间线内容的最大宽度(居中)。
/// 会话内容最大宽度(pt),取自外观偏好。
pub fn content_max_w(cx: &App) -> f32 {
    cx.try_global::<crate::UiPrefs>().map_or(820., |p| p.chat_width)
}

/// 虚拟列表的一行:加上居中容器与垂直间距。
pub fn render_row(session: &Entity<SessionModel>, ix: usize, window: &mut Window, cx: &mut App) -> AnyElement {
    let (row, count) = {
        let m = session.read(cx);
        (m.rows.get(ix).cloned(), m.rows.len())
    };
    let Some(row) = row else { return div().into_any_element() };
    let body = match row {
        crate::row_groups::Row::Item(i) => {
            let item = session.read(cx).state.items.get(i).cloned();
            let Some(item) = item else { return div().into_any_element() };
            let running = session.read(cx).state.phase == Phase::Running;
            let footer = match &item {
                TimelineItem::Assistant { id, text, .. } if crate::row_groups::is_turn_last_assistant(&session.read(cx).state.items, i, running) => Some((id.clone(), text.clone())),
                _ => None,
            };
            let body = render_item(session, &item, 0, window, cx);
            match footer {
                Some((id, text)) => {
                    let meta = crate::turn_footer::meta_text(session, i, cx);
                    v_flex().w_full().child(body).child(crate::turn_footer::render(session, &id, &text, meta, cx)).into_any_element()
                }
                None => body,
            }
        }
        crate::row_groups::Row::Tools(range) => crate::tool_group::render_group(session, range, cx),
        crate::row_groups::Row::Agents(range) => crate::agent_group::render_group(session, range, window, cx),
        crate::row_groups::Row::Digest(range) => crate::turn_digest::render_digest(session, range, window, cx),
        crate::row_groups::Row::Working => render_working(session, cx),
    };
    div()
        .w_full()
        .flex()
        .justify_center()
        .px_1()
        .py(px(5.))
        // 首行顶部、末行底部留白(不能用列表内边距,见 workbench 里的说明);要放在 py 之后才生效
        .when(ix == 0, |d| d.pt(px(13.)))
        .when(ix + 1 == count, |d| d.pb(px(25.)))
        .child(div().w_full().max_w(px(content_max_w(cx))).child(body))
        .into_any_element()
}

/// 回合进行中、暂时没有内容在流出时的「处理中」提示行。
fn render_working(session: &Entity<SessionModel>, cx: &App) -> AnyElement {
    let (seed, started) = (session.entity_id().as_u64(), session.read(cx).turn_started);
    let t = cx.theme();
    h_flex().h(px(26.)).px_2().gap_2().items_center()
        .child(crate::appearance::glyph_spinner("working", t.primary, cx))
        .child(ShimmerText::new(crate::appearance::status_line(crate::appearance::verb_text(cx, &seed.to_string()), started)).id("working-shimmer"))
        .text_size(px(13.)).text_color(t.muted_foreground)
        .into_any_element()
}

/// 渲染一个条目(可嵌套在子 Agent 里,`depth` 为嵌套层级)。
pub fn render_item(session: &Entity<SessionModel>, item: &TimelineItem, depth: usize, window: &mut Window, cx: &mut App) -> AnyElement {
    match item {
        TimelineItem::User { id, text, images, files } => crate::user_message::render_user(session, id, text, images, files, cx),
        TimelineItem::Assistant { id, text, .. } => crate::rich_text::render_assistant(session, id, text, cx),
        TimelineItem::Thinking { id, text, streaming } => render_thinking(session, id, text, *streaming, cx),
        TimelineItem::Tool(tool) => tool_card::render_tool(session, tool, cx),
        TimelineItem::Todos { id, items } => cards::render_todos(id, items, cx),
        TimelineItem::Subagent(card) => crate::subagent_card::render(session, card, depth, window, cx),
        TimelineItem::Permission(req) => crate::ask_cards::render_permission(req, cx),
        TimelineItem::Question(req) => crate::ask_cards::render_question(req, cx),
        TimelineItem::PlanApproval(plan) => crate::ask_cards::render_plan(plan, cx),
        TimelineItem::Notice { level, text, .. } => cards::render_notice(*level, text, cx),
        TimelineItem::Canvas { id, title } => cards::render_canvas(session, id, title, cx),
        TimelineItem::Changes { files, .. } => cards::render_changes(session, files, cx),
        TimelineItem::TaskNote { task_id, kind, status, text, .. } => crate::task_ui::render_note(session, task_id, *kind, *status, text, cx),
    }
}

/// 「已思考 N 秒」文案:不到 1 秒或没有实时计时(历史回放)就只写「已思考」。
///
/// 参数:`secs` 思考用时(秒)。
pub fn thought_label(secs: Option<u64>) -> String {
    match secs {
        Some(s) if s >= 60 => format!("已思考 {} 分 {} 秒", s / 60, s % 60),
        Some(s) if s >= 1 => format!("已思考 {s} 秒"),
        _ => "已思考".to_string(),
    }
}

/// 思考:折叠行,流式时显示流光「思考中…」。
fn render_thinking(session: &Entity<SessionModel>, id: &str, text: &str, streaming: bool, cx: &App) -> AnyElement {
    let t = cx.theme();
    // 偏好「默认展开思考」时,折叠状态取反(再点一次才折叠)
    let by_default = cx.try_global::<crate::UiPrefs>().is_some_and(|p| p.expand_thinking);
    let toggled = session.read(cx).is_expanded(id);
    let expanded = toggled != by_default;
    let s = session.clone();
    let sid = id.to_string();
    v_flex().w_full().gap_1()
        .child(
            h_flex().id(SharedString::from(format!("think-{id}"))).gap_1p5().items_center().cursor_pointer().h(px(24.))
                .on_click(move |_, _, cx| { s.update(cx, |m, cx| m.toggle(&sid, cx)); })
                .child(if streaming { crate::appearance::glyph_spinner(&format!("think-{id}"), t.primary, cx) } else { icon(IconName::Brain, 14., t.muted_foreground).into_any_element() })
                .child(if streaming {
                    // 字号要在外层设(ShimmerText 自己不带),否则会用默认的大字号,和「已思考」(13px)对不上
                    div().text_size(px(13.)).text_color(t.muted_foreground)
                        .child(ShimmerText::new(crate::appearance::status_line(crate::appearance::verb_text(cx, id), session.read(cx).turn_started)).id(SharedString::from(format!("think-shimmer-{id}"))))
                        .into_any_element()
                } else {
                    div().text_size(px(13.)).text_color(t.muted_foreground).child(thought_label(session.read(cx).thinking_secs(id))).into_any_element()
                })
                .child(icon(if expanded { IconName::ChevronDown } else { IconName::ChevronRight }, 12., t.muted_foreground)),
        )
        .when(expanded || streaming && text.len() < 400, |d| {
            d.child(div().pl(px(20.)).text_size(px(13.)).text_color(t.muted_foreground).child(text.to_string()))
        })
        .into_any_element()
}

#[allow(dead_code)]
fn _keep(_: &Hsla) {
    let _ = pill("", gpui_kit::black(), gpui_kit::black());
}

#[cfg(test)]
mod tests {
    use super::thought_label;

    #[::core::prelude::v1::test]
    fn 思考时长文案() {
        assert_eq!(thought_label(None), "已思考");
        assert_eq!(thought_label(Some(0)), "已思考");
        assert_eq!(thought_label(Some(1)), "已思考 1 秒");
        assert_eq!(thought_label(Some(75)), "已思考 1 分 15 秒");
    }
}
