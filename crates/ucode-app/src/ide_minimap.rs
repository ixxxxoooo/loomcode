//! 缩略图(minimap):在编辑器右侧画一列缩小的「行形状」,高亮当前视口,点击 / 拖动可快速滚动。
//! 用 `canvas` 直接画矩形,不为每一行建元素,所以几万行也不卡。
//!
//! @author ixxxxoooo

use crate::ide::IdeView;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::input::EditorState;
use gpui_kit::*;
use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;

/// 缩略图宽度。
pub const WIDTH: f32 = 92.;
/// 单行最大高度(像素);行数少时按它画,行数多时整体压缩到刚好放下。
const MAX_ROW_H: f32 = 2.;

/// 计算每一行的形状:(缩进列数, 内容长度),制表符按 4 列算,内容长度上限 160。
pub fn shape_of(text: &str) -> Vec<(u16, u16)> {
    text.split('\n')
        .map(|line| {
            let line = line.trim_end_matches('\r');
            let indent: usize = line.chars().take_while(|c| c.is_whitespace()).map(|c| if c == '\t' { 4 } else { 1 }).sum();
            let len = line.trim().chars().count().min(160);
            (indent.min(80) as u16, len as u16)
        })
        .collect()
}

/// 每行高度:行数少按 [`MAX_ROW_H`],多则压缩到刚好放下(但不小于 0.4 像素)。
pub fn row_height(rows: usize, height: f32) -> f32 {
    (height / rows.max(1) as f32).clamp(0.4, MAX_ROW_H)
}

/// 缩略图元素。
///
/// 参数:`editor` 当前文档编辑器;`shape` 行形状;`bounds_cell` 用来记下上次绘制的区域(供鼠标换算位置)。
pub fn minimap(editor: &Entity<EditorState>, shape: Rc<Vec<(u16, u16)>>, bounds_cell: Rc<Cell<Bounds<Pixels>>>, cx: &mut Context<IdeView>) -> impl IntoElement {
    let t = cx.theme().clone();
    let visible: Option<Range<usize>> = editor.read(cx).visible_row_range();
    let (bar, view_fill) = (t.muted_foreground.opacity(0.45), t.foreground.opacity(0.09));
    let (rows, cell) = (shape.len(), bounds_cell.clone());
    let (ed1, ed2, cell1, cell2) = (editor.clone(), editor.clone(), bounds_cell.clone(), bounds_cell);
    div()
        .id("ide-minimap")
        .w(px(WIDTH))
        .h_full()
        .flex_none()
        .border_l_1()
        .border_color(t.border)
        .bg(t.background)
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, cx.listener(move |_, ev: &MouseDownEvent, _, cx| scroll_to(&ed1, &cell1, rows, ev.position.y, cx)))
        .on_mouse_move(cx.listener(move |_, ev: &MouseMoveEvent, _, cx| {
            if ev.pressed_button == Some(MouseButton::Left) {
                scroll_to(&ed2, &cell2, rows, ev.position.y, cx);
            }
        }))
        .child(
            canvas(
                move |bounds, _, _| cell.set(bounds),
                move |bounds, _, window, _| {
                    let row_h = row_height(rows, f32::from(bounds.size.height));
                    // 行数太多时每隔 step 行画一条
                    let step = (1.0 / row_h).ceil().max(1.) as usize;
                    let max_w = f32::from(bounds.size.width) - 12.;
                    for (i, (indent, len)) in shape.iter().enumerate().step_by(step) {
                        if *len == 0 {
                            continue;
                        }
                        let y = bounds.origin.y + px(row_h * i as f32);
                        let x = bounds.origin.x + px(6. + f32::from(*indent).min(max_w * 0.6) * 0.7);
                        let w = (f32::from(*len) * 0.7).min(max_w - f32::from(x - bounds.origin.x - px(6.))).max(1.);
                        window.paint_quad(fill(Bounds::new(point(x, y), size(px(w), px((row_h * 0.75).max(0.5)))), bar));
                    }
                    if let Some(v) = &visible {
                        let top = bounds.origin.y + px(row_h * v.start as f32);
                        let h = px((row_h * v.len() as f32).max(6.));
                        window.paint_quad(fill(Bounds::new(point(bounds.origin.x, top), size(bounds.size.width, h)), view_fill));
                    }
                },
            )
            .size_full(),
        )
}

/// 鼠标在缩略图上的纵坐标 → 把对应行滚到视口中央。
fn scroll_to(editor: &Entity<EditorState>, cell: &Rc<Cell<Bounds<Pixels>>>, rows: usize, y: Pixels, cx: &mut App) {
    let b = cell.get();
    if b.size.height <= px(0.) {
        return;
    }
    let row_h = row_height(rows, f32::from(b.size.height));
    let target_row = (f32::from(y - b.origin.y) / row_h).max(0.);
    editor.update(cx, |e, cx| {
        let (Some(lh), Some(vis)) = (e.line_height(), e.visible_row_range()) else { return };
        let top = (target_row - vis.len() as f32 / 2.).max(0.) * f32::from(lh);
        let off = e.scroll_offset();
        e.set_scroll_offset(point(off.x, px(-top)), cx);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 行形状() {
        let s = shape_of("fn a() {\n    let x = 1;\n\n\treturn;\r\n}");
        assert_eq!(s.len(), 5);
        assert_eq!(s[0], (0, 8));
        assert_eq!(s[1], (4, 10));
        assert_eq!(s[2], (0, 0));
        assert_eq!(s[3], (4, 7));
    }

    #[::core::prelude::v1::test]
    fn 行高随行数压缩() {
        assert_eq!(row_height(10, 600.), MAX_ROW_H);
        assert!((row_height(3000, 600.) - 0.4).abs() < 1e-6 || row_height(3000, 600.) < MAX_ROW_H);
        assert!(row_height(0, 600.) <= MAX_ROW_H);
    }
}
