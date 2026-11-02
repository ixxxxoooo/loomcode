//! 终端的鼠标处理:程序开了鼠标模式就上报给它,否则选择文字、⌘ 点击打开链接、滚轮回看。
//!
//! @author ixxxxoooo

use crate::term_links::{LinkTarget, OpenTermPath, find_at, resolve_path};
use crate::terminal_view::TerminalView;
use gpui_kit::*;
use loom_term::{Mods, MouseButton as TermButton, MouseKind, Run};

/// 单次滚轮事件最多上报的行数,防止惯性滚动一次刷出大量事件。
const MAX_WHEEL_LINES: i32 = 12;

fn mods_of(m: &Modifiers) -> Mods {
    Mods { ctrl: m.control, alt: m.alt, shift: m.shift }
}

fn term_button(b: MouseButton) -> Option<TermButton> {
    match b {
        MouseButton::Left => Some(TermButton::Left),
        MouseButton::Middle => Some(TermButton::Middle),
        MouseButton::Right => Some(TermButton::Right),
        _ => None,
    }
}

/// 一行按列展开成字符(宽字符后面补 `'\0'`),用来在文字里找路径与网址。
fn row_chars(runs: &[Run], cols: usize) -> Vec<char> {
    let mut row = vec![' '; cols];
    for r in runs {
        let mut col = r.col;
        let wide = r.cells > r.text.chars().count();
        for ch in r.text.chars() {
            if col < cols {
                row[col] = ch;
            }
            if wide && col + 1 < cols {
                row[col + 1] = '\0';
            }
            col += if wide { 2 } else { 1 };
        }
    }
    row
}

impl TerminalView {
    /// 鼠标按下。程序接管鼠标时(按住 Shift 可强制选择文字)上报给它;否则左键 ⌘ 点链接、其余选择文字。
    pub(crate) fn mouse_down(&mut self, button: MouseButton, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let (Some(tb), Some(c)) = (term_button(button), self.cell_under(ev.position)) else { return };
        if self.term.mouse_mode().any() && !ev.modifiers.shift {
            if self.term.report_mouse(MouseKind::Down(tb), c.col, c.row, mods_of(&ev.modifiers)) {
                self.reported = Some(tb);
                self.last_cell = Some((c.row, c.col));
            }
            return;
        }
        if tb == TermButton::Left {
            if ev.modifiers.platform && self.open_link_at(c.row, c.col, window, cx) {
                return;
            }
            self.begin_select(ev, cx);
        }
    }

    /// 鼠标松开。
    pub(crate) fn mouse_up(&mut self, button: MouseButton, ev: &MouseUpEvent, cx: &mut Context<Self>) {
        if let Some(tb) = self.reported.take() {
            if let Some(c) = self.cell_under(ev.position) {
                self.term.report_mouse(MouseKind::Up(tb), c.col, c.row, mods_of(&ev.modifiers));
            }
            self.last_cell = None;
            return;
        }
        if button == MouseButton::Left {
            self.end_select(ev, cx);
        }
    }

    /// 鼠标移动:上报拖动 / 移动(同一格内不重复发),否则更新选区。
    pub(crate) fn mouse_move(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        let mode = self.term.mouse_mode();
        if mode.any() && !ev.modifiers.shift {
            let Some(c) = self.cell_under(ev.position) else { return };
            if self.last_cell == Some((c.row, c.col)) {
                return;
            }
            self.last_cell = Some((c.row, c.col));
            let kind = MouseKind::Move(self.reported);
            self.term.report_mouse(kind, c.col, c.row, mods_of(&ev.modifiers));
            return;
        }
        self.drag_select(ev, cx);
    }

    /// 滚轮:程序接管鼠标就上报滚轮;备用屏幕(vim、less)下换成方向键;否则回看历史。
    pub(crate) fn on_wheel(&mut self, ev: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let line_h = self.metrics.1;
        self.wheel_acc += f32::from(ev.delta.pixel_delta(px(line_h)).y);
        let lines = (self.wheel_acc / line_h).trunc();
        cx.stop_propagation();
        if lines == 0. {
            return;
        }
        self.wheel_acc -= lines * line_h;
        let (mode, up, n) = (self.term.mouse_mode(), lines > 0., (lines.abs() as i32).min(MAX_WHEEL_LINES));
        if !ev.modifiers.shift && mode.any() {
            if let Some(c) = self.cell_under(ev.position) {
                for _ in 0..n {
                    self.term.report_mouse(MouseKind::Wheel(up), c.col, c.row, mods_of(&ev.modifiers));
                }
            }
            return;
        }
        if !ev.modifiers.shift && mode.alt_screen && mode.alt_scroll {
            let key: &[u8] = match (up, self.term.app_cursor()) {
                (true, true) => b"\x1bOA",
                (true, false) => b"\x1b[A",
                (false, true) => b"\x1bOB",
                (false, false) => b"\x1b[B",
            };
            for _ in 0..n {
                self.term.write(key);
            }
            return;
        }
        self.term.scroll(lines as i32);
        cx.notify();
    }

    /// 窗口焦点进出时,程序开了焦点上报(模式 1004)就告诉它。
    pub(crate) fn sync_focus_report(&mut self, focused: bool) {
        if focused != self.last_focus {
            self.last_focus = focused;
            if self.term.mouse_mode().focus {
                self.term.write(if focused { b"\x1b[I" } else { b"\x1b[O" });
            }
        }
    }

    /// ⌘ 点击:打开 (行, 列) 处的链接(OSC 8 超链接、网址、存在的文件路径)。打开了返回 `true`。
    pub(crate) fn open_link_at(&mut self, row: usize, col: usize, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let snap = self.term.snapshot();
        let Some(runs) = snap.lines.get(row) else { return false };
        if let Some(uri) = runs.iter().find(|r| r.link.is_some() && col >= r.col && col < r.col + r.cells).and_then(|r| r.link.clone()) {
            cx.open_url(&uri);
            return true;
        }
        match find_at(&row_chars(runs, snap.cols), col).map(|x| x.0) {
            Some(LinkTarget::Url(u)) => {
                cx.open_url(&u);
                true
            }
            Some(LinkTarget::Path { path, line, .. }) => match resolve_path(&path, &self.cwd) {
                Some(full) => {
                    window.dispatch_action(Box::new(OpenTermPath { path: full.to_string_lossy().to_string(), line }), cx);
                    true
                }
                None => false,
            },
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_term::{Style, TermColor};

    fn run(col: usize, cells: usize, text: &str) -> Run {
        Run { col, cells, link: None, text: text.into(), fg: TermColor::Default, bg: TermColor::Default, style: Style::default() }
    }

    #[::core::prelude::v1::test]
    fn 行展开成按列的字符_宽字符补占位() {
        let row = row_chars(&[run(0, 1, "a"), run(1, 2, "中"), run(3, 2, "bc")], 6);
        assert_eq!(row, vec!['a', '中', '\0', 'b', 'c', ' ']);
    }
}
