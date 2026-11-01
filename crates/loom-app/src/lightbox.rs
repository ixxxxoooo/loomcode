//! 图片预览灯箱:全屏遮罩,滚轮 / 按钮 / 键盘缩放(滚轮以鼠标为中心),按住拖动平移,Esc 或点空白处关闭。
//!
//! @author ixxxxoooo

use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::*;
use std::path::PathBuf;

/// 最小 / 最大缩放倍数(相对「适应窗口」)。
const MIN_ZOOM: f32 = 0.2;
const MAX_ZOOM: f32 = 12.;
/// 点击与拖动的区分阈值(像素)。
const DRAG_SLOP: f32 = 3.;

/// 当前打开的灯箱(全局,Workbench 渲染时挂在最上层)。
#[derive(Default)]
pub struct LightboxSlot(pub Option<Entity<Lightbox>>);
impl Global for LightboxSlot {}

/// 灯箱状态。
pub struct Lightbox {
    path: PathBuf,
    name: String,
    /// 图片原始像素尺寸
    natural: (f32, f32),
    zoom: f32,
    /// 相对窗口中心的平移
    off: (f32, f32),
    /// 拖动中:(按下时鼠标位置, 按下时平移, 是否已超过阈值)
    drag: Option<((f32, f32), (f32, f32), bool)>,
    focus: FocusHandle,
}

/// 当前灯箱,供 Workbench 渲染。
pub fn current(cx: &App) -> Option<Entity<Lightbox>> {
    cx.try_global::<LightboxSlot>().and_then(|s| s.0.clone())
}

/// 打开灯箱。参数:`path` 图片路径;`name` 标题。
pub fn open(path: &str, name: &str, window: &mut Window, cx: &mut App) {
    let p = PathBuf::from(path);
    let natural = crate::image_size::read(&p).map_or((1200., 900.), |(w, h)| (w.max(1) as f32, h.max(1) as f32));
    let name = name.to_string();
    let lb = cx.new(|cx| Lightbox { path: p, name, natural, zoom: 1., off: (0., 0.), drag: None, focus: cx.focus_handle() });
    let focus = lb.read(cx).focus.clone();
    window.focus(&focus, cx);
    cx.set_global(LightboxSlot(Some(lb)));
    cx.refresh_windows();
}

impl Lightbox {
    fn close(cx: &mut App) {
        cx.set_global(LightboxSlot(None));
        cx.refresh_windows();
    }

    /// 以窗口坐标 `at`(相对窗口中心)为中心缩放到 `zoom`。
    fn zoom_to(&mut self, zoom: f32, at: (f32, f32), cx: &mut Context<Self>) {
        let z = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        let r = z / self.zoom;
        self.off = (at.0 - (at.0 - self.off.0) * r, at.1 - (at.1 - self.off.1) * r);
        self.zoom = z;
        if (z - 1.).abs() < 0.01 { self.zoom = 1.; }
        cx.notify();
    }

    fn reset(&mut self, cx: &mut Context<Self>) {
        self.zoom = 1.;
        self.off = (0., 0.);
        cx.notify();
    }
}

impl Render for Lightbox {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let vp = window.viewport_size();
        let (vw, vh) = (f32::from(vp.width), f32::from(vp.height));
        let fit = (vw * 0.9 / self.natural.0).min(vh * 0.82 / self.natural.1).min(1.);
        let (w, h) = (self.natural.0 * fit * self.zoom, self.natural.1 * fit * self.zoom);
        let (left, top) = ((vw - w) / 2. + self.off.0, (vh - h) / 2. + self.off.1);
        let dragging = self.drag.is_some_and(|d| d.2);
        let center = (vw / 2., vh / 2.);
        let pill_btn = |id: &'static str, ic: IconName, tip: &'static str, f: fn(&mut Lightbox, &mut Context<Lightbox>)| {
            div().id(id).size(px(32.)).flex().items_center().justify_center().rounded(px(8.)).cursor_pointer()
                .hover(|d| d.bg(white().opacity(0.16))).active(|d| d.bg(white().opacity(0.28)))
                .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tip).build(window, cx))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, _, cx| { cx.stop_propagation(); f(this, cx); }))
                .child(icon(ic, 16., white()))
        };
        div().id("lightbox").absolute().top_0().left_0().size_full().bg(black().opacity(0.88)).occlude().track_focus(&self.focus)
            .on_key_down(cx.listener(move |this, ev: &KeyDownEvent, _, cx| match ev.keystroke.key.as_str() {
                "escape" => Self::close(cx),
                "=" | "+" => this.zoom_to(this.zoom * 1.25, (0., 0.), cx),
                "-" => this.zoom_to(this.zoom / 1.25, (0., 0.), cx),
                "0" => this.reset(cx),
                _ => {}
            }))
            .on_scroll_wheel(cx.listener(move |this, ev: &ScrollWheelEvent, _, cx| {
                let dy = f32::from(ev.delta.pixel_delta(px(20.)).y);
                let at = (f32::from(ev.position.x) - center.0, f32::from(ev.position.y) - center.1);
                this.zoom_to(this.zoom * (dy * 0.004).exp(), at, cx);
            }))
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                if let Some((start, off0, moved)) = this.drag {
                    let (dx, dy) = (f32::from(ev.position.x) - start.0, f32::from(ev.position.y) - start.1);
                    let moved = moved || dx.hypot(dy) > DRAG_SLOP;
                    this.drag = Some((start, off0, moved));
                    if moved { this.off = (off0.0 + dx, off0.1 + dy); cx.notify(); }
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| { this.drag = None; cx.notify(); }))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, cx| { this.drag = None; cx.notify(); }))
            // 点空白处关闭(图片自己拦截按下,拖动不会误关)
            .on_click(|_, _, cx| Self::close(cx))
            .child(
                div().id("lightbox-img").absolute().left(px(left)).top(px(top)).w(px(w)).h(px(h))
                    .cursor(if dragging { CursorStyle::ClosedHand } else { CursorStyle::OpenHand })
                    .on_mouse_down(MouseButton::Left, cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.drag = Some(((f32::from(ev.position.x), f32::from(ev.position.y)), this.off, false));
                    }))
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(img(self.path.clone()).size_full().object_fit(ObjectFit::Contain)),
            )
            // 顶部:文件名 + 关闭
            .child(
                div().absolute().top_0().left_0().right_0().h(px(48.)).px_4().flex().items_center().justify_between()
                    .child(div().text_size(px(13.)).text_color(white().opacity(0.85)).child(self.name.clone()))
                    .child(
                        div().id("lightbox-close").size(px(32.)).flex().items_center().justify_center().rounded(px(8.)).cursor_pointer()
                            .hover(|d| d.bg(white().opacity(0.16))).active(|d| d.bg(white().opacity(0.28)))
                            .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("关闭(Esc)").build(window, cx))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(|_, _, cx| { cx.stop_propagation(); Self::close(cx); })
                            .child(icon(IconName::X, 16., white())),
                    ),
            )
            // 底部工具条:缩小 / 比例 / 放大 / 适应
            .child(
                div().absolute().bottom(px(24.)).left_0().right_0().flex().justify_center().child(
                    div().flex().items_center().gap_1().px_2().py_1().rounded(px(12.)).bg(black().opacity(0.6)).border_1().border_color(white().opacity(0.12))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(pill_btn("lb-out", IconName::Minus, "缩小(-)", |s, cx| s.zoom_to(s.zoom / 1.25, (0., 0.), cx)))
                        .child(div().w(px(52.)).text_center().text_size(px(12.)).text_color(white().opacity(0.85)).child(format!("{:.0}%", self.zoom * fit * 100.)))
                        .child(pill_btn("lb-in", IconName::Plus, "放大(+)", |s, cx| s.zoom_to(s.zoom * 1.25, (0., 0.), cx)))
                        .child(pill_btn("lb-fit", IconName::Maximize, "适应窗口(0)", |s, cx| s.reset(cx))),
                ),
            )
    }
}
