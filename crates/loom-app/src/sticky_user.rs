//! 会话流顶部吸附的用户消息(参考 OhMyAgent 的 StickyUserOverlay):
//! 当前回合的用户消息滚出顶部后,以两行卡片固定在会话流顶部,点击回到原位。
//!
//! @author ixxxxoooo

use crate::chat_scroll::ChatScroll;
use crate::model::SessionModel;
use crate::row_groups::Row;
use crate::user_message::display_text;
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;
use loom_core::TimelineItem;

/// 找出应吸附的用户消息。
///
/// 参数:`rows` 行表;`items` 时间线条目;`top` 视口顶部的行下标;`offset` 顶行已滚出的像素数。
/// 返回:`(行下标, 正文)`;顶部行本身就是用户消息且几乎未滚出时返回 None。
pub fn find(rows: &[Row], items: &[TimelineItem], top: usize, offset: f32) -> Option<(usize, String)> {
    let start = top.min(rows.len().checked_sub(1)?);
    for ix in (0..=start).rev() {
        if let Row::Item(i) = rows[ix]
            && let Some(TimelineItem::User { text, .. }) = items.get(i)
        {
            if ix == start && offset < 6. {
                return None;
            }
            let text = display_text(text);
            return if text.is_empty() { None } else { Some((ix, text)) };
        }
    }
    None
}

/// 渲染吸附卡(绝对定位在会话流顶部)。
///
/// 参数:`session` 会话;`scroller` 滚动状态;返回:无需吸附时 None。
pub fn render(session: &Entity<SessionModel>, scroller: &Entity<ChatScroll>, cx: &App) -> Option<AnyElement> {
    let (top, offset) = scroller.read(cx).top();
    let found = {
        let m = session.read(cx);
        find(&m.rows, &m.state.items, top, offset)
    };
    let (row, text) = found?;
    let t = cx.theme();
    let sc = scroller.clone();
    Some(
        div().absolute().top_0().left_0().right_0().px_3().pt(px(6.)).flex().justify_center()
            .child(
                div().id("sticky-user").w_full().max_w(px(crate::timeline::content_max_w(cx))).px(px(14.)).py(px(8.)).rounded(px(12.))
                    .bg(t.muted).border_1().border_color(t.border).shadow_sm().cursor_pointer()
                    .text_size(px(13.)).text_color(t.foreground).line_clamp(2)
                    .on_click(move |_, _, cx| { sc.update(cx, |s, cx| s.scroll_to_row(row, cx)); })
                    .child(text),
            )
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: &str, text: &str) -> TimelineItem {
        TimelineItem::User { id: id.into(), text: text.into(), images: vec![], files: vec![] }
    }

    #[::core::prelude::v1::test]
    fn 用户消息滚出后吸附() {
        let items = vec![user("a", "问题一"), TimelineItem::Notice { id: "n".into(), level: loom_core::NoticeLevel::Info, text: "x".into() }];
        let rows = vec![Row::Item(0), Row::Item(1)];
        assert_eq!(find(&rows, &items, 1, 0.), Some((0, "问题一".into())));
        assert_eq!(find(&rows, &items, 0, 2.), None);
        assert_eq!(find(&rows, &items, 0, 30.), Some((0, "问题一".into())));
    }
}
