//! 鼠标上报:程序(vim、htop、opencode 等)开启鼠标模式后,把鼠标事件按 xterm 协议编码写给它。
//!
//! @author ixxxxoooo

use crate::keys::Mods;
use alacritty_terminal::term::TermMode;

/// 鼠标按键。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

/// 鼠标事件种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseKind {
    Down(MouseButton),
    Up(MouseButton),
    /// 移动;按着某个键时是拖动
    Move(Option<MouseButton>),
    /// 滚轮,`true` 向上
    Wheel(bool),
}

/// 程序当前开启的鼠标相关模式。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MouseMode {
    /// 上报点击(含松开与滚轮)
    pub click: bool,
    /// 上报按住拖动
    pub drag: bool,
    /// 上报所有移动
    pub motion: bool,
    /// 用 SGR 扩展格式
    pub sgr: bool,
    /// 在备用屏幕
    pub alt_screen: bool,
    /// 备用屏幕下滚轮换成方向键
    pub alt_scroll: bool,
    /// 上报窗口焦点进出
    pub focus: bool,
}

impl MouseMode {
    pub(crate) fn from_term(m: TermMode) -> Self {
        Self {
            click: m.contains(TermMode::MOUSE_REPORT_CLICK),
            drag: m.contains(TermMode::MOUSE_DRAG),
            motion: m.contains(TermMode::MOUSE_MOTION),
            sgr: m.contains(TermMode::SGR_MOUSE),
            alt_screen: m.contains(TermMode::ALT_SCREEN),
            alt_scroll: m.contains(TermMode::ALTERNATE_SCROLL),
            focus: m.contains(TermMode::FOCUS_IN_OUT),
        }
    }

    /// 程序是否要接管鼠标(任一鼠标上报模式开启)。
    pub fn any(&self) -> bool {
        self.click || self.drag || self.motion
    }
}

/// 把鼠标事件编码成要写给程序的字节;当前模式不需要这类事件、或坐标超出传统格式范围时返回 `None`。
///
/// 参数:`col` / `row` 为 0 起始的格子坐标;`mods` 修饰键。
pub fn encode(mode: MouseMode, kind: MouseKind, col: usize, row: usize, mods: Mods) -> Option<Vec<u8>> {
    let wanted = match kind {
        MouseKind::Down(_) | MouseKind::Up(_) | MouseKind::Wheel(_) => mode.any(),
        MouseKind::Move(Some(_)) => mode.drag || mode.motion,
        MouseKind::Move(None) => mode.motion,
    };
    if !wanted {
        return None;
    }
    let btn = |b: MouseButton| match b {
        MouseButton::Left => 0u32,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    };
    let mut code = match kind {
        MouseKind::Down(b) => btn(b),
        // 传统格式不区分松开的是哪个键,统一为 3;SGR 保留原键并用 `m` 结尾
        MouseKind::Up(b) => if mode.sgr { btn(b) } else { 3 },
        MouseKind::Move(Some(b)) => btn(b) + 32,
        MouseKind::Move(None) => 3 + 32,
        MouseKind::Wheel(up) => if up { 64 } else { 65 },
    };
    if mods.shift {
        code += 4;
    }
    if mods.alt {
        code += 8;
    }
    if mods.ctrl {
        code += 16;
    }
    let (x, y) = (col + 1, row + 1);
    if mode.sgr {
        let end = if matches!(kind, MouseKind::Up(_)) { 'm' } else { 'M' };
        return Some(format!("\x1b[<{code};{x};{y}{end}").into_bytes());
    }
    // 传统格式:坐标加 32 后必须落在一个字节里
    if x + 32 > 255 || y + 32 > 255 {
        return None;
    }
    Some(vec![0x1b, b'[', b'M', (code + 32) as u8, (x + 32) as u8, (y + 32) as u8])
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Mods = Mods { ctrl: false, alt: false, shift: false };
    fn mode(sgr: bool) -> MouseMode {
        MouseMode { click: true, sgr, ..Default::default() }
    }

    #[test]
    fn sgr_点击与松开() {
        let down = encode(mode(true), MouseKind::Down(MouseButton::Left), 4, 9, NONE).unwrap();
        assert_eq!(down, b"\x1b[<0;5;10M");
        let up = encode(mode(true), MouseKind::Up(MouseButton::Right), 4, 9, NONE).unwrap();
        assert_eq!(up, b"\x1b[<2;5;10m");
    }

    #[test]
    fn 传统格式与滚轮() {
        let d = encode(mode(false), MouseKind::Down(MouseButton::Left), 0, 0, NONE).unwrap();
        assert_eq!(d, vec![0x1b, b'[', b'M', 32, 33, 33]);
        let w = encode(mode(true), MouseKind::Wheel(true), 1, 1, NONE).unwrap();
        assert_eq!(w, b"\x1b[<64;2;2M");
    }

    #[test]
    fn 修饰键与超范围坐标() {
        let m = Mods { ctrl: true, alt: false, shift: true };
        assert_eq!(encode(mode(true), MouseKind::Down(MouseButton::Left), 0, 0, m).unwrap(), b"\x1b[<20;1;1M");
        assert!(encode(mode(false), MouseKind::Down(MouseButton::Left), 300, 0, NONE).is_none());
    }

    #[test]
    fn 模式不需要时不上报() {
        let only_click = mode(true);
        assert!(encode(only_click, MouseKind::Move(None), 0, 0, NONE).is_none());
        assert!(encode(only_click, MouseKind::Move(Some(MouseButton::Left)), 0, 0, NONE).is_none());
        assert!(encode(MouseMode::default(), MouseKind::Down(MouseButton::Left), 0, 0, NONE).is_none());
        let drag = MouseMode { drag: true, sgr: true, ..Default::default() };
        assert!(encode(drag, MouseKind::Move(Some(MouseButton::Left)), 0, 0, NONE).is_some());
    }
}
