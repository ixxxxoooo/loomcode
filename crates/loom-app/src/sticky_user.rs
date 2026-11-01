//! 会话流顶部吸附的用户消息(对标 OhMyAgent `StickyUser`):
//! 当前回合的用户消息滚出顶部后固定在会话流顶部;下一条用户消息上来时把它顶出;
//! 悬停出现重试 / 复制 / 编辑,点击后编辑卡悬浮在顶部向下展开(不推挤会话流)。
//!
//! @author ixxxxoooo

use crate::chat_scroll::ChatScroll;
use crate::model::SessionModel;
use crate::row_groups::Row;
use crate::user_message::{action_btn, display_text};
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::TimelineItem;

/// 吸附卡与顶边、与下一条消息之间的间距。
const INSET: f32 = 8.;
/// 渐隐高度。
const FADE_H: f32 = 28.;

/// 吸附目标:行下标、消息 id、正文、其后下一条用户消息的行下标。
#[derive(Debug, PartialEq, Clone)]
pub struct Target {
    pub row: usize,
    pub id: String,
    pub text: String,
    pub next: Option<usize>,
}

/// 某行是不是用户消息(返回条目)。
fn user_at<'a>(rows: &[Row], items: &'a [TimelineItem], ix: usize) -> Option<&'a TimelineItem> {
    match rows.get(ix)? {
        Row::Item(i) => items.get(*i).filter(|t| matches!(t, TimelineItem::User { .. })),
        _ => None,
    }
}

/// 解析吸附目标:从视口顶行往前,找最近的一条「卡片顶边已滚过视口顶边」的用户消息。
///
/// 参数:`rows` 行表;`items` 条目;`top` 视口顶部的行下标;`past` 某行卡片顶边是否已滚过视口顶边。
/// 返回:没有消息到顶(含还没到顶的情况)时 None,保持原状。
pub fn resolve(rows: &[Row], items: &[TimelineItem], top: usize, past: impl Fn(usize) -> bool) -> Option<Target> {
    let start = top.min(rows.len().checked_sub(1)?);
    for ix in (0..=start).rev() {
        let Some(TimelineItem::User { id, text, .. }) = user_at(rows, items, ix) else { continue };
        // 顶行本身是还没到顶的用户消息:继续往前找(前一条正在被它顶出)
        if !past(ix) {
            continue;
        }
        let text = display_text(text);
        if text.is_empty() {
            return None;
        }
        let next = (ix + 1..rows.len()).find(|&n| user_at(rows, items, n).is_some());
        return Some(Target { row: ix, id: id.clone(), text, next });
    }
    None
}

/// 顶出位移(≤ 0):下一条用户消息的顶边逼近吸附卡底边时,吸附卡整体上移。
///
/// 参数:`next_top` 下一条消息顶边相对视口的 Y,未渲染为 None;`card_h` 吸附卡高度。
pub fn push_offset(next_top: Option<f32>, card_h: f32) -> f32 {
    next_top.map_or(0., |y| (y - INSET - card_h - INSET).min(0.))
}

/// 估算吸附卡高度:短消息单行,长消息两行。
fn card_height(text: &str) -> f32 {
    if text.contains('\n') || text.chars().count() > 60 { 60. } else { 40. }
}

/// 正在编辑的用户消息所在的行下标。
fn editing_row(session: &Entity<SessionModel>, cx: &App) -> Option<usize> {
    let m = session.read(cx);
    let eid = &m.editing.as_ref()?.id;
    let item = m.state.items.iter().position(|i| matches!(i, TimelineItem::User { id, .. } if id == eid))?;
    m.rows.iter().position(|r| matches!(r, Row::Item(i) if *i == item))
}

/// 渲染吸附层(绝对定位在会话流顶部);无需吸附且没有悬浮编辑时返回 None。
///
/// 参数:`session` 会话;`scroller` 滚动状态。
pub fn render(session: &Entity<SessionModel>, scroller: &Entity<ChatScroll>, cx: &App) -> Option<AnyElement> {
    let t = cx.theme();
    let max_w = px(crate::timeline::content_max_w(cx));
    let bg = t.background;
    let backdrop = |h: f32| {
        div().absolute().top_0().left_0().right_0().h(px(h))
            .bg(linear_gradient(0., linear_color_stop(bg.opacity(0.), 0.), linear_color_stop(bg, 1.)))
    };
    let sc = scroller.read(cx);
    // 悬浮编辑:编辑卡盖在消息原来的位置上向下展开(吸附时盖在顶部),不推挤会话流
    if let Some(editor) = crate::user_message::render_pinned_editor(session, cx) {
        let y = editing_row(session, cx).and_then(|r| sc.row_top(r)).unwrap_or(INSET).max(INSET);
        return Some(
            div().absolute().top(px(y)).left_0().right_0().px_3()
                .child(div().w_full().px_1().flex().justify_center().child(div().w_full().max_w(max_w).child(editor)))
                .into_any_element(),
        );
    }
    // 回弹位移期间冻结在上一次稳定的结果上;没有历史结果就不显示
    let held = if sc.displaced() { sc.sticky_hold.borrow().clone() } else { None };
    let (target, push) = match held {
        Some(h) => h,
        None if sc.displaced() => return None,
        None => {
            let top = sc.top().0;
            let found = {
                let m = session.read(cx);
                resolve(&m.rows, &m.state.items, top, |r| sc.row_scrolled_past(r))
            };
            let Some(target) = found else {
                *sc.sticky_hold.borrow_mut() = None;
                return None;
            };
            let push = push_offset(target.next.and_then(|n| sc.row_top(n)), card_height(&target.text));
            *sc.sticky_hold.borrow_mut() = Some((target.clone(), push));
            (target, push)
        }
    };
    let Target { id, text, .. } = target;
    let edit = {
        let (s, id, text) = (session.clone(), id.clone(), text.clone());
        move |window: &mut Window, cx: &mut App| { s.update(cx, |m, cx| m.begin_edit(&id, &text, true, window, cx)); }
    };
    let (edit_click, edit_btn) = (edit.clone(), edit);
    let (s_retry, id_retry, copy_text) = (session.clone(), id.clone(), text.clone());
    let (copied, retried) = {
        let c = session.read(cx).copied.as_deref();
        (c == Some(format!("c-{id}").as_str()), c == Some(format!("r-{id}").as_str()))
    };
    let (s_cm, id_cm, s_rm, id_rm) = (session.clone(), id.clone(), session.clone(), id.clone());
    Some(
        div().absolute().top(px(push)).left_0().right_0()
            // 卡片范围内与背景同色,下缘再渐隐,内容从卡片下面平滑淡入
            .child(backdrop(card_height(&text) + INSET + FADE_H))
            .child(
                div().w_full().px_3().pt(px(INSET)).flex().justify_center().child(
                    div().id("sticky-user").group("sticky-user").relative().w_full().max_w(max_w)
                        .px(px(14.)).py(px(8.)).pr(px(96.)).rounded(px(12.))
                        .bg(t.muted).cursor_pointer().text_size(px(14.)).text_color(t.foreground).line_clamp(2)
                        .on_click(move |_, window, cx| edit_click(window, cx))
                        .child(text)
                        .child(
                            h_flex().absolute().right(px(8.)).top(px(7.)).gap_0p5().when(!(copied || retried), |d| d.invisible().group_hover("sticky-user", |d| d.visible()))
                                .child(action_btn("sticky-retry".into(), IconName::RotateCcw, "重试(原样重新发送)", retried, cx)
                                    .on_click(move |_, _, cx| {
                                        cx.stop_propagation();
                                        s_rm.update(cx, |m, cx| m.mark_copied(&format!("r-{id_rm}"), cx));
                                        s_retry.update(cx, |m, cx| m.retry_user(&id_retry, cx));
                                    }))
                                .child(action_btn("sticky-copy".into(), IconName::Copy, "复制消息", copied, cx)
                                    .on_click(move |_, _, cx| {
                                        cx.stop_propagation();
                                        cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone()));
                                        s_cm.update(cx, |m, cx| m.mark_copied(&format!("c-{id_cm}"), cx));
                                    }))
                                .child(action_btn("sticky-edit".into(), IconName::SquarePen, "编辑并重新发送", false, cx)
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
    fn note() -> TimelineItem {
        TimelineItem::Notice { id: "n".into(), level: loom_core::NoticeLevel::Info, text: "x".into() }
    }

    #[::core::prelude::v1::test]
    fn 到顶才吸附() {
        let items = vec![user("a", "问题一"), note()];
        let rows = vec![Row::Item(0), Row::Item(1)];
        let t = resolve(&rows, &items, 1, |_| true).unwrap();
        assert_eq!((t.row, t.id.as_str(), t.text.as_str(), t.next), (0, "a", "问题一", None));
        assert_eq!(resolve(&rows, &items, 1, |_| false), None);
    }

    #[::core::prelude::v1::test]
    fn 下一条用户消息进入时前一条继续吸附() {
        let items = vec![user("a", "一"), note(), user("b", "二")];
        let rows = vec![Row::Item(0), Row::Item(1), Row::Item(2)];
        // 顶行是第二条用户消息但还没到顶:仍吸附第一条,并知道下一条是第 2 行
        let t = resolve(&rows, &items, 2, |r| r == 0).unwrap();
        assert_eq!((t.row, t.next), (0, Some(2)));
        // 第二条到顶后接管
        let t = resolve(&rows, &items, 2, |_| true).unwrap();
        assert_eq!((t.row, t.id.as_str()), (2, "b"));
    }

    #[::core::prelude::v1::test]
    fn 顶出位移() {
        assert_eq!(push_offset(None, 40.), 0.);
        assert_eq!(push_offset(Some(200.), 40.), 0.);
        assert_eq!(push_offset(Some(30.), 40.), -26.);
    }
}
