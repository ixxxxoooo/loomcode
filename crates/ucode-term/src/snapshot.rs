//! 终端画面快照:把 alacritty 的网格转成界面好画的「按样式合并的文本段」。
//!
//! @author ixxxxoooo

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::{Term, TermMode, cell::Flags};
use alacritty_terminal::vte::ansi::Color;

/// 终端颜色。`Default` 由界面映射成主题的前景 / 背景色,`Indexed` 是 256 色表。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermColor {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// 文字样式。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    pub dim: bool,
    pub strike: bool,
}

/// 一段样式相同的连续文字。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub fg: TermColor,
    pub bg: TermColor,
    pub style: Style,
}

/// 某一时刻的画面。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Snapshot {
    pub cols: usize,
    pub rows: usize,
    /// 每行一组文字段,共 `rows` 行(行尾的默认空白已裁掉)
    pub lines: Vec<Vec<Run>>,
    /// 光标位置 (行, 列);光标隐藏或画面正在回看历史时为 `None`
    pub cursor: Option<(usize, usize)>,
    /// 程序设置的窗口标题
    pub title: String,
    /// shell 已退出时的退出码
    pub exit_code: Option<u32>,
    /// 画面正停在历史记录里(没有在最底部)
    pub scrolled_back: bool,
    /// 选区在当前画面里的范围 (起点行列, 终点行列),含两端;超出画面的部分已夹到边缘
    pub selection: Option<((usize, usize), (usize, usize))>,
}

impl Snapshot {
    /// 画面的纯文本(行以换行连接,行尾空白已去掉)。测试与复制用。
    pub fn text(&self) -> String {
        self.lines.iter().map(|l| l.iter().map(|r| r.text.as_str()).collect::<String>()).collect::<Vec<_>>().join("\n")
    }
}

/// 把 alacritty 的颜色转成 [`TermColor`];前景 / 背景 / 光标等「默认类」颜色归为 `Default`。
pub(crate) fn convert_color(c: Color) -> TermColor {
    match c {
        Color::Spec(rgb) => TermColor::Rgb(rgb.r, rgb.g, rgb.b),
        Color::Indexed(i) => TermColor::Indexed(i),
        Color::Named(n) => match n as usize {
            i @ 0..=15 => TermColor::Indexed(i as u8),
            // 暗色变体 DimBlack..DimWhite 当普通 8 色画
            i @ 259..=266 => TermColor::Indexed((i - 259) as u8),
            _ => TermColor::Default,
        },
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Cell {
    ch: char,
    fg: TermColor,
    bg: TermColor,
    style: Style,
}

const BLANK: Cell = Cell { ch: ' ', fg: TermColor::Default, bg: TermColor::Default, style: Style { bold: false, italic: false, underline: false, inverse: false, dim: false, strike: false } };

/// 由终端状态生成快照。`'\0'` 用来标记宽字符占位格,合并时跳过。
pub(crate) fn build<T: EventListener>(term: &Term<T>, title: String, exit_code: Option<u32>) -> Snapshot {
    let (rows, cols) = (term.screen_lines(), term.columns());
    let content = term.renderable_content();
    let offset = content.display_offset as i32;
    let mut grid = vec![vec![BLANK; cols]; rows];
    for item in content.display_iter {
        let (row, col) = (item.point.line.0 + offset, item.point.column.0);
        if row < 0 || row as usize >= rows || col >= cols {
            continue;
        }
        let f = item.cell.flags;
        grid[row as usize][col] = if f.contains(Flags::WIDE_CHAR_SPACER) || f.contains(Flags::LEADING_WIDE_CHAR_SPACER) {
            Cell { ch: '\0', ..BLANK }
        } else {
            Cell {
                ch: if f.contains(Flags::HIDDEN) { ' ' } else { item.cell.c },
                fg: convert_color(item.cell.fg),
                bg: convert_color(item.cell.bg),
                style: Style {
                    bold: f.intersects(Flags::BOLD),
                    italic: f.intersects(Flags::ITALIC),
                    underline: f.intersects(Flags::ALL_UNDERLINES),
                    inverse: f.intersects(Flags::INVERSE),
                    dim: f.intersects(Flags::DIM),
                    strike: f.intersects(Flags::STRIKEOUT),
                },
            }
        };
    }
    let lines = grid.into_iter().map(merge_runs).collect();
    let at_bottom = content.display_offset == 0;
    let cursor_visible = content.mode.contains(TermMode::SHOW_CURSOR) && at_bottom;
    let cursor = cursor_visible.then(|| (content.cursor.point.line.0.max(0) as usize, content.cursor.point.column.0)).filter(|(r, c)| *r < rows && *c < cols);
    let selection = content.selection.and_then(|r| {
        let (top, bottom) = (r.start.line.0 + offset, r.end.line.0 + offset);
        if bottom < 0 || top >= rows as i32 {
            return None;
        }
        let start = if top < 0 { (0, 0) } else { (top as usize, r.start.column.0.min(cols - 1)) };
        let end = if bottom >= rows as i32 { (rows - 1, cols - 1) } else { (bottom as usize, r.end.column.0.min(cols - 1)) };
        Some((start, end))
    });
    Snapshot { cols, rows, lines, cursor, title, exit_code, scrolled_back: !at_bottom, selection }
}

/// 行尾可以裁掉的格子:宽字符占位,或没有背景色、没有反色的空格。
fn is_trailing_blank(c: &Cell) -> bool {
    c.ch == '\0' || (c.ch == ' ' && c.bg == TermColor::Default && !c.style.inverse)
}

/// 把一行格子合并成样式相同的文字段,并裁掉行尾的默认空白。
fn merge_runs(cells: Vec<Cell>) -> Vec<Run> {
    let end = cells.iter().rposition(|c| !is_trailing_blank(c)).map_or(0, |i| i + 1);
    let mut runs: Vec<Run> = Vec::new();
    for c in cells.into_iter().take(end).filter(|c| c.ch != '\0') {
        match runs.last_mut() {
            Some(r) if r.fg == c.fg && r.bg == c.bg && r.style == c.style => r.text.push(c.ch),
            _ => runs.push(Run { text: c.ch.to_string(), fg: c.fg, bg: c.bg, style: c.style }),
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::vte::ansi::Processor;

    struct Size(usize, usize);
    impl Dimensions for Size {
        fn total_lines(&self) -> usize {
            self.1
        }
        fn screen_lines(&self) -> usize {
            self.1
        }
        fn columns(&self) -> usize {
            self.0
        }
    }

    fn feed(cols: usize, rows: usize, input: &str) -> Snapshot {
        let mut term = Term::new(Config::default(), &Size(cols, rows), VoidListener);
        let mut parser: Processor = Processor::default();
        parser.advance(&mut term, input.as_bytes());
        build(&term, String::new(), None)
    }

    #[test]
    fn 纯文本与换行() {
        let s = feed(20, 3, "hello\r\nworld");
        assert_eq!(s.text(), "hello\nworld\n");
        assert_eq!(s.cursor, Some((1, 5)));
    }

    #[test]
    fn 颜色与样式分段() {
        // 红色粗体 "ab",再恢复默认 "c"
        let s = feed(20, 2, "\x1b[1;31mab\x1b[0mc");
        let runs = &s.lines[0];
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].text, "ab");
        assert_eq!(runs[0].fg, TermColor::Indexed(1));
        assert!(runs[0].style.bold);
        assert_eq!(runs[1].text, "c");
        assert_eq!(runs[1].fg, TermColor::Default);
    }

    #[test]
    fn 真彩色与_256_色() {
        let s = feed(20, 1, "\x1b[38;2;10;20;30mx\x1b[48;5;200my");
        assert_eq!(s.lines[0][0].fg, TermColor::Rgb(10, 20, 30));
        assert_eq!(s.lines[0][1].bg, TermColor::Indexed(200));
    }

    #[test]
    fn 宽字符不产生占位字符() {
        let s = feed(10, 1, "中a");
        assert_eq!(s.text(), "中a");
    }

    /// 先按 80 列输出,再缩窄并加高:重排后每一行内容都必须还在画面里(只会多折行,不能被顶进历史)。
    #[test]
    fn 缩窄重排不丢内容() {
        let mut term = Term::new(Config::default(), &Size(80, 24), VoidListener);
        let mut parser: Processor = Processor::default();
        let rows: Vec<String> = (0..8).map(|i| format!("row{i}-{}", "x".repeat(50))).collect();
        parser.advance(&mut term, format!("{}\r\n", rows.join("\r\n")).as_bytes());
        crate::terminal::resize_term(&mut term, 40, 60);
        let text = build(&term, String::new(), None).text();
        let flat: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        for r in &rows {
            assert!(flat.contains(r.as_str()), "丢了 {r},画面:{text:?}");
        }
    }

    #[test]
    fn 清屏与光标定位() {
        let s = feed(10, 3, "xxx\x1b[2J\x1b[2;3Hhi");
        assert_eq!(s.lines[0], vec![]);
        assert_eq!(s.text(), "\n  hi\n");
    }
}
