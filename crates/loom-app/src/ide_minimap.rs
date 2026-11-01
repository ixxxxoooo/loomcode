//! 迷你地图(minimap):编辑器右侧的缩略图,按语法着色,带视口滑块、Git 改动刻度与光标刻度。
//!
//! - 用 `canvas` 批量填充色块(一行一组同色连续块),不为每行建元素,几万行也不卡。
//! - 文档比面板长时缩略图按编辑器滚动比例滚动,滑块的行为和滚动条一致(抓取偏移拖动、点击轨道跳转)。
//! - 数据与几何在 `minimap_model`(纯逻辑,有测试),这里只负责取数、绘制与鼠标事件。
//!
//! @author ixxxxoooo

use crate::git_status::GitMark;
use crate::ide::IdeView;
use crate::minimap_model::{Geo, MiniMap, build};
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::highlighter::{HighlightTheme, SyntaxHighlighter};
use gpui_kit::component::input::EditorState;
use gpui_kit::component::Rope;
use gpui_kit::*;
use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

/// 缩略图宽度。
pub const WIDTH: f32 = 92.;
/// 每行像素高度。
const ROW_H: f32 = 2.;
/// 每列像素宽度(120 列刚好放进 [`WIDTH`])。
const COL_W: f32 = 0.66;
/// 编辑区(含缩略图)窄于这个宽度时自动隐藏缩略图。
pub const MIN_EDITOR_W: f32 = 520.;
/// 超过这个字节数的文件不做语法着色(单色缩略图),避免每次编辑重新解析太久。
const MAX_SYNTAX_BYTES: usize = 300_000;

/// 跨帧共享的界面状态(用 `Cell`,鼠标移动时不必重建任何东西)。
#[derive(Default)]
pub struct MinimapUi {
    /// 上次绘制的缩略图区域(鼠标坐标换算用)
    pub bounds: Cell<Bounds<Pixels>>,
    /// 正在拖动滑块时,鼠标相对滑块顶部的偏移
    pub grab: Cell<Option<f32>>,
    /// 鼠标是否悬停在缩略图上
    pub hover: Cell<bool>,
    /// 编辑区(编辑器 + 缩略图)上次的宽度;0 表示还没量过
    pub row_width: Cell<f32>,
}

/// 生成一个文档的缩略数据。
///
/// 参数:`text` 全文;`lang` 语言名;`syntax` 是否着色;`theme` 语法主题。
/// 太大、纯文本或解析失败时退化成单色。
pub fn build_for(text: &str, lang: &str, syntax: bool, theme: &HighlightTheme) -> MiniMap<Hsla> {
    let mut styles: Vec<(Range<usize>, Option<Hsla>)> = Vec::new();
    if syntax && lang != "text" && text.len() <= MAX_SYNTAX_BYTES {
        let mut hl = SyntaxHighlighter::new(lang);
        hl.update(None, &Rope::from(text), Some(Duration::from_millis(80)));
        styles = hl.styles(&(0..text.len()), theme).into_iter().map(|(r, s)| (r, s.color)).collect();
        styles.sort_by_key(|(r, _)| r.start);
    }
    build(text, &styles)
}

/// 缩略图高度已知时,由编辑器视口得到几何。
fn geo_for(editor: &Entity<EditorState>, ui: &MinimapUi, rows: usize, cx: &App) -> Option<Geo> {
    let vis = editor.read(cx).visible_row_range()?;
    let h = f32::from(ui.bounds.get().size.height);
    (h > 0.).then(|| Geo { row_h: ROW_H, panel_h: h, rows, vis_start: vis.start, vis_len: vis.len().max(1) })
}

/// 按住滑块拖动:鼠标纵坐标减去抓取偏移 = 滑块顶部,换算成编辑器的首行并滚动过去。
fn apply_drag(editor: &Entity<EditorState>, ui: &MinimapUi, rows: usize, y: Pixels, cx: &mut App) {
    let (Some(grab), Some(g)) = (ui.grab.get(), geo_for(editor, ui, rows, cx)) else { return };
    let y_local = f32::from(y - ui.bounds.get().origin.y);
    let start = g.start_row_for_slider_top(y_local - grab);
    editor.update(cx, |e, cx| {
        let Some(lh) = e.line_height() else { return };
        let off = e.scroll_offset();
        e.set_scroll_offset(point(off.x, px(-(start * f32::from(lh)))), cx);
    });
}

/// 迷你地图元素。
///
/// 参数:`editor` 当前文档编辑器;`mm` 缩略数据;`hunks` Git 改动行区间;`ui` 共享界面状态;
/// `slider_always` 为真时滑块一直显示,否则只在悬停或拖动时显示。
pub fn minimap(
    editor: &Entity<EditorState>,
    mm: Rc<MiniMap<Hsla>>,
    hunks: Rc<Vec<(Range<usize>, GitMark)>>,
    ui: Rc<MinimapUi>,
    slider_always: bool,
    cx: &mut Context<IdeView>,
) -> impl IntoElement {
    let t = cx.theme().clone();
    let rows = mm.rows.len();
    let (vis_start, vis_len) = editor.read(cx).visible_row_range().map_or((0, 1), |r| (r.start, r.len().max(1)));
    let cursor_row = editor.read(cx).cursor_position().line as usize;
    let show_slider = slider_always || ui.hover.get() || ui.grab.get().is_some();
    let (bar, slider_fill) = (t.muted_foreground.opacity(0.5), t.foreground.opacity(if ui.grab.get().is_some() { 0.16 } else { 0.1 }));
    let (added, modified, deleted, cursor_c) = (t.success, t.warning, t.danger, t.primary);
    let (ed_down, ed_move, ed_wheel) = (editor.clone(), editor.clone(), editor.clone());
    let (ui_down, ui_move, ui_up, ui_hover, ui_wheel, ui_prepaint) = (ui.clone(), ui.clone(), ui.clone(), ui.clone(), ui.clone(), ui);
    div()
        .id("ide-minimap")
        .w(px(WIDTH))
        .h_full()
        .flex_none()
        .border_l_1()
        .border_color(t.border)
        .bg(t.background)
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, cx.listener(move |_, ev: &MouseDownEvent, _, cx| {
            let Some(g) = geo_for(&ed_down, &ui_down, rows, cx) else { return };
            let (top, h) = g.slider();
            let y_local = f32::from(ev.position.y - ui_down.bounds.get().origin.y);
            // 点在滑块上:保持抓取点;点在轨道上:滑块中心跳到点击处
            let grab = if y_local >= top && y_local <= top + h { y_local - top } else { h / 2. };
            ui_down.grab.set(Some(grab));
            apply_drag(&ed_down, &ui_down, rows, ev.position.y, cx);
            cx.notify();
        }))
        .on_mouse_move(cx.listener(move |_, ev: &MouseMoveEvent, _, cx| {
            if ev.pressed_button == Some(MouseButton::Left) {
                apply_drag(&ed_move, &ui_move, rows, ev.position.y, cx);
            }
        }))
        .on_mouse_up(MouseButton::Left, cx.listener({
            let ui = ui_up.clone();
            move |_, _, _, cx| {
                ui.grab.set(None);
                cx.notify();
            }
        }))
        .on_mouse_up_out(MouseButton::Left, cx.listener(move |_, _, _, cx| {
            ui_up.grab.set(None);
            cx.notify();
        }))
        .on_hover(cx.listener(move |_, hovered: &bool, _, cx| {
            ui_hover.hover.set(*hovered);
            cx.notify();
        }))
        // 滚轮在缩略图上也滚动编辑器
        .on_scroll_wheel(cx.listener(move |_, ev: &ScrollWheelEvent, _, cx| {
            let _ = &ui_wheel;
            ed_wheel.update(cx, |e, cx| {
                let Some(lh) = e.line_height() else { return };
                let off = e.scroll_offset();
                let dy = ev.delta.pixel_delta(lh).y;
                e.set_scroll_offset(point(off.x, (off.y + dy).min(px(0.))), cx);
            });
            cx.stop_propagation();
        }))
        .child(
            canvas(
                move |bounds, _, _| ui_prepaint.bounds.set(bounds),
                move |bounds, _, window, _| {
                    let h = f32::from(bounds.size.height);
                    let g = Geo { row_h: ROW_H, panel_h: h, rows, vis_start, vis_len };
                    let off = g.offset();
                    // 只画落在面板内的行
                    let first = (off / ROW_H).floor().max(0.) as usize;
                    let last = (((off + h) / ROW_H).ceil() as usize + 1).min(rows);
                    for i in first..last {
                        let y = bounds.origin.y + px(g.row_y(i as f32));
                        for r in &mm.rows[i] {
                            let x = bounds.origin.x + px(6. + f32::from(r.start) * COL_W);
                            let color = r.color.map_or(bar, |c| c.opacity(0.9));
                            window.paint_quad(fill(Bounds::new(point(x, y), size(px(f32::from(r.len) * COL_W), px(ROW_H * 0.75))), color));
                        }
                    }
                    if show_slider {
                        let (top, sh) = g.slider();
                        window.paint_quad(fill(Bounds::new(point(bounds.origin.x, bounds.origin.y + px(top)), size(bounds.size.width, px(sh))), slider_fill));
                    }
                    // Git 改动刻度:贴右缘的细条
                    for (range, mark) in hunks.iter() {
                        let color = match mark {
                            GitMark::Added => added,
                            GitMark::Modified => modified,
                            GitMark::Deleted => deleted,
                        };
                        let y = g.row_y(range.start as f32);
                        let hh = (ROW_H * range.len() as f32).max(2.);
                        if y + hh >= 0. && y <= h {
                            window.paint_quad(fill(Bounds::new(point(bounds.origin.x + bounds.size.width - px(3.), bounds.origin.y + px(y)), size(px(2.), px(hh))), color.opacity(0.9)));
                        }
                    }
                    // 光标所在行的刻度
                    let cy = g.row_y(cursor_row as f32);
                    if cy >= 0. && cy <= h {
                        window.paint_quad(fill(Bounds::new(point(bounds.origin.x, bounds.origin.y + px(cy)), size(bounds.size.width, px(1.))), cursor_c.opacity(0.7)));
                    }
                },
            )
            .size_full(),
        )
}
