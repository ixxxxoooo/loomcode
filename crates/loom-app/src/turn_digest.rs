//! 回合折叠行(参考 OhMyAgent / Cursor 的「Worked for …」):回合结束后,整段过程
//! (思考、工具调用、中间叙述)折成一行「已工作 12分8秒」,点一下在原位向下展开,再点一下收起。
//!
//! @author ixxxxoooo

use crate::model::SessionModel;
use crate::row_groups::{Row, build_rows};
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::*;
use std::ops::Range;

/// 折叠状态用的 key(与工具组的 `grp:` 区分)。
pub fn digest_key(first_id: &str) -> String {
    format!("dg:{first_id}")
}

/// 用时文案:不到 1 分钟「8秒」,不到 1 小时「12分8秒」,否则「1小时2分」。
///
/// 参数:`secs` 用时(秒)。
pub fn work_text(secs: u64) -> String {
    match secs {
        0..=59 => format!("{}秒", secs.max(1)),
        60..=3599 => format!("{}分{}秒", secs / 60, secs % 60),
        _ => format!("{}小时{}分", secs / 3600, secs % 3600 / 60),
    }
}

/// 渲染一个回合折叠行。
///
/// 参数:`session` 会话;`range` 被折叠的条目范围。
pub fn render_digest(session: &Entity<SessionModel>, range: Range<usize>, window: &mut Window, cx: &mut App) -> AnyElement {
    let (items, secs, density) = {
        let m = session.read(cx);
        let items: Vec<TimelineItem> = m.state.items[range.clone()].to_vec();
        // 用时挂在这个回合的用户消息上:往前找最近的用户消息
        let secs = m.state.items[..range.start].iter().rev().find_map(|i| if let TimelineItem::User { id, .. } = i { m.state.durations.get(id).copied() } else { None });
        (items, secs.unwrap_or(0), cx.try_global::<crate::UiPrefs>().map_or_else(Default::default, |p| p.tool_density))
    };
    let Some(first_id) = items.first().map(|i| i.id().to_string()) else { return div().into_any_element() };
    let key = digest_key(&first_id);
    let expanded = session.read(cx).is_expanded(&key);
    let failed = items.iter().filter(|i| matches!(i, TimelineItem::Tool(t) if t.status == ToolStatus::Failed)).count();
    let (muted, fg, danger, hover) = { let t = cx.theme(); (t.muted_foreground, t.foreground, t.danger, t.list_hover) };
    let tool_count = items.iter().filter(|i| matches!(i, TimelineItem::Tool(_))).count();
    let s = session.clone();
    let k = key.clone();
    let header = h_flex().id(SharedString::from(format!("digest-{first_id}"))).group(SharedString::from(format!("dg-g-{first_id}"))).self_start().h(px(crate::flow_style::ROW_H)).px_2().gap_1p5().items_center().rounded(px(6.)).cursor_pointer().hover(|d| d.bg(hover))
        .on_click(move |_, _, cx| { s.update(cx, |m, cx| m.toggle(&k, cx)); })
        .child(div().text_size(px(13.)).text_color(if expanded { fg } else { muted }).child(format!("已工作 {}", work_text(secs))))
        .when(tool_count > 0, |d| d.child(div().text_size(px(11.5)).text_color(muted.opacity(0.75)).child(format!("{tool_count} 次工具"))))
        .when(failed > 0, |d| d.child(div().text_size(px(12.)).text_color(danger).child(format!("{failed} 项失败"))))
        .child(session.read(cx).motion.chevron(&key, expanded, 12., muted));
    v_flex().w_full()
        .child(header)
        .when(expanded, |d| {
            // 展开后按所选详细程度排列这段过程
            let sub = build_rows(&items, false, density, &Default::default());
            let body: Vec<AnyElement> = sub.into_iter().map(|row| match row {
                Row::Item(i) => crate::timeline::render_item(session, &items[i], 0, window, cx),
                Row::Tools(r) => crate::tool_group::render_group(session, range.start + r.start..range.start + r.end, cx),
                Row::Agents(r) => crate::agent_group::render_group(session, range.start + r.start..range.start + r.end, window, cx),
                Row::Digest(_) | Row::Working => div().into_any_element(),
            }).collect();
            d.child(v_flex().w_full().pl(px(14.)).gap_1().children(body))
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 用时文案() {
        assert_eq!(work_text(0), "1秒");
        assert_eq!(work_text(8), "8秒");
        assert_eq!(work_text(728), "12分8秒");
        assert_eq!(work_text(3720), "1小时2分");
    }
}
