//! 会话流顶部吸附的用户消息(参考 OhMyAgent 的 StickyUserOverlay):
//! 当前回合的用户消息滚出顶部后,以两行卡片固定在会话流顶部,点击回到原位。
//!
//! @author ixxxxoooo

use crate::chat_scroll::ChatScroll;
use crate::model::SessionModel;
use crate::row_groups::Row;
use crate::user_message::display_text;
use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex};
use gpui_kit::*;
use loom_core::TimelineItem;

/// 找出应吸附的用户消息。
///
/// 参数:`rows` 行表;`items` 时间线条目;`top` 视口顶部的行下标;`past` 判断某行卡片顶边是否已滚过视口顶边。
/// 返回:`(行下标, 消息 id, 正文)`;最近的用户消息还没滚到顶时返回 None(保持原状)。
pub fn find(rows: &[Row], items: &[TimelineItem], top: usize, past: impl Fn(usize) -> bool) -> Option<(usize, String, String)> {
    let start = top.min(rows.len().checked_sub(1)?);
    for ix in (0..=start).rev() {
        if let Row::Item(i) = rows[ix]
            && let Some(TimelineItem::User { id, text, .. }) = items.get(i)
        {
            if !past(ix) {
                return None;
            }
            let text = display_text(text);
            return if text.is_empty() { None } else { Some((ix, id.clone(), text)) };
        }
    }
    None
}

/// 渲染吸附卡(绝对定位在会话流顶部):悬停显示重试 / 复制 / 编辑,点击向下展开并进入编辑;下沿有渐隐。
///
/// 参数:`session` 会话;`scroller` 滚动状态;返回:无需吸附时 None。
pub fn render(session: &Entity<SessionModel>, scroller: &Entity<ChatScroll>, cx: &App) -> Option<AnyElement> {
    let sc = scroller.read(cx);
    let top = sc.top().0;
    let found = {
        let m = session.read(cx);
        find(&m.rows, &m.state.items, top, |r| sc.row_scrolled_past(r))
    };
    let (row, id, text) = found?;
    let t = cx.theme();
    let edit = {
        let (s, sc, id, text) = (session.clone(), scroller.clone(), id.clone(), text.clone());
        move |window: &mut Window, cx: &mut App| {
            // 先进入编辑再滚到这条消息顶部:编辑框从原位置向下展开,吸附卡随之消失
            s.update(cx, |m, cx| m.begin_edit(&id, &text, window, cx));
            sc.update(cx, |s, cx| s.scroll_to_row(row, cx));
        }
    };
    let (edit_click, edit_btn) = (edit.clone(), edit);
    let (s_retry, id_retry, copy_text) = (session.clone(), id.clone(), text.clone());
    let btn = |bid: &'static str, ic: IconName, tip: &'static str| {
        div().id(bid).size(px(24.)).flex().items_center().justify_center().rounded(px(6.)).cursor_pointer()
            .hover(|d| d.bg(t.foreground.opacity(0.08)))
            .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tip).build(window, cx))
            .child(icon(ic, 12., t.muted_foreground))
    };
    Some(
        div().absolute().top_0().left_0().right_0().flex().flex_col().items_center()
            // 下沿渐隐:卡片底下再铺一段背景色到透明的过渡
            .child(
                div().absolute().top_0().left_0().right_0().h(px(58.))
                    .bg(linear_gradient(0., linear_color_stop(t.background.opacity(0.), 0.), linear_color_stop(t.background, 1.))),
            )
            .child(
                div().w_full().px_3().pt(px(5.)).flex().justify_center().child(
                    div().id("sticky-user").group("sticky-user").relative().w_full().max_w(px(crate::timeline::content_max_w(cx)))
                        .px(px(14.)).py(px(8.)).pr(px(96.)).rounded(px(12.))
                        .bg(t.muted).cursor_pointer().text_size(px(14.)).text_color(t.foreground).line_clamp(2)
                        .on_click(move |_, window, cx| edit_click(window, cx))
                        .child(text)
                        .child(
                            h_flex().absolute().right(px(8.)).top(px(6.)).gap_0p5().invisible().group_hover("sticky-user", |d| d.visible())
                                .child(btn("sticky-retry", IconName::RotateCcw, "重试(原样重新发送)")
                                    .on_click(move |_, _, cx| { cx.stop_propagation(); s_retry.update(cx, |m, cx| m.retry_user(&id_retry, cx)); }))
                                .child(btn("sticky-copy", IconName::Copy, "复制消息")
                                    .on_click(move |_, _, cx| { cx.stop_propagation(); cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone())); }))
                                .child(btn("sticky-edit", IconName::SquarePen, "编辑并重新发送")
                                    .on_click(move |_, window, cx| { cx.stop_propagation(); edit_btn(window, cx); })),
                        ),
                ),
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
    fn 到顶才吸附() {
        let items = vec![user("a", "问题一"), TimelineItem::Notice { id: "n".into(), level: loom_core::NoticeLevel::Info, text: "x".into() }];
        let rows = vec![Row::Item(0), Row::Item(1)];
        assert_eq!(find(&rows, &items, 1, |_| true), Some((0, "a".into(), "问题一".into())));
        assert_eq!(find(&rows, &items, 1, |_| false), None);
    }
}
