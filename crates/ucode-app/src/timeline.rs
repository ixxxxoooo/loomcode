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
use ucode_core::*;

/// 时间线内容的最大宽度(居中)。
pub const CONTENT_MAX_W: f32 = 820.;

/// 虚拟列表的一行:加上居中容器与垂直间距。
pub fn render_row(session: &Entity<SessionModel>, ix: usize, window: &mut Window, cx: &mut App) -> AnyElement {
    let item = session.read(cx).state.items.get(ix).cloned();
    let Some(item) = item else { return div().into_any_element() };
    div()
        .w_full()
        .flex()
        .justify_center()
        .px_1()
        .py(px(5.))
        .child(div().w_full().max_w(px(CONTENT_MAX_W)).child(render_item(session, &item, 0, window, cx)))
        .into_any_element()
}

/// 渲染一个条目(可嵌套在子 Agent 里,`depth` 为嵌套层级)。
pub fn render_item(session: &Entity<SessionModel>, item: &TimelineItem, depth: usize, window: &mut Window, cx: &mut App) -> AnyElement {
    match item {
        TimelineItem::User { id, text, images, files } => render_user(session, id, text, images, files, cx),
        TimelineItem::Assistant { id, text, .. } => crate::rich_text::render_assistant(id, text, cx),
        TimelineItem::Thinking { id, text, streaming } => render_thinking(session, id, text, *streaming, cx),
        TimelineItem::Tool(tool) => tool_card::render_tool(session, tool, cx),
        TimelineItem::Todos { id, items } => cards::render_todos(id, items, cx),
        TimelineItem::Subagent(card) => cards::render_subagent(session, card, depth, window, cx),
        TimelineItem::Permission(req) => cards::render_permission(session, req, cx),
        TimelineItem::Question(req) => cards::render_question(session, req, cx),
        TimelineItem::PlanApproval(plan) => cards::render_plan(session, plan, cx),
        TimelineItem::Notice { level, text, .. } => cards::render_notice(*level, text, cx),
        TimelineItem::Canvas { id, title } => cards::render_canvas(session, id, title, cx),
        TimelineItem::Changes { files, .. } => cards::render_changes(session, files, cx),
    }
}

/// 用户消息:卡片底色,图片/文件 chip,长文折叠。
fn render_user(session: &Entity<SessionModel>, id: &str, text: &str, images: &[String], files: &[String], cx: &App) -> AnyElement {
    let t = cx.theme();
    let lines = text.lines().count();
    let long = lines > 6;
    let expanded = session.read(cx).is_expanded(id);
    let shown: String = if long && !expanded { text.lines().take(6).collect::<Vec<_>>().join("\n") } else { text.to_string() };
    let s = session.clone();
    let sid = id.to_string();
    v_flex()
        .w_full()
        .gap_1p5()
        .p_3()
        .rounded(px(10.))
        .bg(t.muted)
        .when(!images.is_empty() || !files.is_empty(), |d| {
            d.child(
                h_flex().flex_wrap().gap_1p5()
                    .children(images.iter().map(|name| {
                        h_flex().gap_1().items_center().px_2().h(px(22.)).rounded(px(6.)).bg(t.background).border_1().border_color(t.border)
                            .child(icon(IconName::Image, 12., t.muted_foreground))
                            .child(div().text_size(px(12.)).child(name.clone()))
                    }))
                    .children(files.iter().map(|name| {
                        h_flex().gap_1().items_center().px_2().h(px(22.)).rounded(px(6.)).bg(t.background).border_1().border_color(t.border)
                            .child(icon(IconName::FileText, 12., t.muted_foreground))
                            .child(div().text_size(px(12.)).child(name.clone()))
                    })),
            )
        })
        .when(!shown.is_empty(), |d| d.child(div().text_size(px(14.)).child(shown.clone())))
        .when(long, |d| {
            d.child(
                div().id(SharedString::from(format!("user-fold-{id}"))).text_size(px(12.)).text_color(t.primary).cursor_pointer()
                    .on_click(move |_, _, cx| { s.update(cx, |m, cx| m.toggle(&sid, cx)); })
                    .child(if expanded { "收起".to_string() } else { format!("展开全部({lines} 行)") }),
            )
        })
        .into_any_element()
}

/// 思考:折叠行,流式时显示流光「思考中…」。
fn render_thinking(session: &Entity<SessionModel>, id: &str, text: &str, streaming: bool, cx: &App) -> AnyElement {
    let t = cx.theme();
    let expanded = session.read(cx).is_expanded(id);
    let s = session.clone();
    let sid = id.to_string();
    v_flex().w_full().gap_1()
        .child(
            h_flex().id(SharedString::from(format!("think-{id}"))).gap_1p5().items_center().cursor_pointer().h(px(24.))
                .on_click(move |_, _, cx| { s.update(cx, |m, cx| m.toggle(&sid, cx)); })
                .child(icon(IconName::Brain, 14., t.muted_foreground))
                .child(if streaming {
                    ShimmerText::new("思考中…").id(SharedString::from(format!("think-shimmer-{id}"))).into_any_element()
                } else {
                    div().text_size(px(13.)).text_color(t.muted_foreground).child("已思考").into_any_element()
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
