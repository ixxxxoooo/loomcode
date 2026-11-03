//! 会话时间线:把条目分发到各自的渲染函数。

use crate::cards;
use crate::model::SessionModel;
use crate::tool_card;
use crate::ui::{icon, pill};
use gpui_kit::assets::IconName;
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
    let (row, count, gap) = {
        let m = session.read(cx);
        let kind = |r: &crate::row_groups::Row| crate::flow_style::kind_of(r, &m.state.items);
        let gap = m.rows.get(ix).map_or(0., |r| crate::flow_style::gap_before(ix.checked_sub(1).and_then(|p| m.rows.get(p)).map(kind), kind(r)));
        (m.rows.get(ix).cloned(), m.rows.len(), gap)
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
        // 语义间距:步骤之间紧凑、正文与步骤之间适中、新的用户消息前留大间距(见 flow_style)
        .pt(px(if ix == 0 { crate::flow_style::FIRST_TOP } else { gap }))
        // 末行底部留白(不能用列表内边距,见 workbench 里的说明)
        .when(ix + 1 == count, |d| d.pb(px(25.)))
        .child({
            // 实时回合里刚出现的行淡入(见 motion)
            let m = session.read(cx);
            let key = m.rows.get(ix).map(|r| crate::motion::row_key(r, &m.state.items)).unwrap_or_default();
            m.motion.appear(&key, div().w_full().max_w(px(content_max_w(cx))).child(body))
        })
        .into_any_element()
}

/// 回合进行中、暂时没有内容在流出时的「处理中」提示行。
fn render_working(session: &Entity<SessionModel>, cx: &App) -> AnyElement {
    let (seed, started) = (session.entity_id().as_u64(), session.read(cx).turn_started);
    // 有进行中的待办时显示它的 activeForm(对标 Claude Code 的状态行),否则随机动词
    let verb = crate::todo_view::current_active(&session.read(cx).state.items).map(|a| format!("{a}…")).unwrap_or_else(|| crate::appearance::verb_text(cx, &seed.to_string()));
    let t = cx.theme();
    // 并行工具里有请求在等用户时,不再假装「处理中」
    if session.read(cx).state.awaiting_user() {
        return h_flex().h(px(26.)).px_2().gap_2().items_center().text_size(px(13.)).text_color(t.warning)
            .child(crate::ui::icon(gpui_kit::assets::IconName::CirclePause, 12., t.warning))
            .child("等待你的批准")
            .into_any_element();
    }
    h_flex().h(px(26.)).px_2().gap_2().items_center()
        .child(crate::appearance::glyph_spinner("working", t.primary, cx))
        .child(crate::shimmer_text::shimmer_label(crate::appearance::status_line(verb, started), "working-shimmer", cx))
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
        TimelineItem::Todos { id, items } => crate::todo_view::render_line(session, id, items, cx),
        TimelineItem::Subagent(card) => crate::subagent_card::render(session, card, depth, window, cx),
        TimelineItem::Permission(req) => crate::ask_cards::render_permission(req, cx),
        TimelineItem::Question(req) => crate::ask_cards::render_question(req, cx),
        TimelineItem::PlanApproval(plan) => crate::plan_card::render_card(session, plan, cx),
        TimelineItem::Notice { id, level, text } => cards::render_notice(session, id, *level, text, cx),
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
                        .child(crate::shimmer_text::shimmer_label(crate::appearance::status_line(crate::appearance::verb_text(cx, id), session.read(cx).turn_started), format!("think-shimmer-{id}"), cx))
                        .into_any_element()
                } else {
                    div().text_size(px(13.)).text_color(t.muted_foreground).child(thought_label(session.read(cx).thinking_secs(id))).into_any_element()
                })
                .child(session.read(cx).motion.chevron(id, expanded, 12., t.muted_foreground)),
        )
        .when(expanded && !streaming, |d| {
            d.child(div().pl(px(20.)).text_size(px(13.)).text_color(t.muted_foreground).child(gpui_base::SelectableText::new(SharedString::from(format!("think-sel-{id}")), text.to_string())))
        })
        // 流式中:固定高度的尾部窗口贴底跟随、顶部渐隐(对标 Cursor),不随字数忽现忽隐
        .when(streaming && !text.trim().is_empty(), |d| d.child(thinking_tail(id, text, cx)))
        .into_any_element()
}

/// 思考尾部窗口的高度(约 3 行)。
const THINK_TAIL_H: f32 = 60.;

/// 流式思考的尾部窗口:内容不足一窗时按实际高度,超出后限高、贴底滚动,顶部叠一层渐隐。
///
/// 参数:`id` 思考条目 id;`text` 当前已流出的思考文本。
fn thinking_tail(id: &str, text: &str, cx: &App) -> AnyElement {
    let t = cx.theme();
    let sid = SharedString::from(format!("think-tail-{id}"));
    let handle = crate::code_box::handle_for(&sid);
    handle.scroll_to_bottom();
    div().relative().pl(px(20.)).w_full()
        .child(div().id(sid).w_full().max_h(px(THINK_TAIL_H)).overflow_y_scroll().track_scroll(&handle)
            .text_size(px(13.)).line_height(px(20.)).text_color(t.muted_foreground.opacity(0.85)).child(gpui_base::SelectableText::new(SharedString::from(format!("think-tail-sel-{id}")), text.to_string())))
        // 估算超出一窗(约 3 行)才加渐隐,短内容不遮第一行
        .when(text.lines().count() > 3 || text.chars().count() > 150, |d| d.child(div().absolute().top_0().left(px(20.)).right_0().h(px(18.))
            .bg(linear_gradient(180., linear_color_stop(t.background, 0.), linear_color_stop(t.background.opacity(0.), 1.)))))
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
