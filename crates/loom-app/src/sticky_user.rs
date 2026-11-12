//! 会话流顶部吸附的用户消息(对标 OhMyAgent `StickyUser`):
//! 当前回合的用户消息滚出顶部后固定在会话流顶部;下一条用户消息上来时把它顶出;
//! 悬停出现重试 / 复制 / 编辑,点击后编辑卡悬浮在顶部向下展开(不推挤会话流)。
//!
//! @author ixxxxoooo

use crate::chat_scroll::ChatScroll;
use crate::model::SessionModel;
use crate::row_groups::Row;
use crate::user_message::display_text;
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;
use loom_core::TimelineItem;

/// 吸附卡与下一条消息之间的间距(悬浮编辑卡与顶边的间距也用它)。
const INSET: f32 = 8.;
/// 吸附卡贴着会话流顶边(像 CSS `position: sticky; top: 0`:滚到顶就停住,位置与大小都不变)。
const STICKY_TOP: f32 = 0.;
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
    pub images: Vec<String>,
    pub files: Vec<String>,
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
        let Some(TimelineItem::User { id, text, images, files }) = user_at(rows, items, ix) else { continue };
        // 顶行本身是还没到顶的用户消息:继续往前找(前一条正在被它顶出)
        if !past(ix) {
            continue;
        }
        let text = display_text(text);
        if text.is_empty() {
            return None;
        }
        let next = (ix + 1..rows.len()).find(|&n| user_at(rows, items, n).is_some());
        return Some(Target { row: ix, id: id.clone(), text, images: images.clone(), files: files.clone(), next });
    }
    None
}

/// 顶出位移(≤ 0):下一条用户消息的顶边逼近吸附卡底边时,吸附卡整体上移。
///
/// 参数:`next_top` 下一条消息顶边相对视口的 Y,未渲染为 None;`card_h` 吸附卡高度。
pub fn push_offset(next_top: Option<f32>, card_h: f32) -> f32 {
    next_top.map_or(0., |y| (y - INSET - card_h - STICKY_TOP).min(0.))
}

/// 吸附卡下缘渐隐带的高度:默认 [`FADE_H`];下一条用户消息已经靠近时,只铺到它的顶边为止。
///
/// 参数:`next_top` 下一条消息顶边相对视口的 Y,未渲染为 None;`card_bottom` 吸附卡底边相对视口的 Y。
pub fn fade_height(next_top: Option<f32>, card_bottom: f32) -> f32 {
    next_top.map_or(FADE_H, |y| (y - card_bottom).clamp(0., FADE_H))
}

/// 某条正在编辑的用户消息所在的行下标。
fn editing_row(session: &Entity<SessionModel>, eid: &str, cx: &App) -> Option<usize> {
    let m = session.read(cx);
    let item = m.state.items.iter().position(|i| matches!(i, TimelineItem::User { id, .. } if id == eid))?;
    m.rows.iter().position(|r| matches!(r, Row::Item(i) if *i == item))
}

/// 渲染吸附层(绝对定位在会话流顶部):吸顶卡 + 悬浮编辑卡。
///
/// 参数:`session` 会话;`scroller` 滚动状态。
pub fn render(session: &Entity<SessionModel>, scroller: &Entity<ChatScroll>, cx: &App) -> Option<AnyElement> {
    let max_w = px(crate::timeline::content_max_w(cx));
    let sc = scroller.read(cx);
    // 悬浮编辑:编辑卡盖在消息原来的位置上向下展开(滚出顶部时盖在顶部),不推挤会话流。
    // 可以同时有多张,各在各自消息的位置上,互不影响;后打开的盖在上面
    let pinned: Vec<AnyElement> = crate::user_message::pinned_edit_ids(session, cx).into_iter().filter_map(|eid| {
        let y = editing_row(session, &eid, cx).and_then(|r| sc.row_top(r)).unwrap_or(INSET).max(INSET);
        let editor = crate::user_message::render_pinned_editor(session, &eid, editor_body_max(sc.viewport_height(), y), cx)?;
        Some(
            div().absolute().top(px(y)).left_0().right_0().px_3()
                .child(div().w_full().px_1().flex().justify_center().child(div().w_full().max_w(max_w).child(editor)))
                .into_any_element(),
        )
    }).collect();
    // 吸顶卡照常显示:打开某条消息的编辑卡不影响顶部的吸顶卡(只有吸顶的正是这条被编辑的消息时,编辑卡已盖在顶部,才不画)
    Some(div().absolute().top_0().left_0().right_0().child(sticky_layer(session.clone(), scroller.clone())).children(pinned).into_any_element())
}

/// 吸顶卡所在的层:一块零高度的画布,卡片在**预绘制阶段**才决定画不画、画在哪。
///
/// 为什么不在渲染阶段算:行的位置(`RowProbe`)是列表预绘制时才记下的,渲染阶段读到的是上一帧的。
/// 滚动时每一帧都在动,按上一帧的位置摆吸顶卡就总慢一帧——卡片到顶的那一刻先被裁掉一截再弹回顶边,
/// 被下一条消息顶出时两张卡之间的缝隙忽大忽小,看起来就是「跳」。画布排在列表之后,
/// 它预绘制时本帧的行位置已经记好,在这里现场排版并定位,和列表同帧。
/// 卡片高度也在这里现场量(`layout_as_root` 的结果),不用再拿上一帧量到的高度去估。
fn sticky_layer(session: Entity<SessionModel>, scroller: Entity<ChatScroll>) -> AnyElement {
    canvas(
        move |bounds, window, cx| {
            let (target, next_top) = {
                let sc = scroller.read(cx);
                let m = session.read(cx);
                let target = resolve(&m.rows, &m.state.items, sc.max_painted().unwrap_or(0), |r| sc.row_scrolled_past(r))?;
                let next_top = target.next.and_then(|n| sc.row_top(n));
                (target, next_top)
            };
            // 吸顶的正是被编辑的那条:编辑卡已盖在顶部,不再画
            if crate::user_message::pinned_edit_ids(&session, cx).contains(&target.id) {
                return None;
            }
            let mut el = sticky_element(&session, &target, bounds.size.width, cx);
            let size = el.layout_as_root(gpui_kit::size(AvailableSpace::Definite(bounds.size.width), AvailableSpace::MinContent), window, cx);
            let h = f32::from(size.height);
            let push = push_offset(next_top, h);
            let top = STICKY_TOP + push;
            el.prepaint_at(point(bounds.left(), bounds.top() + px(top)), window, cx);
            // 卡片下缘的渐隐带:正文从卡片下面平滑淡入;下一条用户消息靠近时收窄到两张卡之间的缝隙,不盖住它的上边框
            let fade = fade_height(next_top, top + h);
            let band = Bounds::new(point(bounds.left(), bounds.top() + px(top + h)), gpui_kit::size(bounds.size.width, px(fade)));
            Some((el, band, cx.theme().background))
        },
        |_, state: Option<(AnyElement, Bounds<Pixels>, Hsla)>, window, cx| {
            if let Some((mut el, band, bg)) = state {
                el.paint(window, cx);
                if band.size.height > px(0.) {
                    window.paint_quad(fill(band, linear_gradient(180., linear_color_stop(bg, 0.), linear_color_stop(bg.opacity(0.), 1.))));
                }
            }
        },
    ).absolute().top_0().left_0().right_0().h(px(0.)).into_any_element()
}

/// 吸顶卡的元素:与会话流里的消息卡是同一个组件(同样的折叠 / 展开、悬停的回退 / 重试 / 复制、点击编辑),
/// 外层容器的写法也与会话流的行完全一致(`px_3` → `px_1` + 居中 → 限宽),所以宽度与左右位置分毫不差,
/// 吸顶前后只是「停住」,大小不变。
///
/// 参数:`target` 吸顶的消息;`width` 会话流视口宽度。
fn sticky_element(session: &Entity<SessionModel>, target: &Target, width: Pixels, cx: &App) -> AnyElement {
    let bg = cx.theme().background;
    let max_w = px(crate::timeline::content_max_w(cx));
    let card = crate::user_message::render_card(session, &target.id, &target.text, &target.images, &target.files, cx);
    div().relative().w(width)
        // 卡片范围内是不透明的背景色(把滚过去的原卡完全盖住,吸顶后不能再看到它)
        .child(div().absolute().top_0().left_0().right_0().bottom_0().bg(bg))
        .child(div().relative().w_full().px_3().child(div().w_full().px_1().flex().justify_center().child(div().w_full().max_w(max_w).child(card))))
        .into_any_element()
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
    fn 渐隐带不盖住下一条消息() {
        // 下一条还远(或没渲染):完整的渐隐带
        assert_eq!(fade_height(None, 40.), FADE_H);
        assert_eq!(fade_height(Some(300.), 40.), FADE_H);
        // 下一条顶上来:只剩两张卡之间的缝隙
        assert_eq!(fade_height(Some(48.), 40.), INSET);
        assert_eq!(fade_height(Some(30.), 40.), 0.);
    }

    #[::core::prelude::v1::test]
    fn 顶出位移() {
        assert_eq!(push_offset(None, 40.), 0.);
        assert_eq!(push_offset(Some(200.), 40.), 0.);
        assert_eq!(push_offset(Some(30.), 40.), -18.);
    }
}
