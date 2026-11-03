//! 图表全屏查看器(Mermaid、折线 / K 线图):铺满窗口,支持缩放与平移。
//!
//! 交互按图表 / 地图类查看器的通行做法:
//! - **缩放**:鼠标滚轮以光标为中心缩放;触控板捏合缩放;触控板双指滑动 = 平移,按住 ⌘ / ⌃ 再滑 = 缩放;
//!   `+` `-` 键与工具条按钮以窗口中心缩放;
//! - **平移**:按住左键拖动,方向键(⇧ 加大步长);平移范围受限,图不会被拖丢;
//! - **适应窗口**:打开时默认适应(`0` 键 / 工具条按钮回到适应,窗口缩放时自动重新适应),`1` 键回到 100%,
//!   双击在「适应」与「100%」之间切换;
//! - **关闭**:Esc 或右上角按钮(整个窗口都是画布,点空白处不关闭,免得拖动时误关)。
//!
//! Mermaid 是矢量场景,缩放时按倍数重新排布与绘制(线宽、字号一起放大),放大不糊。
//! 数学部分(适应、以光标为中心缩放、范围限制)在 `pan_zoom.rs`,有单元测试。
//!
//! @author ixxxxoooo

use crate::pan_zoom::PanZoom;
use crate::rich_text::ChartSpec;
use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_mermaid::{RenderError, Scene};
use std::rc::Rc;

/// 适应窗口时四周留白(像素)与放大上限(小图不要被放得过大)。
const FIT_MARGIN: f32 = 72.;
const FIT_MAX: f32 = 2.;
/// 图表的固有尺寸(像素,100% 时的大小)。
const CHART_SIZE: (f32, f32) = (900., 480.);
/// 方向键平移步长。
const ARROW_STEP: f32 = 48.;

/// 当前打开的查看器:属于哪个窗口 + 实体(Workbench / IDE 窗口渲染时各自取属于自己的)。
#[derive(Default)]
pub struct ViewerSlot(Option<(WindowId, Entity<DiagramViewer>)>);
impl Global for ViewerSlot {}

enum Content {
    Mermaid { src: String, scene: Rc<Result<Scene, RenderError>> },
    Chart(ChartSpec),
}

pub struct DiagramViewer {
    title: String,
    content: Content,
    /// 100% 时的内容尺寸
    natural: (f32, f32),
    pz: PanZoom,
    /// 处于「适应窗口」:窗口大小变化时自动重新适应,用户一缩放 / 平移就脱离
    fitted: bool,
    /// 拖动中:(按下时鼠标位置, 按下时的偏移)
    drag: Option<((f32, f32), (f32, f32))>,
    focus: FocusHandle,
}

/// 当前窗口里的查看器(没有或属于别的窗口返回 `None`)。
pub fn current(window: &Window, cx: &App) -> Option<Entity<DiagramViewer>> {
    cx.try_global::<ViewerSlot>().and_then(|s| s.0.as_ref()).filter(|(id, _)| *id == window.window_handle().window_id()).map(|(_, v)| v.clone())
}

/// 全屏查看一张 Mermaid 图(无法排版的不打开)。
pub fn open_mermaid(title: &str, src: &str, window: &mut Window, cx: &mut App) {
    let scene = crate::mermaid_view::scene_of(src);
    let natural = match scene.as_ref() {
        Ok(s) => (s.width.max(1.), s.height.max(1.)),
        Err(_) => return,
    };
    open(title.to_string(), Content::Mermaid { src: src.to_string(), scene }, natural, window, cx);
}

/// 全屏查看一张图表。
pub fn open_chart(spec: &ChartSpec, window: &mut Window, cx: &mut App) {
    let title = if spec.title().is_empty() { "图表".to_string() } else { spec.title().to_string() };
    open(title, Content::Chart(spec.clone()), CHART_SIZE, window, cx);
}

fn open(title: String, content: Content, natural: (f32, f32), window: &mut Window, cx: &mut App) {
    let v = cx.new(|cx| DiagramViewer { title, content, natural, pz: PanZoom { scale: 1., off: (0., 0.) }, fitted: true, drag: None, focus: cx.focus_handle() });
    let focus = v.read(cx).focus.clone();
    window.focus(&focus, cx);
    cx.set_global(ViewerSlot(Some((window.window_handle().window_id(), v))));
    cx.refresh_windows();
}

impl DiagramViewer {
    fn close(cx: &mut App) {
        cx.set_global(ViewerSlot(None));
        cx.refresh_windows();
    }

    fn viewport(window: &Window) -> (f32, f32) {
        let v = window.viewport_size();
        (f32::from(v.width), f32::from(v.height))
    }

    fn fit(&self, vp: (f32, f32)) -> PanZoom {
        PanZoom::fit(self.natural, vp, FIT_MARGIN, FIT_MAX)
    }

    /// 以视口坐标 `at` 为中心缩放(脱离「适应」状态)。
    fn zoom_by(&mut self, factor: f32, at: (f32, f32), cx: &mut Context<Self>) {
        self.fitted = false;
        self.pz.zoom_by_at(factor, at);
        cx.notify();
    }

    fn pan(&mut self, dx: f32, dy: f32, cx: &mut Context<Self>) {
        self.fitted = false;
        self.pz.pan(dx, dy);
        cx.notify();
    }

    /// 缩放到 100%(以视口中心为中心)。
    fn actual_size(&mut self, vp: (f32, f32), cx: &mut Context<Self>) {
        self.fitted = false;
        self.pz.zoom_to_at(1., (vp.0 / 2., vp.1 / 2.));
        cx.notify();
    }

    /// 双击:适应 ↔ 放大(原本就比 100% 大的小图放大一倍,其余放到 100%),放大以点击位置为中心。
    fn toggle_zoom(&mut self, at: (f32, f32), vp: (f32, f32), cx: &mut Context<Self>) {
        if self.fitted {
            let fit = self.fit(vp).scale;
            self.fitted = false;
            self.pz.zoom_to_at(if fit < 0.95 { 1. } else { fit * 2. }, at);
        } else {
            self.fitted = true;
        }
        cx.notify();
    }
}

impl Render for DiagramViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let vp = Self::viewport(window);
        if self.fitted {
            self.pz = self.fit(vp);
        } else {
            self.pz.clamp(self.natural, vp);
        }
        let pz = self.pz;
        let (cw, ch) = (self.natural.0 * pz.scale, self.natural.1 * pz.scale);
        let body: AnyElement = match &self.content {
            Content::Mermaid { scene, .. } => match scene.as_ref() {
                Ok(s) => crate::mermaid_view::scene_element(s, pz.scale, cx).into_any_element(),
                Err(_) => div().into_any_element(),
            },
            Content::Chart(spec) => crate::rich_text::chart_body("diagram-viewer-chart", spec, Some(cw), ch, cx),
        };
        let copy_src = match &self.content {
            Content::Mermaid { src, .. } => Some(src.clone()),
            Content::Chart(_) => None,
        };
        let dragging = self.drag.is_some();
        let tool_btn = |id: &'static str, label: AnyElement, tip: &'static str, f: fn(&mut DiagramViewer, (f32, f32), &mut Context<DiagramViewer>)| {
            div().id(id).min_w(px(30.)).h(px(30.)).px_1().flex().items_center().justify_center().rounded(px(8.)).cursor_pointer()
                .hover(|d| d.bg(t.list_hover)).active(|d| d.bg(t.list_active))
                .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tip).build(window, cx))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, window, cx| { cx.stop_propagation(); f(this, Self::viewport(window), cx); }))
                .child(label)
        };
        let glyph = |ic: IconName| icon(ic, 15., t.foreground).into_any_element();
        div().id("diagram-viewer").absolute().top_0().left_0().size_full().bg(t.background).occlude().track_focus(&self.focus)
            .cursor(if dragging { CursorStyle::ClosedHand } else { CursorStyle::OpenHand })
            .on_key_down(cx.listener(move |this, ev: &KeyDownEvent, window, cx| {
                let vp = Self::viewport(window);
                let step = if ev.keystroke.modifiers.shift { ARROW_STEP * 4. } else { ARROW_STEP };
                match ev.keystroke.key.as_str() {
                    "escape" => Self::close(cx),
                    "=" | "+" => this.zoom_by(1.25, (vp.0 / 2., vp.1 / 2.), cx),
                    "-" => this.zoom_by(1. / 1.25, (vp.0 / 2., vp.1 / 2.), cx),
                    "0" => { this.fitted = true; cx.notify(); }
                    "1" => this.actual_size(vp, cx),
                    "left" => this.pan(step, 0., cx),
                    "right" => this.pan(-step, 0., cx),
                    "up" => this.pan(0., step, cx),
                    "down" => this.pan(0., -step, cx),
                    _ => {}
                }
            }))
            .on_scroll_wheel(cx.listener(move |this, ev: &ScrollWheelEvent, _, cx| {
                let at = (f32::from(ev.position.x), f32::from(ev.position.y));
                // 鼠标滚轮:原生是按行的增量,窗口的平滑滚动层把它转成了像素增量,靠标记区分;触控板是原生像素增量
                let wheel = matches!(ev.delta, ScrollDelta::Lines(_)) || crate::smooth_scroll::is_wheel_synthetic();
                let d = ev.delta.pixel_delta(px(20.));
                let (dx, dy) = (f32::from(d.x), f32::from(d.y));
                if wheel {
                    // 鼠标滚轮:缩放
                    this.zoom_by((dy * 0.006).exp(), at, cx);
                } else if ev.modifiers.platform || ev.modifiers.control {
                    // 触控板 + ⌘ / ⌃:缩放
                    this.zoom_by((dy * 0.01).exp(), at, cx);
                } else {
                    // 触控板双指滑动:平移
                    this.pan(dx, dy, cx);
                }
            }))
            .on_pinch(cx.listener(|this, ev: &PinchEvent, _, cx| {
                this.zoom_by(1. + ev.delta, (f32::from(ev.position.x), f32::from(ev.position.y)), cx);
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                let at = (f32::from(ev.position.x), f32::from(ev.position.y));
                if ev.click_count >= 2 {
                    this.drag = None;
                    this.toggle_zoom(at, Self::viewport(window), cx);
                } else {
                    this.drag = Some((at, this.pz.off));
                }
            }))
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                if let Some((start, off0)) = this.drag {
                    this.fitted = false;
                    this.pz.off = (off0.0 + f32::from(ev.position.x) - start.0, off0.1 + f32::from(ev.position.y) - start.1);
                    cx.notify();
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| { this.drag = None; cx.notify(); }))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, cx| { this.drag = None; cx.notify(); }))
            .child(
                div().absolute().left(px(pz.off.0)).top(px(pz.off.1)).w(px(cw)).h(px(ch)).flex_none().overflow_hidden().child(body),
            )
            // 顶部:标题 + 操作提示 + 关闭
            .child(
                // 左侧让出红绿灯的位置(无标题栏窗口,同 TitleBar 的 80px)
                h_flex().absolute().top_0().left_0().right_0().h(px(48.)).pl(px(84.)).pr_4().gap_3().items_center()
                    .child(div().flex_none().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(self.title.clone()))
                    .child(div().flex_1().min_w_0().truncate().text_size(px(11.5)).text_color(t.muted_foreground).child("滚轮缩放 · 拖动平移 · 双击切换适应 / 100% · Esc 关闭"))
                    .child(
                        div().id("diagram-close").size(px(30.)).flex().items_center().justify_center().rounded(px(8.)).cursor_pointer()
                            .hover(|d| d.bg(t.list_hover)).active(|d| d.bg(t.list_active))
                            .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("关闭(Esc)").build(window, cx))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(|_, _, cx| { cx.stop_propagation(); Self::close(cx); })
                            .child(icon(IconName::X, 16., t.foreground)),
                    ),
            )
            // 底部工具条:缩小 / 比例 / 放大 / 适应 / 100% / 复制源码
            .child(
                div().absolute().bottom(px(24.)).left_0().right_0().flex().justify_center().child(
                    h_flex().gap_1().px_2().py_1().items_center().rounded(px(12.)).bg(t.popover).border_1().border_color(t.border).shadow_md()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(tool_btn("dv-out", glyph(IconName::Minus), "缩小(-)", |s, vp, cx| s.zoom_by(1. / 1.25, (vp.0 / 2., vp.1 / 2.), cx)))
                        .child(div().w(px(52.)).text_center().text_size(px(12.)).text_color(t.muted_foreground).child(format!("{:.0}%", pz.scale * 100.)))
                        .child(tool_btn("dv-in", glyph(IconName::Plus), "放大(+)", |s, vp, cx| s.zoom_by(1.25, (vp.0 / 2., vp.1 / 2.), cx)))
                        .child(tool_btn("dv-fit", glyph(IconName::Maximize), "适应窗口(0)", |s, _, cx| { s.fitted = true; cx.notify(); }))
                        .child(tool_btn("dv-100", div().text_size(px(12.)).text_color(t.foreground).child("1:1").into_any_element(), "实际大小 100%(1)", |s, vp, cx| s.actual_size(vp, cx)))
                        .when_some(copy_src, |d, src| d.child(
                            div().id("dv-copy").size(px(30.)).flex().items_center().justify_center().rounded(px(8.)).cursor_pointer()
                                .hover(|d| d.bg(t.list_hover)).active(|d| d.bg(t.list_active))
                                .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("复制源码").build(window, cx))
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(move |_, _, cx| { cx.stop_propagation(); cx.write_to_clipboard(ClipboardItem::new_string(src.clone())); })
                                .child(icon(IconName::Copy, 15., t.foreground)),
                        )),
                ),
            )
    }
}
