//! 终端网格绘制:整个画面在一个 canvas 里按格子坐标画出来(参考 Zed 的做法)。
//!
//! 文字用 `shape_line(..., Some(格宽))` 强制每个字形占一格(宽字符占两格),
//! 背景、选区、搜索高亮、光标都按「列 × 格宽」定位,所以中文 / emoji 不会把同一行后面的内容挤歪。
//!
//! @author ixxxxoooo

use crate::term_blocks::block;
use crate::term_geom::selection_spans;
use crate::term_palette::indexed_rgb;
use crate::ui::MONO;
use gpui_kit::*;
use loom_config::CursorShape;
use loom_term::{Run, Snapshot, TermColor};

/// 绘制一帧需要的全部输入。
pub struct GridPaint {
    pub snap: Snapshot,
    pub cell_w: Pixels,
    pub line_h: f32,
    pub font_size: f32,
    pub pad: f32,
    pub dark: bool,
    pub fg: Hsla,
    pub bg: Hsla,
    /// 选区与光标的强调色
    pub accent: Hsla,
    /// 搜索高亮色
    pub find: Hsla,
    pub cursor: CursorShape,
    pub focused: bool,
    /// 搜索匹配:(画面行, 起始列, 格数, 是否当前项)
    pub matches: Vec<(usize, usize, usize, bool)>,
}

/// 终端颜色 → 界面颜色;`Default` 用调用方给的默认色。
pub fn to_hsla(c: TermColor, default: Hsla, dark: bool) -> Hsla {
    let (r, g, b) = match c {
        TermColor::Default => return default,
        TermColor::Indexed(i) => indexed_rgb(i, dark),
        TermColor::Rgb(r, g, b) => (r, g, b),
    };
    rgb(((r as u32) << 16) | ((g as u32) << 8) | b as u32).into()
}

/// 一个文字段最终的 (前景, 背景, 粗体, 斜体, 下划线, 删除线)。反色与暗淡在这里处理。
fn resolve(run: &Run, fg_default: Hsla, bg_default: Hsla, dark: bool) -> (Hsla, Option<Hsla>) {
    let s = run.style;
    let (mut fg, mut bg) = (to_hsla(run.fg, fg_default, dark), (run.bg != TermColor::Default).then(|| to_hsla(run.bg, bg_default, dark)));
    if s.inverse {
        (fg, bg) = (bg.unwrap_or(bg_default), Some(fg));
    }
    if s.dim {
        fg = fg.opacity(0.6);
    }
    (fg, bg)
}

impl GridPaint {
    /// 格子 (行, 列) 的左上角,取整到像素,相邻格子共用同一条边,不会出现缝。
    fn x(&self, origin: Point<Pixels>, col: usize) -> Pixels {
        origin.x + px(f32::from(self.cell_w * col as f32).round())
    }

    fn y(&self, origin: Point<Pixels>, row: usize) -> Pixels {
        origin.y + px((self.line_h * row as f32).round())
    }

    /// 画一块 `cells` 格宽、一行高的矩形。
    fn fill_cells(&self, origin: Point<Pixels>, row: usize, col: usize, cells: usize, color: Hsla, window: &mut Window) {
        let (x0, x1) = (self.x(origin, col), self.x(origin, col + cells));
        let (y0, y1) = (self.y(origin, row), self.y(origin, row + 1));
        window.paint_quad(fill(Bounds::new(point(x0, y0), size(x1 - x0, y1 - y0)), color));
    }

    /// 方块字符:按格子分数画矩形。
    fn paint_block(&self, origin: Point<Pixels>, row: usize, col: usize, ch: char, color: Hsla, window: &mut Window) {
        let Some(b) = block(ch) else { return };
        let (cx, cy, w, h) = (f32::from(self.cell_w) * col as f32, self.line_h * row as f32, f32::from(self.cell_w), self.line_h);
        for (x0, y0, x1, y1) in b.rects {
            let (l, r) = ((cx + x0 * w).round(), (cx + x1 * w).round());
            let (t, bt) = ((cy + y0 * h).round(), (cy + y1 * h).round());
            window.paint_quad(fill(Bounds::new(point(origin.x + px(l), origin.y + px(t)), size(px(r - l), px(bt - t))), color.opacity(b.alpha)));
        }
    }

    /// 画一个文字段里的文字(方块字符另画,其余按列定位后强制格宽排版)。
    fn paint_run(&self, origin: Point<Pixels>, row: usize, run: &Run, fg: Hsla, window: &mut Window, cx: &mut App) {
        let s = run.style;
        let font = Font {
            family: MONO.into(),
            features: FontFeatures::default(),
            fallbacks: None,
            weight: if s.bold { FontWeight::BOLD } else { FontWeight::NORMAL },
            style: if s.italic { FontStyle::Italic } else { FontStyle::Normal },
        };
        let underline = (s.underline || run.link.is_some()).then(|| UnderlineStyle { thickness: px(1.), color: None, wavy: false });
        let strikethrough = s.strike.then(|| StrikethroughStyle { thickness: px(1.), color: None });
        // 宽字符段只有一个字符却占两格:强制宽度按「格数 ÷ 字符数」
        let per_char = self.cell_w * (run.cells as f32 / run.text.chars().count().max(1) as f32);
        // 把段按方块字符切开:方块字符单独画,其余连续文字各自按起始列排版
        let mut seg = String::new();
        let mut seg_col = run.col;
        let mut col = run.col;
        let flush = |seg: &mut String, seg_col: usize, window: &mut Window, cx: &mut App| {
            if seg.is_empty() {
                return;
            }
            let text = std::mem::take(seg);
            let tr = TextRun { len: text.len(), font: font.clone(), color: fg, background_color: None, underline: underline.clone(), strikethrough: strikethrough.clone() };
            let line = window.text_system().shape_line(text.into(), px(self.font_size), &[tr], Some(per_char));
            let _ = line.paint(point(self.x(origin, seg_col), self.y(origin, row)), px(self.line_h), TextAlign::Left, None, window, cx);
        };
        for ch in run.text.chars() {
            if block(ch).is_some() {
                flush(&mut seg, seg_col, window, cx);
                self.paint_block(origin, row, col, ch, fg, window);
                seg_col = col + 1;
            } else {
                if seg.is_empty() {
                    seg_col = col;
                }
                seg.push(ch);
            }
            col += if run.cells > run.text.chars().count() { 2 } else { 1 };
        }
        flush(&mut seg, seg_col, window, cx);
    }

    /// 画整个网格:背景 → 选区 / 搜索高亮 → 文字 → 光标。
    pub fn paint(&self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let origin = point(bounds.origin.x + px(self.pad), bounds.origin.y + px(self.pad));
        let resolved: Vec<Vec<(Hsla, Option<Hsla>)>> = self.snap.lines.iter().map(|l| l.iter().map(|r| resolve(r, self.fg, self.bg, self.dark)).collect()).collect();
        for (row, runs) in self.snap.lines.iter().enumerate() {
            for (r, (_, bg)) in runs.iter().zip(&resolved[row]) {
                if let Some(bg) = bg {
                    self.fill_cells(origin, row, r.col, r.cells, *bg, window);
                }
            }
        }
        for (row, from, to) in self.snap.selection.into_iter().flat_map(|sel| selection_spans(sel, self.snap.cols)) {
            self.fill_cells(origin, row, from, to - from + 1, self.accent.opacity(0.28), window);
        }
        for (row, col, cells, current) in &self.matches {
            self.fill_cells(origin, *row, *col, *cells, self.find.opacity(if *current { 0.75 } else { 0.35 }), window);
        }
        for (row, runs) in self.snap.lines.iter().enumerate() {
            for (r, (fg, _)) in runs.iter().zip(&resolved[row]) {
                self.paint_run(origin, row, r, *fg, window, cx);
            }
        }
        if let Some((row, col)) = self.snap.cursor {
            let (x, y) = (self.x(origin, col), self.y(origin, row));
            let (w, h) = (self.x(origin, col + 1) - x, self.y(origin, row + 1) - y);
            if !self.focused {
                // 失焦时统一画成空心块
                window.paint_quad(outline(Bounds::new(point(x, y), size(w, h)), self.accent, BorderStyle::default()));
            } else {
                let (b, color) = match self.cursor {
                    CursorShape::Block => (Bounds::new(point(x, y), size(w, h)), self.accent.opacity(0.6)),
                    CursorShape::Bar => (Bounds::new(point(x, y), size(px(2.), h)), self.accent.opacity(0.7)),
                    CursorShape::Underline => (Bounds::new(point(x, y + h - px(2.)), size(w, px(2.))), self.accent.opacity(0.7)),
                };
                window.paint_quad(fill(b, color));
            }
        }
    }
}
