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
use crate::row_probe::RowProbe;
use std::rc::Rc;

/// 视口外额外渲染的高度,滚动更顺滑。
const OVERDRAW: Pixels = px(400.);
/// 未渲染过的行的估算高度。
///
/// gpui 列表把「从没测量过、也没有高度提示」的行按 0 高度计:长会话回放后只有底部一屏被测量,
/// 上面全是 0 高度,用户往上滚进这片区域时「滚动位置 → 行」的换算会塌缩(一次滚得比已测量部分多,
/// 就直接落到很靠前的行),表现为滚着滚着突然跳走。给每行一个估值,换算才连续;实际渲染后会被真实高度替换。
const EST_ROW_H: Pixels = px(120.);
/// 调试:环境变量 `LOOM_SCROLL_TRACE` 开启后,每次滚动事件把列表位置写进日志。
fn scroll_trace() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("LOOM_SCROLL_TRACE").is_ok())
}

/// 距离底部不超过这个像素数就算「在底部」。
pub const BOTTOM_SLOP: f32 = 24.;

/// 滚动状态:包一层 `ListState`,提供与原 `MessageScrollerState` 同名的操作。
pub struct ChatScroll {
    list: ListState,
    /// 每帧各行实际画出的位置(吸附层据此判断)。
    probe: Rc<RowProbe>,
}

impl ChatScroll {
    /// 创建状态,开启尾部跟随。参数:`item_count` 初始行数。
    pub fn new(item_count: usize, _cx: &mut Context<Self>) -> Self {
        let list = ListState::new(item_count, ListAlignment::Top, OVERDRAW).measure_all().with_uniform_item_height(EST_ROW_H);
        list.set_follow_mode(FollowMode::Tail);
        Self { list, probe: Rc::new(RowProbe::default()) }
    }

    /// 是否离开了底部(用滚动位置判断,不依赖行高是否全部测量)。
    pub fn is_away_from_bottom(&self) -> bool {
        self.distance_from_bottom() > BOTTOM_SLOP
    }

    /// 视口底边离内容底部的距离(像素,内容不足一屏时为 0)。
    pub fn distance_from_bottom(&self) -> f32 {
        let max = f32::from(self.list.max_offset_for_scrollbar().y);
        if max <= 0. {
            return 0.;
        }
        let top = -f32::from(self.list.scroll_px_offset_for_scrollbar().y);
        (max - top).max(0.)
    }

    /// 这一帧画出来的最大行下标(没画过返回 None)。
    pub fn max_painted(&self) -> Option<usize> {
        self.probe.max_ix()
    }

    /// 指定行的卡片顶边是否已经滚过视口顶边(真正「到顶」才算,未到顶保持原状)。
    ///
    /// 画出来的行直接比较顶边(回弹位移对行与视口一视同仁,互相抵消);没画出来的行,
    /// 在已画范围之前就是滚到了上面。
    pub fn row_scrolled_past(&self, row: usize) -> bool {
        match self.probe.top_of(row) {
            // 行内上方有语义间距,卡片顶边 = 行顶 + 间距(这里只用于用户消息行)
            Some(top) => top + crate::flow_style::user_row_inset(row) < f32::from(self.list.viewport_bounds().top()),
            None => self.probe.is_before_painted(row),
        }
    }

    /// 指定行卡片顶边相对视口顶边的 Y(这一帧没画出来返回 None)。
    pub fn row_top(&self, row: usize) -> Option<f32> {
        Some(self.probe.top_of(row)? + crate::flow_style::user_row_inset(row) - f32::from(self.list.viewport_bounds().top()))
    }

    /// 会话流视口高度(像素,吸附层据此限制悬浮卡的最大高度)。
    pub fn viewport_height(&self) -> f32 {
        f32::from(self.list.viewport_bounds().size.height)
    }

    /// 重置为 `item_count` 行并恢复跟随。
    pub fn reset(&mut self, item_count: usize, cx: &mut Context<Self>) {
        if scroll_trace() { loom_diag::info!("列表 reset n={}", item_count); }
        self.list.reset_with_uniform_height(item_count, EST_ROW_H);
        self.list.set_follow_mode(FollowMode::Tail);
        cx.notify();
    }

    /// 把 `range` 这段行替换成 `count` 个新行(用于尾部变化)。
    /// 末行带额外底部留白,「是不是末行」变了的相邻行要重新测量。
    pub fn splice(&mut self, range: Range<usize>, count: usize, cx: &mut Context<Self>) {
        let n = self.list.item_count();
        if scroll_trace() { loom_diag::info!("列表 splice {:?} -> {} (现有 {})", range, count, n); }
        if range.start > range.end || range.end > n {
            return;
        }
        let start = range.start;
        self.list.splice(range, count);
        // 批量新增的行(历史会话分批载入)补上估高:`splice` 出来的行没有高度提示、按 0 高度计,
        // 往上滚进未渲染区时位置换算塌缩,一下跨过十几行(实测第 43 行上方累计高度只有 254px)。
        // 已有高度的行保留原值作提示,只给没有提示的补估值。
        if count >= 2 {
            let _ = self.list.clone().with_uniform_item_height(EST_ROW_H);
        }
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
    let (list, away, probe) = {
        let s = state.read(cx);
        (s.list.clone(), s.is_away_from_bottom(), s.probe.clone())
    };
    let list_el = list_el(list.clone(), move |ix, w, c| {
        let probe = probe.clone();
        // 零尺寸占位画布:记录这一行实际画出的顶边
        div().relative().w_full().min_w_0().px_3()
            .child(canvas(move |b, _, _| { probe.record(ix, f32::from(b.top())); if scroll_trace() { if let Some(old) = probe.note_height(ix, f32::from(b.size.height)) { loom_diag::info!("行高变化 ix={} {:.1} -> {:.1}", ix, old, f32::from(b.size.height)); } } }, |_, _, _, _| {}).absolute().size_full())
            .child(render_row(ix, w, c)).into_any_element()
    });
    let weak = state.downgrade();
    let probe = state.read(cx).probe.clone();
    let probe_frame = probe.clone();
    div().id(id).relative().size_full().min_h_0().overflow_hidden()
        // 帧起点:先于列表绘制,清掉上一帧的行位置记录
        .child(canvas(move |_, _, _| probe_frame.begin_frame(), |_, _, _, _| {}).absolute().size_full())
        .child(
            div().size_full().min_h_0().min_w_0()
                .child(gpui_kit::base::ScrollBounce::new(SharedString::from("timeline-bounce"), &list, list_el).enabled(false).on_scroll(move |_, cx| {
                    let _ = weak.update(cx, |s, cx| {
                        if scroll_trace() {
                            let t = s.list.logical_scroll_top();
                            loom_diag::info!("滚动追踪 item={} off={:.1} bar_y={:.1} max={:.1} following={}", t.item_ix, f32::from(t.offset_in_item), f32::from(s.list.scroll_px_offset_for_scrollbar().y), f32::from(s.list.max_offset_for_scrollbar().y), s.list.is_following_tail());
                        }
                        cx.notify()
                    });
                }))
                .vertical_scrollbar(&list),
        )
        // 一次测完(`measure_all`)发生在列表预绘制里;绘制阶段取走统计记日志,看首帧耗时
        .child({
            let stats = list.clone();
            canvas(|_, _, _| {}, move |_, _, _, _| {
                if let Some((rows, took)) = stats.take_measure_all_stats() {
                    loom_diag::info!("会话流一次测完 rows={} 耗时={:.1}ms", rows, took.as_secs_f64() * 1000.);
                }
            }).absolute().size_full()
        })
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

/// 一次滚轮手势结束后,是否要把会话流吸回底部。
///
/// 只处理「本来贴着底部、被触控板误触轻轻拉开」的情况:手势开始时就不在底部附近(用户在读历史),
/// 或者位移已经不算轻微,都尊重用户,不动。
///
/// 参数:`started_near_bottom` 手势开始时是否贴底;`net` 手势净位移的绝对值(像素);
/// `away` 结束时是否已离开底部。
pub fn should_snap_to_end(started_near_bottom: bool, net: f32, away: bool) -> bool {
    started_near_bottom && away && net < SNAP_BACK_MAX_NET
}

/// 手势净位移小于这个值才算「误触」。
const SNAP_BACK_MAX_NET: f32 = 40.;

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 贴底处轻微晃动才吸回() {
        assert!(should_snap_to_end(true, 12., true));
        // 已经在底部,不用吸
        assert!(!should_snap_to_end(true, 12., false));
        // 位移较大是有意滚动
        assert!(!should_snap_to_end(true, 80., true));
    }

    #[::core::prelude::v1::test]
    fn 读历史时的小幅滚动不被拽回底部() {
        assert!(!should_snap_to_end(false, 12., true));
    }
}
