//! 鼠标滚轮平滑滚动。
//!
//! macOS 上触控板给的是像素级精确增量(系统自带惯性,本来就顺滑),而普通鼠标滚轮给的是「行」增量,
//! gpui 收到后一次性跳 N 行,看起来一卡一卡。这里在窗口最前面(capture 阶段)拦下按行的滚轮事件,
//! 把距离攒进目标,随后每帧按指数缓动派发一小段像素增量的滚轮事件。所有滚动区域(会话流、列表、
//! 编辑器、终端、设置页……)走的仍是各自原来的滚动逻辑,因此一处接入、全窗口生效。
//!
//! @author ixxxxoooo

use gpui_kit::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// 缓动时间常数:剩余距离每过这么久衰减到约 37%,约 4 倍时长基本停稳。
const TAU_MS: f32 = 55.;
/// 剩余距离小于这个像素数就一次走完,避免拖尾。
const SNAP_PX: f32 = 0.5;
/// 两帧间隔的上限,卡顿后的一帧不至于一下走完全部距离。
const MAX_FRAME: Duration = Duration::from_millis(50);

/// 单个窗口的缓动状态。
#[derive(Default)]
struct Glide {
    /// 还没派发出去的剩余距离(像素,x / y)。
    remaining: (f32, f32),
    /// 上一帧的时间;`None` 表示动画未在运行。
    last: Option<Instant>,
}

thread_local! {
    /// 每个窗口一份缓动状态(界面线程独占)。
    static GLIDES: RefCell<HashMap<WindowId, Glide>> = RefCell::new(HashMap::new());
}

/// 放在窗口根元素的**第一个**子元素位置:不占布局、不绘制,只注册 capture 阶段的滚轮拦截。
/// 必须先于其它子元素绘制,capture 阶段才会最先收到事件。
///
/// 返回:一个绝对定位、零尺寸的元素。
pub fn layer() -> impl IntoElement {
    canvas(|_, _, _| (), |_, _, window, _| {
        window.on_mouse_event(|ev: &ScrollWheelEvent, phase, window, cx| {
            if phase != DispatchPhase::Capture || cx.reduce_motion() {
                return;
            }
            let ScrollDelta::Lines(lines) = ev.delta else { return };
            cx.stop_propagation();
            let lh = f32::from(window.line_height());
            push(window, (lines.x * lh, lines.y * lh));
        });
    })
    .absolute()
    .size_0()
}

/// 把一次滚轮的距离加进目标;反向滚动时丢掉旧的剩余距离,立刻转向。
///
/// 参数:`window` 当前窗口;`delta` 新增距离(像素)。
fn push(window: &mut Window, delta: (f32, f32)) {
    let id = window.window_handle().window_id();
    let start = GLIDES.with_borrow_mut(|map| {
        let g = map.entry(id).or_default();
        g.remaining.0 = merge(g.remaining.0, delta.0);
        g.remaining.1 = merge(g.remaining.1, delta.1);
        let idle = g.last.is_none();
        if idle {
            g.last = Some(Instant::now());
        }
        idle
    });
    if start {
        window.on_next_frame(move |w, cx| tick(id, w, cx));
    }
}

/// 同向累加,反向则以新值为准。
///
/// 参数:`old` 旧的剩余距离;`add` 新增距离。返回:合并后的剩余距离。
fn merge(old: f32, add: f32) -> f32 {
    if old * add < 0. { add } else { old + add }
}

/// 计算这一帧该走的距离。
///
/// 参数:`remaining` 剩余距离;`dt` 距上一帧的时间。返回:本帧位移。
fn step(remaining: f32, dt: Duration) -> f32 {
    if remaining.abs() <= SNAP_PX {
        return remaining;
    }
    let k = 1. - (-(dt.min(MAX_FRAME).as_secs_f32() * 1000.) / TAU_MS).exp();
    remaining * k
}

/// 每帧推进一次:派发本帧位移的像素滚轮事件,没走完就预约下一帧。
///
/// 参数:`id` 窗口 id;`window`、`cx` 帧回调给的上下文。
fn tick(id: WindowId, window: &mut Window, cx: &mut App) {
    let now = Instant::now();
    let Some((dx, dy, more)) = GLIDES.with_borrow_mut(|map| {
        let g = map.get_mut(&id)?;
        let dt = now.saturating_duration_since(g.last?);
        let (dx, dy) = (step(g.remaining.0, dt), step(g.remaining.1, dt));
        g.remaining = (g.remaining.0 - dx, g.remaining.1 - dy);
        let more = g.remaining.0 != 0. || g.remaining.1 != 0.;
        g.last = more.then_some(now);
        Some((dx, dy, more))
    }) else {
        return;
    };
    if dx != 0. || dy != 0. {
        window.dispatch_event(
            PlatformInput::ScrollWheel(ScrollWheelEvent {
                position: window.mouse_position(),
                delta: ScrollDelta::Pixels(point(px(dx), px(dy))),
                touch_phase: TouchPhase::Moved,
                modifiers: window.modifiers(),
            }),
            cx,
        );
    }
    if more {
        window.on_next_frame(move |w, cx| tick(id, w, cx));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn merge_reverses_on_opposite_sign() {
        assert_eq!(merge(30., 20.), 50.);
        assert_eq!(merge(30., -20.), -20.);
        assert_eq!(merge(0., -20.), -20.);
    }

    #[::core::prelude::v1::test]
    fn step_converges_and_snaps() {
        let mut r = 60.;
        let mut total = 0.;
        for _ in 0..60 {
            let d = step(r, Duration::from_millis(16));
            r -= d;
            total += d;
        }
        assert_eq!(r, 0.);
        assert!((total - 60.).abs() < 1e-3);
        // 一帧不会把距离全部走完(有过渡)
        assert!(step(60., Duration::from_millis(16)) < 30.);
    }
}
