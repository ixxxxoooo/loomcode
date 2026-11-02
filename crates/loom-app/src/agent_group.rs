//! 并行子 Agent 的汇总行:连续派发的多个子 Agent 合成一组,
//! 头部一行显示「并行 7 个子 Agent · 2 运行中 · 3 已完成 · 1 失败」,展开后才看到各张卡片。
//!
//! 默认:还有在运行的或有失败的就展开;全部成功结束则折叠。手动点击反转默认状态。
//!
//! @author ixxxxoooo

use crate::model::SessionModel;
use crate::row_groups::agents_tally;
use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::ops::Range;
use loom_core::*;

/// 折叠状态用的 key。
pub fn group_key(first_id: &str) -> String {
    format!("ag:{first_id}")
}

/// 渲染一组连续的子 Agent。
///
/// 参数:`session` 会话;`range` 条目下标范围。
pub fn render_group(session: &Entity<SessionModel>, range: Range<usize>, window: &mut Window, cx: &mut App) -> AnyElement {
    let cards: Vec<SubagentCard> = session.read(cx).state.items[range].iter().filter_map(|i| if let TimelineItem::Subagent(c) = i { Some(c.clone()) } else { None }).collect();
    let Some(first) = cards.first() else { return div().into_any_element() };
    let key = group_key(&first.id);
    let refs: Vec<&SubagentCard> = cards.iter().collect();
    let (running, done, failed, stopped) = agents_tally(&refs);
    let attention = running > 0 || failed > 0;
    let expanded = session.read(cx).is_expanded(&key) != attention;
    let (muted, fg, danger, success, warning) = { let t = cx.theme(); (t.muted_foreground, t.foreground, t.danger, t.success, t.warning) };
    let now = now_ms();
    let stalled = cards.iter().filter(|c| c.stalled(now)).count();
    let (s, k) = (session.clone(), key.clone());
    let chip = |n: usize, text: &str, color: Hsla| div().text_size(px(12.)).text_color(color).child(format!("{n} {text}"));
    let header = h_flex().id(SharedString::from(format!("agrp-{}", first.id))).self_start().h(px(crate::flow_style::ROW_H)).px_2().gap_2().items_center().rounded(px(6.)).cursor_pointer()
        .hover(|d| d.bg(cx.theme().list_hover))
        .on_click(move |_, _, cx| { s.update(cx, |m, cx| m.toggle(&k, cx)); })
        .child(if running > 0 { crate::appearance::row_spinner(&format!("agrp-sp-{}", first.id), cx) } else { icon(IconName::Bot, 14., muted).into_any_element() })
        .child(div().text_size(px(13.)).text_color(if expanded { fg } else { muted }).child(format!("并行 {} 个子 Agent", cards.len())))
        .when(running > 0, |d| d.child(chip(running, "运行中", cx.theme().primary)))
        .when(done > 0, |d| d.child(chip(done, "已完成", success)))
        .when(failed > 0, |d| d.child(chip(failed, "失败", danger)))
        .when(stopped > 0, |d| d.child(chip(stopped, "已停止", muted)))
        .when(stalled > 0, |d| d.child(chip(stalled, "疑似停滞", warning)))
        .child(session.read(cx).motion.chevron(&key, expanded, 12., muted));
    v_flex().w_full()
        .child(header)
        .when(expanded, |d| {
            let body: Vec<AnyElement> = cards.iter().map(|c| crate::subagent_card::render(session, c, 0, window, cx)).collect();
            d.child(v_flex().w_full().pl(px(14.)).gap_0p5().children(body))
        })
        .into_any_element()
}
