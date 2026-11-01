//! 会话流的虚拟滚动列表。
//!
//! 基于 gpui 的 `ListState`,自己判断「是否在底部」:库自带的 `MessageScroller` 在行高未全部测量时
//! (长会话回放后总是如此)会把「已在底部」误判成「向上滚动了」,导致一直显示跳转按钮和底部渐隐。
//! 这里改用「滚动位置 vs 最大偏移」判断,和列表布局用的是同一套估算高度,不会自相矛盾。
//!
//! @author ixxxxoooo

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::ops::Range;

/// 视口外额外渲染的高度,滚动更顺滑。
const OVERDRAW: Pixels = px(400.);
/// 距离底部不超过这个像素数就算「在底部」。
const BOTTOM_SLOP: f32 = 24.;

/// 滚动状态:包一层 `ListState`,提供与原 `MessageScrollerState` 同名的操作。
pub struct ChatScroll {
    list: ListState,
}

impl ChatScroll {
    /// 创建状态,开启尾部跟随。参数:`item_count` 初始行数。
    pub fn new(item_count: usize, _cx: &mut Context<Self>) -> Self {
        let list = ListState::new(item_count, ListAlignment::Top, OVERDRAW);
        list.set_follow_mode(FollowMode::Tail);
        Self { list }
    }

    /// 是否离开了底部(用滚动位置判断,不依赖行高是否全部测量)。
    pub fn is_away_from_bottom(&self) -> bool {
        let max = f32::from(self.list.max_offset_for_scrollbar().y);
        if max <= 0. {
            return false;
        }
        let top = -f32::from(self.list.scroll_px_offset_for_scrollbar().y);
        max - top > BOTTOM_SLOP
    }

    /// 视口顶部的行下标与该行已滚出的像素数。
    pub fn top(&self) -> (usize, f32) {
        let o = self.list.logical_scroll_top();
        (o.item_ix, f32::from(o.offset_in_item))
    }

    /// 指定行的卡片顶边是否已经滚过视口顶边(真正「到顶」才算,未到顶保持原状)。
    pub fn row_scrolled_past(&self, row: usize) -> bool {
        match self.list.bounds_for_item(row) {
            // 行内上方有 5px 间距,卡片顶边 = 行顶 + 5
            Some(b) => b.top() + px(5.) < self.list.viewport_bounds().top(),
            None => self.list.logical_scroll_top().item_ix > row,
        }
    }

    /// 滚到指定行顶部(暂停贴底跟随)。
    pub fn scroll_to_row(&mut self, row: usize, cx: &mut Context<Self>) {
        self.list.scroll_to(ListOffset { item_ix: row, offset_in_item: px(0.) });
        cx.notify();
    }

    /// 重置为 `item_count` 行并恢复跟随。
    pub fn reset(&mut self, item_count: usize, cx: &mut Context<Self>) {
        self.list.reset(item_count);
        self.list.set_follow_mode(FollowMode::Tail);
        cx.notify();
    }

    /// 把 `range` 这段行替换成 `count` 个新行(用于尾部变化)。
    /// 末行带额外底部留白,「是不是末行」变了的相邻行要重新测量。
    pub fn splice(&mut self, range: Range<usize>, count: usize, cx: &mut Context<Self>) {
        let n = self.list.item_count();
        if range.start > range.end || range.end > n {
            return;
        }
        let start = range.start;
        self.list.splice(range, count);
        let total = self.list.item_count();
        if start > 0 && start <= total {
            self.list.remeasure_items(start - 1..start);
        }
        if total > 0 {
            self.list.remeasure_items(total - 1..total);
        }
        cx.notify();
    }

    /// 重新测量一段行(内容变了)。范围越界返回 false。
    pub fn remeasure_items(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> bool {
        if range.start > range.end || range.end > self.list.item_count() {
            return false;
        }
        self.list.remeasure_items(range);
        cx.notify();
        true
    }

    /// 全部重新测量。
    pub fn remeasure(&mut self, cx: &mut Context<Self>) {
        self.list.remeasure();
        cx.notify();
    }

    /// 暂停贴底跟随:用户点开 / 收起某一行时调用。
    ///
    /// 贴底跟随时底边是钉住的,行变高会把内容往上顶(看起来「向上展开」);暂停后行从原位置往下长,
    /// 再点一次就在原地收起。用户滚回底部后列表会自动恢复跟随。
    pub fn pause_following(&mut self) {
        self.list.pause_following_tail();
    }

    /// 滚到最新一行并恢复跟随。
    pub fn scroll_to_end(&mut self, cx: &mut Context<Self>) {
        self.list.set_follow_mode(FollowMode::Tail);
        self.list.scroll_to_end();
        cx.notify();
    }
}

/// 渲染会话流:列表 + 滚动条 + 「跳到最新」按钮(仅离开底部时显示)。
///
/// 参数:`id` 元素 id;`state` 滚动状态;`row_gap_px` 无;`render_row` 行渲染函数。
pub fn chat_scroller(
    id: &'static str,
    state: Entity<ChatScroll>,
    mut render_row: impl FnMut(usize, &mut Window, &mut App) -> AnyElement + 'static,
    sticky: Option<AnyElement>,
    cx: &App,
) -> impl IntoElement {
    let t = cx.theme().clone();
    let (list, away) = {
        let s = state.read(cx);
        (s.list.clone(), s.is_away_from_bottom())
    };
    let list_el = list_el(list.clone(), move |ix, w, c| {
        div().w_full().min_w_0().px_3().child(render_row(ix, w, c)).into_any_element()
    });
    let weak = state.downgrade();
    div().id(id).relative().size_full().min_h_0().overflow_hidden()
        .child(
            div().size_full().min_h_0().min_w_0()
                .child(gpui_kit::base::ScrollBounce::new(SharedString::from("timeline-bounce"), &list, list_el).on_scroll(move |_, cx| {
                    let _ = weak.update(cx, |_, cx| cx.notify());
                }))
                .vertical_scrollbar(&list),
        )
        // 底部(输入框上方)渐隐过渡
        .child(
            div().absolute().left_0().right_0().bottom_0().h(px(28.))
                .bg(linear_gradient(180., linear_color_stop(t.background.opacity(0.), 0.), linear_color_stop(t.background, 1.))),
        )
        .children(sticky)
        .when(away, |d| {
            d.child(
                div().absolute().left_0().right_0().bottom(px(12.)).flex().justify_center().child(
                    Button::new("timeline-jump").secondary().icon(IconName::ArrowDown).tooltip("跳到最新")
                        .rounded(t.radius_full()).border_1().border_color(t.border).bg(t.background).text_color(t.foreground)
                        .shadow_md()
                        .on_click(move |_, _, cx| { state.update(cx, |s, cx| s.scroll_to_end(cx)); }),
                ),
            )
        })
}

/// 构造 gpui 列表元素(撑满父容器)。
fn list_el(list: ListState, render: impl FnMut(usize, &mut Window, &mut App) -> AnyElement + 'static) -> impl IntoElement {
    gpui_kit::list(list, render).size_full().min_h_0()
}
