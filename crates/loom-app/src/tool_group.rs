//! 连续工具调用的折叠摘要行:图标 + 「运行了 2 条命令、读取了 3 个文件」+ 失败提示 + 展开箭头。
//!
//! 默认:全部成功结束的折叠;还有没结束的或有失败的默认展开。手动点击会反转默认状态。
//!
//! @author ixxxxoooo

use crate::model::SessionModel;
use crate::row_groups::{needs_attention, summary};
use crate::tool_card::render_tool;
use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
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
    let text = summary(&tools);
    let s = session.clone();
    let k = key.clone();
    let header = h_flex()
        .id(SharedString::from(format!("tgrp-{}", first.id)))
        .h(px(26.)).px_2().gap_2().items_center().rounded(px(6.)).cursor_pointer().self_start()
        .hover(|d| d.bg(t.list_hover))
        .on_click(move |_, _, cx| { s.update(cx, |m, cx| m.toggle(&k, cx)); })
        .child(if running { Spinner::new().xsmall().into_any_element() } else { icon(IconName::Layers, 14., t.muted_foreground).into_any_element() })
        .child(div().text_size(px(13.)).text_color(t.muted_foreground).child(text))
        .when(failed > 0, |d| d.child(div().text_size(px(12.)).text_color(t.danger).child(format!("{failed} 项失败"))))
        .child(icon(if expanded { IconName::ChevronDown } else { IconName::ChevronRight }, 12., t.muted_foreground));
    v_flex().w_full()
        .child(header)
        .when(expanded, |d| d.child(v_flex().w_full().pl(px(14.)).children(tools.iter().map(|tool| render_tool(session, tool, cx)))))
        .into_any_element()
}
