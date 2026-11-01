//! 把按键翻译成发给 shell 的字节(xterm 风格)。纯函数。
//!
//! @author ixxxxoooo

/// 修饰键。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Mods {
    /// xterm 的修饰键参数:1 + shift(1) + alt(2) + ctrl(4);没有修饰键返回 `None`。
    fn param(self) -> Option<u8> {
        let v = self.shift as u8 + (self.alt as u8) * 2 + (self.ctrl as u8) * 4;
        (v > 0).then_some(v + 1)
    }
}

/// 编码一个按键。
///
/// 参数:
/// - `key` 按键名:`enter` `escape` `tab` `backspace` `delete` `up` `down` `left` `right`
///   `home` `end` `pageup` `pagedown` `space`,或单个字符;
/// - `mods` 修饰键;
/// - `app_cursor` 程序是否开启了「应用光标键模式」(vim、less 等会开),此时方向键用 `ESC O x`。
///
/// 返回:要写进终端的字节;普通可打印字符(没有 Ctrl / Alt)返回 `None`,
/// 由调用方直接发送输入法给出的文本。
pub fn encode_key(key: &str, mods: Mods, app_cursor: bool) -> Option<Vec<u8>> {
    let named = |code: &str| -> Vec<u8> {
        match mods.param() {
            Some(p) => format!("\x1b[1;{p}{code}").into_bytes(),
            None if app_cursor => format!("\x1bO{code}").into_bytes(),
            None => format!("\x1b[{code}").into_bytes(),
        }
    };
    let tilde = |n: u8| -> Vec<u8> {
        match mods.param() {
            Some(p) => format!("\x1b[{n};{p}~").into_bytes(),
            None => format!("\x1b[{n}~").into_bytes(),
        }
    };
    let bytes = match key {
        "enter" => b"\r".to_vec(),
        "escape" => vec![0x1b],
        "tab" if mods.shift => b"\x1b[Z".to_vec(),
        "tab" => b"\t".to_vec(),
        "backspace" => vec![0x7f],
        "up" => named("A"),
        "down" => named("B"),
        "right" => named("C"),
        "left" => named("D"),
        "home" => named("H"),
        "end" => named("F"),
        "delete" => tilde(3),
        "pageup" => tilde(5),
        "pagedown" => tilde(6),
        "space" if mods.ctrl => vec![0],
        "space" => b" ".to_vec(),
        k => {
            let mut chars = k.chars();
            let (c, None) = (chars.next()?, chars.next()) else { return None };
            if mods.ctrl {
                match c.to_ascii_lowercase() {
                    l @ 'a'..='z' => vec![l as u8 - b'a' + 1],
                    '@' | ' ' => vec![0],
                    '[' => vec![0x1b],
                    '\\' => vec![0x1c],
                    ']' => vec![0x1d],
                    '_' | '/' => vec![0x1f],
                    _ => return None,
                }
            } else if mods.alt {
                let mut v = vec![0x1b];
                v.extend(c.to_string().into_bytes());
                return Some(v);
            } else {
                return None;
            }
        }
    };
    // Alt 前缀 ESC(方向键等已经把修饰键编进序列里了)
    let in_sequence = matches!(key, "up" | "down" | "left" | "right" | "home" | "end" | "delete" | "pageup" | "pagedown");
    if mods.alt && !in_sequence && key.chars().count() != 1 {
        let mut v = vec![0x1b];
        v.extend(bytes);
        return Some(v);
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Mods = Mods { ctrl: false, alt: false, shift: false };
    const CTRL: Mods = Mods { ctrl: true, alt: false, shift: false };
    const ALT: Mods = Mods { ctrl: false, alt: true, shift: false };

    #[test]
    fn 方向键区分普通与应用模式() {
        assert_eq!(encode_key("up", NONE, false).unwrap(), b"\x1b[A");
        assert_eq!(encode_key("up", NONE, true).unwrap(), b"\x1bOA");
        assert_eq!(encode_key("left", CTRL, false).unwrap(), b"\x1b[1;5D");
        assert_eq!(encode_key("right", Mods { shift: true, ..NONE }, true).unwrap(), b"\x1b[1;2C");
    }

    #[test]
    fn 功能键() {
        assert_eq!(encode_key("enter", NONE, false).unwrap(), b"\r");
        assert_eq!(encode_key("backspace", NONE, false).unwrap(), vec![0x7f]);
        assert_eq!(encode_key("tab", Mods { shift: true, ..NONE }, false).unwrap(), b"\x1b[Z");
        assert_eq!(encode_key("delete", NONE, false).unwrap(), b"\x1b[3~");
        assert_eq!(encode_key("pageup", NONE, false).unwrap(), b"\x1b[5~");
        assert_eq!(encode_key("escape", NONE, false).unwrap(), vec![0x1b]);
    }

    #[test]
    fn ctrl_字母与_alt_前缀() {
        assert_eq!(encode_key("c", CTRL, false).unwrap(), vec![3]);
        assert_eq!(encode_key("A", CTRL, false).unwrap(), vec![1]);
        assert_eq!(encode_key("space", CTRL, false).unwrap(), vec![0]);
        assert_eq!(encode_key("[", CTRL, false).unwrap(), vec![0x1b]);
        assert_eq!(encode_key("b", ALT, false).unwrap(), vec![0x1b, b'b']);
        assert_eq!(encode_key("backspace", ALT, false).unwrap(), vec![0x1b, 0x7f]);
    }

    #[test]
    fn 普通字符交给调用方() {
        assert_eq!(encode_key("a", NONE, false), None);
        assert_eq!(encode_key("é", NONE, false), None);
        assert_eq!(encode_key("unknown-key", NONE, false), None);
        // Ctrl + 无对应控制码的字符
        assert_eq!(encode_key("1", CTRL, false), None);
    }
}
