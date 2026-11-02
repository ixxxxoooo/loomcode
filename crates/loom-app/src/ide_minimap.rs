//! 迷你地图(minimap):编辑器右侧的缩略图,按语法着色、按字形疏密分三档深浅(标点淡、字母适中、大写与符号浓),
//! 带视口滑块、选区色带、Git 改动刻度、诊断(错误 / 警告)刻度与光标刻度。
//!
//! - 用 `canvas` 批量填充色块(一行一组同色连续块),不为每行建元素,几万行也不卡。
//! - 文档比面板长时缩略图按编辑器滚动比例滚动,滑块的行为和滚动条一致(抓取偏移拖动、点击轨道跳转)。
//! - 数据与几何在 `minimap_model`(纯逻辑,有测试),这里只负责取数、绘制与鼠标事件。
//!
//! @author ixxxxoooo

use crate::git_status::GitMark;
use crate::ide::IdeView;
use crate::minimap_model::{Geo, MAX_COLS, MiniMap, build};
use gpui_kit::component::RopeExt as _;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::highlighter::{HighlightTheme, SyntaxHighlighter};
use gpui_kit::component::input::EditorState;
use gpui_kit::component::Rope;
use gpui_kit::*;
use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

/// 每行像素高度。
const ROW_H: f32 = 2.;
/// 左右留白。
const PAD: f32 = 8.;
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

/// 迷你地图的显示选项(来自设置)。
#[derive(Clone, Copy)]
pub struct MinimapOpts {
    /// 宽度(像素)
    pub width: f32,
    /// 滑块始终显示
    pub slider_always: bool,
    /// 显示 Git 改动刻度
    pub git: bool,
}

impl MinimapOpts {
    /// 由编辑器偏好得到选项,宽度限制在 60–160。
    pub fn from_prefs(p: &loom_config::EditorPrefs) -> Self {
        Self { width: f32::from(p.minimap_width.clamp(60, 160)), slider_always: p.minimap_slider_always, git: p.minimap_git }
    }
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

/// 编辑器视口的 (首行, 行数),含小数。
///
/// 编辑器按像素平滑滚动,所以首行取「滚动偏移 ÷ 行高」的连续值,而不是 `visible_row_range` 的整数行;
/// 行数取画布高度 ÷ 行高(画布与编辑器同高)。自动换行等原因使两者对不上时,退回整数行范围。
/// 滚动偏移是实时状态,在绘制阶段读到的就是当帧值(不像布局信息要等编辑器自己绘制后才更新)。
fn viewport(ed: &EditorState, panel_h: f32) -> Option<(f32, f32)> {
    let vis = ed.visible_row_range()?;
    let lh = f32::from(ed.line_height()?);
    if lh <= 0. {
        return None;
    }
    let start = (-f32::from(ed.scroll_offset().y) / lh).max(0.);
    let len = (panel_h / lh).max(1.);
    let consistent = (start - vis.start as f32).abs() <= 2.;
    Some(if consistent { (start, len) } else { (vis.start as f32, vis.len().max(1) as f32) })
}

/// 缩略图高度已知时,由编辑器视口得到几何。
fn geo_for(editor: &Entity<EditorState>, ui: &MinimapUi, rows: usize, cx: &App) -> Option<Geo> {
    let h = f32::from(ui.bounds.get().size.height);
    if h <= 0. {
        return None;
    }
    let (vis_start, vis_len) = viewport(editor.read(cx), h)?;
    Some(Geo { row_h: ROW_H, panel_h: h, rows, vis_start, vis_len })
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
/// 参数:`editor` 当前文档编辑器;`mm` 缩略数据;`hunks` Git 改动行区间;`diag_marks` 诊断所在行与是否错误;`ui` 共享界面状态;
/// `opts` 宽度、滑块与 Git 刻度显示选项(滑块非常驻时只在悬停或拖动时显示)。
pub fn minimap(
    editor: &Entity<EditorState>,
    mm: Rc<MiniMap<Hsla>>,
    hunks: Rc<Vec<(Range<usize>, GitMark)>>,
    diag_marks: Rc<Vec<(usize, bool)>>,
    ui: Rc<MinimapUi>,
    opts: MinimapOpts,
    cx: &mut Context<IdeView>,
) -> impl IntoElement {
    let t = cx.theme().clone();
    let rows = mm.rows.len();
    let cursor_row = editor.read(cx).cursor_position().line as usize;
    // 选区覆盖的行范围(无选区为 None),在缩略图上画一条淡色带
    let sel_rows = {
        let ed = editor.read(cx);
        let r = ed.selected_range();
        (r.start != r.end).then(|| (ed.text().offset_to_position(r.start).line as usize, ed.text().offset_to_position(r.end).line as usize))
    };
    let (diag_err, diag_warn) = (t.danger, t.warning);
    let sel_c = t.primary;
    let (width, col_w) = (opts.width, (opts.width - PAD * 2.) / MAX_COLS as f32);
    let show_slider = opts.slider_always || ui.hover.get() || ui.grab.get().is_some();
    let (bar, slider_fill, slider_edge) = (t.muted_foreground.opacity(0.5), t.foreground.opacity(if ui.grab.get().is_some() { 0.18 } else { 0.1 }), t.foreground.opacity(0.18));
    let band = t.foreground.opacity(0.05);
    let (added, modified, deleted, cursor_c) = (t.success, t.warning, t.danger, t.primary);
    let (ed_down, ed_move, ed_wheel, ed_paint) = (editor.clone(), editor.clone(), editor.clone(), editor.clone());
    let (ui_down, ui_move, ui_up, ui_hover, ui_wheel, ui_prepaint) = (ui.clone(), ui.clone(), ui.clone(), ui.clone(), ui.clone(), ui);
    div()
        .id("ide-minimap")
        .w(px(width))
        .h_full()
        .flex_none()
        .border_l_1()
        .border_color(t.border.opacity(0.6))
        .bg(t.muted.opacity(0.25))
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
                move |bounds, _, window, cx| {
                    let h = f32::from(bounds.size.height);
                    // 在绘制阶段读编辑器:与编辑器同一帧的滚动位置,缩略图不会比正文慢半拍
                    let Some((vis_start, vis_len)) = viewport(ed_paint.read(cx), h) else { return };
                    let g = Geo { row_h: ROW_H, panel_h: h, rows, vis_start, vis_len };
                    // 整体偏移对齐到设备像素(而不是逐行取整):行距保持均匀,滚动时按半像素连续移动
                    let scale = window.scale_factor();
                    let snap = |v: f32| (v * scale).round() / scale;
                    let off = snap(g.offset());
                    let row_y = |row: f32| row * ROW_H - off;
                    // 视口带:缩略图上标出编辑器当前可见的范围(随连续位置移动,不再按行切换文字深浅)
                    {
                        let (y0, y1) = (row_y(vis_start).max(0.), row_y(vis_start + vis_len).min(h));
                        if y1 > y0 {
                            window.paint_quad(fill(Bounds::new(point(bounds.origin.x, bounds.origin.y + px(y0)), size(bounds.size.width, px(y1 - y0))), band));
                        }
                    }
                    // 只画落在面板内的行
                    let first = (off / ROW_H).floor().max(0.) as usize;
                    let last = (((off + h) / ROW_H).ceil() as usize + 1).min(rows);
                    for i in first..last {
                        let y = bounds.origin.y + px(row_y(i as f32));
                        for r in &mm.rows[i] {
                            let x = bounds.origin.x + px(PAD + f32::from(r.start) * col_w);
                            // 墨量档位再叠一层深浅:标点淡、大写与符号浓
                            let w = r.weight.alpha();
                            let color = r.color.map_or(bar.opacity(bar.a * w), |c| c.opacity(0.85 * w));
                            let rect = Bounds::new(point(x, y), size(px((f32::from(r.len) * col_w).max(1.)), px(ROW_H * 0.8)));
                            window.paint_quad(quad(rect, px(0.6), color, 0., transparent_black(), BorderStyle::default()));
                        }
                    }
                    // 选区色带:盖在文字下面一层,先画
                    if let Some((a, b)) = sel_rows {
                        let (y0, y1) = (row_y(a as f32), row_y(b as f32 + 1.));
                        if y1 >= 0. && y0 <= h {
                            window.paint_quad(fill(Bounds::new(point(bounds.origin.x + px(1.), bounds.origin.y + px(y0.max(0.))), size(bounds.size.width - px(2.), px((y1.min(h) - y0.max(0.)).max(2.)))), sel_c.opacity(0.22)));
                        }
                    }
                    if show_slider {
                        let sh = g.slider().1;
                        let top = row_y(g.start());
                        let rect = Bounds::new(point(bounds.origin.x + px(1.), bounds.origin.y + px(top)), size(bounds.size.width - px(2.), px(sh)));
                        window.paint_quad(quad(rect, px(3.), slider_fill, 1., slider_edge, BorderStyle::default()));
                    }
                    // Git 改动刻度:贴右缘的细条
                    for (range, mark) in hunks.iter().filter(|_| opts.git) {
                        let color = match mark {
                            GitMark::Added => added,
                            GitMark::Modified => modified,
                            GitMark::Deleted => deleted,
                        };
                        let y = row_y(range.start as f32);
                        let hh = (ROW_H * range.len() as f32).max(2.);
                        if y + hh >= 0. && y <= h {
                            window.paint_quad(fill(Bounds::new(point(bounds.origin.x + bounds.size.width - px(3.), bounds.origin.y + px(y)), size(px(2.), px(hh))), color.opacity(0.9)));
                        }
                    }
                    // 诊断刻度:错误红、警告黄,贴在 Git 刻度左边
                    for (row, is_err) in diag_marks.iter() {
                        let y = row_y(*row as f32);
                        if y >= -ROW_H && y <= h {
                            let c = if *is_err { diag_err } else { diag_warn };
                            window.paint_quad(fill(Bounds::new(point(bounds.origin.x + bounds.size.width - px(7.), bounds.origin.y + px(y)), size(px(3.), px(ROW_H.max(2.)))), c.opacity(0.95)));
                        }
                    }
                    // 光标所在行的刻度
                    let cy = row_y(cursor_row as f32);
                    if cy >= 0. && cy <= h {
                        window.paint_quad(quad(Bounds::new(point(bounds.origin.x + px(2.), bounds.origin.y + px(cy)), size(bounds.size.width - px(4.), px(1.5))), px(1.), cursor_c.opacity(0.8), 0., transparent_black(), BorderStyle::default()));
                    }
                },
            )
            .size_full(),
        )
}
