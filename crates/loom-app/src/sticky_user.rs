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
/// 悬浮编辑卡里正文区之外的固定高度(内边距、按钮行、提示行、边框)。
const EDITOR_CHROME: f32 = 100.;
/// 悬浮编辑卡正文区的最小高度。
const EDITOR_MIN_BODY: f32 = 60.;

/// 悬浮编辑卡正文区的最大高度:视口剩余高度扣掉上下间距与卡片自身的固定部分。
///
/// 参数:`viewport_h` 会话流视口高度;`top` 卡片顶边相对视口的 Y。
pub fn editor_body_max(viewport_h: f32, top: f32) -> f32 {
    (viewport_h - top - INSET * 2. - EDITOR_CHROME).max(EDITOR_MIN_BODY)
}

/// 吸附目标:行下标、消息 id、正文、其后下一条用户消息的行下标。
#[derive(Debug, PartialEq)]
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

/// 吸附卡正文行高与最多行数:吸附卡始终很矮(最多两行),长消息只显示开头。
const LINE_H: f32 = 20.;
const MAX_LINES: usize = 2;

/// 吸附卡里显示的正文:换行折成空格,让两行都用来显示内容。
fn flatten(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 估算吸附卡高度:短消息单行,长消息两行(上下内边距各 10)。
fn card_height(text: &str) -> f32 {
    let lines = if text.contains('\n') || text.chars().count() > 60 { MAX_LINES } else { 1 };
    LINE_H * lines as f32 + 20.
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
    let y = editing_row(session, cx).and_then(|r| sc.row_top(r)).unwrap_or(INSET).max(INSET);
    if let Some(editor) = crate::user_message::render_pinned_editor(session, editor_body_max(sc.viewport_height(), y), cx) {
        return Some(
            div().absolute().top(px(y)).left_0().right_0().px_3()
                .child(div().w_full().px_1().flex().justify_center().child(div().w_full().max_w(max_w).child(editor)))
                .into_any_element(),
        );
    }
    let top = sc.max_painted().unwrap_or(0);
    let target = {
        let m = session.read(cx);
        resolve(&m.rows, &m.state.items, top, |r| sc.row_scrolled_past(r))
    }?;
    let push = push_offset(target.next.and_then(|n| sc.row_top(n)), card_height(&target.text));
    let Target { id, text, .. } = target;
    let edit = {
        let (s, id, text) = (session.clone(), id.clone(), text.clone());
        move |window: &mut Window, cx: &mut App| { s.update(cx, |m, cx| m.begin_edit(&id, &text, true, window, cx)); }
    };
    let edit_click = edit;
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
                        .px(px(14.)).py(px(10.)).min_h(px(40.)).pr(px(60.)).rounded(px(12.)).border_1().border_color(crate::user_message::card_border(cx))
                        // 文本被限制在框内:超长内容截断,不外溢
                        .min_w_0().overflow_hidden()
                        .bg(crate::user_message::card_bg(cx)).cursor_pointer().text_size(px(14.)).line_height(px(LINE_H)).text_color(t.foreground).line_clamp(MAX_LINES)
                        .on_click(move |_, window, cx| edit_click(window, cx))
                        .child(flatten(&text))
                        .child(
                            h_flex().absolute().right(px(8.)).bottom(px(8.)).gap_0p5().when(!(copied || retried), |d| d.invisible().group_hover("sticky-user", |d| d.visible()))
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
    fn 编辑卡正文最大高度() {
        assert_eq!(editor_body_max(600., 8.), 600. - 8. - 16. - EDITOR_CHROME);
        // 视口很矮时保底,不会变成 0 或负数
        assert_eq!(editor_body_max(100., 8.), EDITOR_MIN_BODY);
    }

    #[::core::prelude::v1::test]
    fn 吸附卡高度与正文压平() {
        assert_eq!(card_height("短"), 40.);
        assert_eq!(card_height("a\nb"), 60.);
        assert_eq!(card_height(&"长".repeat(61)), 60.);
        assert_eq!(flatten("a  b\n\n c"), "a b c");
    }

    #[::core::prelude::v1::test]
    fn 顶出位移() {
        assert_eq!(push_offset(None, 40.), 0.);
        assert_eq!(push_offset(Some(200.), 40.), 0.);
        assert_eq!(push_offset(Some(30.), 40.), -26.);
    }
}
