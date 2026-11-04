//! 差异标签的迷你地图:右侧的缩略图,每行一条(文字按语法着色、增删行带红 / 绿底色、折叠分隔条淡灰),
//! 带视口滑块,拖动 / 点击轨道 / 滚轮都能滚动差异正文。
//!
//! 与编辑器的迷你地图(`ide_minimap`)共用纯模型 `minimap_model`(色块与滑块几何),
//! 区别只在滚动的是一个 `ScrollHandle`(差异正文是整块自绘画布放在滚动容器里),而不是编辑器状态。
//!
//! @author ixxxxoooo

use crate::diff_code::{MiniRows, ROW_H as DIFF_ROW_H, Tint};
use crate::ide::IdeView;
use crate::minimap_model::{Geo, MAX_COLS};
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;
use std::cell::Cell;
use std::rc::Rc;

/// 缩略图每行像素高度。
const ROW_H: f32 = 2.;
/// 左右留白。
const PAD: f32 = 8.;
/// 差异区(正文 + 缩略图)窄于这个宽度时隐藏缩略图。
pub const MIN_W: f32 = 520.;

/// 跨帧共享的界面状态(`Cell`,鼠标移动时不必重建任何东西)。
#[derive(Default)]
pub struct DiffMiniUi {
    /// 上次绘制的缩略图区域(鼠标坐标换算用)
    pub bounds: Cell<Bounds<Pixels>>,
    /// 拖动滑块时,鼠标相对滑块顶部的偏移
    pub grab: Cell<Option<f32>>,
    pub hover: Cell<bool>,
    /// 差异区(正文 + 缩略图)上次的宽度;0 表示还没量过
    pub row_width: Cell<f32>,
}

/// 滚动容器当前视口对应的几何(缩略图高度或滚动区还没量过时为 `None`)。
fn geo_for(scroll: &ScrollHandle, ui: &DiffMiniUi, rows: usize) -> Option<Geo> {
    let h = f32::from(ui.bounds.get().size.height);
    let view_h = f32::from(scroll.bounds().size.height);
    if h <= 0. || view_h <= 0. {
        return None;
    }
    let start = (-f32::from(scroll.offset().y) / DIFF_ROW_H).max(0.);
    Some(Geo { row_h: ROW_H, panel_h: h, rows, vis_start: start, vis_len: (view_h / DIFF_ROW_H).max(1.) })
}

/// 把滚动容器滚到第 `start` 行(夹在可滚动范围内)。
fn scroll_to_row(scroll: &ScrollHandle, start: f32) {
    let max = f32::from(scroll.max_offset().y).max(0.);
    scroll.set_offset(point(px(0.), px(-(start * DIFF_ROW_H).clamp(0., max))));
}

/// 按住滑块拖动:鼠标纵坐标减去抓取偏移 = 滑块顶部,换算成首行再滚动过去。
fn apply_drag(scroll: &ScrollHandle, ui: &DiffMiniUi, rows: usize, y: Pixels) {
    let (Some(grab), Some(g)) = (ui.grab.get(), geo_for(scroll, ui, rows)) else { return };
    let y_local = f32::from(y - ui.bounds.get().origin.y);
    scroll_to_row(scroll, g.start_row_for_slider_top(y_local - grab));
}

/// 差异迷你地图元素。
///
/// 参数:`mini` 逐行数据;`scroll` 差异正文滚动容器的句柄;`ui` 共享界面状态;`width` 宽度;`slider_always` 滑块是否常驻
/// (否则只在悬停或拖动时显示)。
pub fn diff_minimap(mini: Rc<MiniRows>, scroll: ScrollHandle, ui: Rc<DiffMiniUi>, width: f32, slider_always: bool, cx: &mut Context<IdeView>) -> impl IntoElement {
    let t = cx.theme().clone();
    let rows = mini.left.rows.len();
    let col_w = (width - PAD * 2.) / MAX_COLS as f32;
    // 左右对比时每半边的列宽(每半边最多画 MAX_COLS / 2 列)
    let half_cw = (width / 2. - PAD * 2.) / (MAX_COLS / 2) as f32;
    let show_slider = slider_always || ui.hover.get() || ui.grab.get().is_some();
    let (bar, band) = (t.muted_foreground.opacity(0.5), t.foreground.opacity(0.05));
    let (slider_fill, slider_edge) = (t.foreground.opacity(if ui.grab.get().is_some() { 0.18 } else { 0.1 }), t.foreground.opacity(0.18));
    let tint_color = move |tint: Tint| match tint {
        Tint::None => None,
        Tint::Add => Some(t.success.opacity(0.35)),
        Tint::Del => Some(t.danger.opacity(0.35)),
        Tint::Sep => Some(t.muted.opacity(0.7)),
    };
    let (sc_down, sc_move, sc_wheel, sc_paint) = (scroll.clone(), scroll.clone(), scroll.clone(), scroll);
    let (ui_down, ui_move, ui_up, ui_out, ui_hover, ui_prepaint) = (ui.clone(), ui.clone(), ui.clone(), ui.clone(), ui.clone(), ui);
    div()
        .id("diff-minimap")
        .w(px(width))
        .h_full()
        .flex_none()
        .border_l_1()
        .border_color(cx.theme().border.opacity(0.6))
        .bg(cx.theme().muted.opacity(0.25))
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, cx.listener(move |_, ev: &MouseDownEvent, _, cx| {
            let Some(g) = geo_for(&sc_down, &ui_down, rows) else { return };
            let (top, h) = g.slider();
            let y_local = f32::from(ev.position.y - ui_down.bounds.get().origin.y);
            // 点在滑块上:保持抓取点;点在轨道上:滑块中心跳到点击处
            ui_down.grab.set(Some(if y_local >= top && y_local <= top + h { y_local - top } else { h / 2. }));
            apply_drag(&sc_down, &ui_down, rows, ev.position.y);
            cx.notify();
        }))
        .on_mouse_move(cx.listener(move |_, ev: &MouseMoveEvent, _, cx| {
            if ev.pressed_button == Some(MouseButton::Left) {
                apply_drag(&sc_move, &ui_move, rows, ev.position.y);
                cx.notify();
            }
        }))
        .on_mouse_up(MouseButton::Left, cx.listener(move |_, _, _, cx| {
            ui_up.grab.set(None);
            cx.notify();
        }))
        .on_mouse_up_out(MouseButton::Left, cx.listener(move |_, _, _, cx| {
            ui_out.grab.set(None);
            cx.notify();
        }))
        .on_hover(cx.listener(move |_, hovered: &bool, _, cx| {
            ui_hover.hover.set(*hovered);
            cx.notify();
        }))
        // 滚轮在缩略图上也滚动差异正文
        .on_scroll_wheel(cx.listener(move |_, ev: &ScrollWheelEvent, _, cx| {
            let off = sc_wheel.offset();
            let dy = ev.delta.pixel_delta(px(DIFF_ROW_H)).y;
            let max = sc_wheel.max_offset().y;
            sc_wheel.set_offset(point(off.x, (off.y + dy).clamp(-max, px(0.))));
            cx.stop_propagation();
            cx.notify();
        }))
        .child(
            canvas(
                move |bounds, _, _| ui_prepaint.bounds.set(bounds),
                move |bounds, _, window, _| {
                    let h = f32::from(bounds.size.height);
                    let view_h = f32::from(sc_paint.bounds().size.height);
                    if h <= 0. || view_h <= 0. {
                        return;
                    }
                    let g = Geo { row_h: ROW_H, panel_h: h, rows, vis_start: (-f32::from(sc_paint.offset().y) / DIFF_ROW_H).max(0.), vis_len: (view_h / DIFF_ROW_H).max(1.) };
                    // 整体偏移对齐设备像素:行距均匀,滚动时按半像素连续移动
                    let scale = window.scale_factor();
                    let off = (g.offset() * scale).round() / scale;
                    let row_y = |row: f32| row * ROW_H - off;
                    // 视口带
                    let (y0, y1) = (row_y(g.vis_start).max(0.), row_y(g.vis_start + g.vis_len).min(h));
                    if y1 > y0 {
                        window.paint_quad(fill(Bounds::new(point(bounds.origin.x, bounds.origin.y + px(y0)), size(bounds.size.width, px(y1 - y0))), band));
                    }
                    let first = (off / ROW_H).floor().max(0.) as usize;
                    let last = (((off + h) / ROW_H).ceil() as usize + 1).min(rows);
                    let half = bounds.size.width / 2.;
                    for i in first..last {
                        let y = bounds.origin.y + px(row_y(i as f32));
                        // 增删底色:左右对比时左半边画旧侧、右半边画新侧;两半相同(统一视图 / 折叠条)则整行
                        let (l, r) = mini.tints[i];
                        if l == r {
                            if let Some(c) = tint_color(l) {
                                window.paint_quad(fill(Bounds::new(point(bounds.origin.x + px(1.), y), size(bounds.size.width - px(1.), px(ROW_H))), c));
                            }
                        } else {
                            if let Some(c) = tint_color(l) {
                                window.paint_quad(fill(Bounds::new(point(bounds.origin.x + px(1.), y), size(half - px(1.), px(ROW_H))), c));
                            }
                            if let Some(c) = tint_color(r) {
                                window.paint_quad(fill(Bounds::new(point(bounds.origin.x + half, y), size(half, px(ROW_H))), c));
                            }
                        }
                        // 文字:左右对比时左半边画旧侧、右半边画新侧(各用一半宽度);否则整行一份
                        let sides: &[(&crate::minimap_model::MiniMap<Hsla>, f32, f32)] = if mini.split {
                            &[(&mini.left, 0., half_cw), (&mini.right, f32::from(half), half_cw)]
                        } else {
                            &[(&mini.left, 0., col_w)]
                        };
                        let max_cols = if mini.split { MAX_COLS / 2 } else { MAX_COLS };
                        for (map, dx, cw) in sides {
                            for run in &map.rows[i] {
                                let start = usize::from(run.start);
                                if start >= max_cols {
                                    continue;
                                }
                                let len = usize::from(run.len).min(max_cols - start);
                                let x = bounds.origin.x + px(dx + PAD + start as f32 * cw);
                                let w = run.weight.alpha();
                                let color = run.color.map_or(bar.opacity(bar.a * w), |c| c.opacity(0.85 * w));
                                let rect = Bounds::new(point(x, y), size(px((len as f32 * cw).max(1.)), px(ROW_H * 0.8)));
                                window.paint_quad(quad(rect, px(0.6), color, 0., transparent_black(), BorderStyle::default()));
                            }
                        }
                    }
                    if show_slider {
                        let top = row_y(g.start());
                        let rect = Bounds::new(point(bounds.origin.x + px(1.), bounds.origin.y + px(top)), size(bounds.size.width - px(2.), px(g.slider().1)));
                        window.paint_quad(quad(rect, px(3.), slider_fill, 1., slider_edge, BorderStyle::default()));
                    }
                },
            )
            .size_full(),
        )
}
