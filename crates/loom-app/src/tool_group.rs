//! 连续工具调用的折叠摘要行:图标 + 「运行了 2 条命令、读取了 3 个文件」+ 失败提示 + 展开箭头。
//!
//! 默认折叠(对齐 Cursor / OhMyAgent);有失败的默认展开。手动点击会反转默认状态。
//! 折叠且运行中时,标题行下方固定一行「最新一步」:新步骤从下方滑入、旧步骤向上顶出。
//!
//! @author ixxxxoooo

use crate::model::SessionModel;
use crate::row_groups::{needs_attention, summary};
use crate::tool_card::render_tool;
use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::shimmer::ShimmerText;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::ops::Range;
use loom_core::*;

/// 折叠状态用的 key(和单个工具自己的展开状态区分开)。
pub fn group_key(first_id: &str) -> String {
    format!("grp:{first_id}")
}

/// 渲染一段连续工具调用。
///
/// 参数:`session` 会话;`range` 条目下标范围。
pub fn render_group(session: &Entity<SessionModel>, range: Range<usize>, cx: &App) -> AnyElement {
    let t = cx.theme();
    let model = session.read(cx);
    let tools: Vec<&ToolCall> = model.state.items[range.clone()].iter().filter_map(|i| if let TimelineItem::Tool(t) = i { Some(t) } else { None }).collect();
    let Some(first) = tools.first() else { return div().into_any_element() };
    let key = group_key(&first.id);
    // 手动点击反转默认状态
    let expanded = model.is_expanded(&key) != needs_attention(&tools);
    let failed = tools.iter().filter(|t| t.status == ToolStatus::Failed).count();
    let running = tools.iter().any(|t| !t.status.is_finished());
    // 工具之间的空窗(上一步刚结束、下一步未开始):回合还在进行且分组是时间线末尾,也保留这一行
    let live_tail = model.turn_started.is_some() && range.end == model.state.items.len();
    let text = summary(&tools);
    let s = session.clone();
    let k = key.clone();
    let header = h_flex()
        .id(SharedString::from(format!("tgrp-{}", first.id)))
        .h(px(crate::flow_style::ROW_H)).px_2().gap_2().items_center().rounded(px(6.)).cursor_pointer().self_start()
        .hover(|d| d.bg(t.list_hover))
        .on_click(move |_, _, cx| { s.update(cx, |m, cx| m.toggle(&k, cx)); })
        .child(if running { crate::appearance::row_spinner(&format!("tgrp-sp-{}", first.id), cx) } else { icon(IconName::Layers, 14., t.muted_foreground).into_any_element() })
        .child(div().text_size(px(13.)).text_color(t.muted_foreground).child(text))
        .when(failed > 0, |d| d.child(div().text_size(px(12.)).text_color(t.danger).child(format!("{failed} 项失败"))))
        .child(model.motion.chevron(&key, expanded, 12., t.muted_foreground));
    v_flex().w_full()
        .child(header)
        .when(!expanded && (running || live_tail), |d| d.child(latest_step(session, &tools, &key, running, cx)))
        .when(expanded, |d| d.child(v_flex().w_full().pl(px(14.)).children(tools.iter().map(|tool| render_tool(session, tool, cx)))))
        .into_any_element()
}

/// 「最新一步」行高(容器与每一行一致,滑动距离恰为一整行)。
const STEP_H: f32 = 22.;

/// 折叠分组下方的「最新一步」行:只显示最后一个(并行时取仍在运行的最后一个)工具;
/// 换步时新行从底部滑入、上一步同时向上顶出(200ms 缓出),点击等同于点击分组标题。
fn latest_step(session: &Entity<SessionModel>, tools: &[&ToolCall], key: &str, running: bool, cx: &App) -> AnyElement {
    let ix = tools.iter().rposition(|t| !t.status.is_finished()).unwrap_or(tools.len() - 1);
    let cur = tools[ix];
    let prev = ix.checked_sub(1).map(|i| tools[i]);
    let (s, k) = (session.clone(), key.to_string());
    let anim = || Animation::new(std::time::Duration::from_millis(200)).with_easing(ease_out_quint());
    let mut host = div().id(SharedString::from(format!("tstep-{key}")))
        .relative().h(px(STEP_H)).ml(px(28.)).overflow_hidden().cursor_pointer()
        .on_click(move |_, _, cx| { s.update(cx, |m, cx| m.toggle(&k, cx)); });
    if let Some(prev) = prev {
        host = host.child(
            step_row(prev, false, cx).absolute().left_0().top(px(0.))
                .with_animation(SharedString::from(format!("tstep-out-{}", cur.id)), anim(), |d, e| d.top(px(-STEP_H * e))),
        );
    }
    host.child(
        step_row(cur, running && !cur.status.is_finished(), cx).absolute().left_0().top(px(0.))
            .with_animation(SharedString::from(format!("tstep-in-{}", cur.id)), anim(), |d, e| d.top(px(STEP_H * (1. - e)))),
    ).into_any_element()
}

/// 一行步骤:图标 + 动词 + 对象(命令取首行,绝对路径只留文件名);运行中带流光。
fn step_row(tool: &ToolCall, live: bool, cx: &App) -> Div {
    let t = cx.theme();
    let p = crate::tool_present::present(tool);
    let mut subject = p.subject.lines().next().unwrap_or("").to_string();
    if subject.starts_with('/') && !subject.contains(' ') {
        subject = subject.rsplit('/').next().unwrap_or(&subject).to_string();
    }
    let text = match (p.verb.is_empty(), subject.is_empty()) {
        (_, true) => p.verb.clone(),
        (true, false) => subject,
        (false, false) => format!("{} {subject}", p.verb),
    };
    let label = if live {
        ShimmerText::new(text).id(SharedString::from(format!("tstep-shim-{}", tool.id))).into_any_element()
    } else {
        div().child(text).into_any_element()
    };
    h_flex().h(px(STEP_H)).gap(px(6.)).items_center().max_w_full()
        .child(icon(crate::tool_card::tool_icon(tool.kind), 11., t.muted_foreground))
        .child(div().min_w_0().truncate().text_size(px(12.)).text_color(t.muted_foreground).child(label))
}
