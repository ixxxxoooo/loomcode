//! ANSI 转义序列解析:把带颜色的终端输出转换成文本加高亮区间。

use gpui_kit::*;
use std::ops::Range;

/// 解析结果:纯文本与对应的样式区间。
pub struct Ansi {
    pub text: String,
    pub runs: Vec<(Range<usize>, HighlightStyle)>,
}

/// 基础 16 色(浅色与深色背景下都可读)。
fn palette(n: u8, bright: bool) -> Hsla {
    let v = match (n, bright) {
        (0, false) => 0x555555,
        (1, false) => 0xD14B4B,
        (2, false) => 0x2F9E5B,
        (3, false) => 0xB8860B,
        (4, false) => 0x3B7DD8,
        (5, false) => 0xB04FB0,
        (6, false) => 0x1E9CA8,
        (7, false) => 0xAAAAAA,
        (0, true) => 0x808080,
        (1, true) => 0xF07070,
        (2, true) => 0x4FC47E,
        (3, true) => 0xE0B04A,
        (4, true) => 0x6FA8F0,
        (5, true) => 0xD080D0,
        (6, true) => 0x4FC4D0,
        _ => 0xDDDDDD,
    };
    rgb(v).into()
}

/// 解析 ANSI 文本(支持 SGR:重置、粗体、前景 30-37/90-97)。
pub fn parse(input: &str) -> Ansi {
    let mut text = String::with_capacity(input.len());
    let mut runs: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
    let mut color: Option<Hsla> = None;
    let mut bold = false;
    let mut run_start = 0usize;
    let mut chars = input.chars().peekable();

    // 在样式变化前,把当前区间落盘
    let flush = |text: &String, start: usize, color: Option<Hsla>, bold: bool, runs: &mut Vec<(Range<usize>, HighlightStyle)>| {
        if text.len() > start && (color.is_some() || bold) {
            runs.push((
                start..text.len(),
                HighlightStyle {
                    color,
                    font_weight: if bold { Some(FontWeight::MEDIUM) } else { None },
                    ..Default::default()
                },
            ));
        }
    };

    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            let mut params = String::new();
            let mut end = ' ';
            for p in chars.by_ref() {
                if p.is_ascii_alphabetic() {
                    end = p;
                    break;
                }
                params.push(p);
            }
            if end != 'm' {
                continue;
            }
            flush(&text, run_start, color, bold, &mut runs);
            run_start = text.len();
            let codes: Vec<u8> = if params.is_empty() {
                vec![0]
            } else {
                params.split(';').filter_map(|s| s.parse().ok()).collect()
            };
            for code in codes {
                match code {
                    0 => {
                        color = None;
                        bold = false;
                    }
                    1 => bold = true,
                    22 => bold = false,
                    39 => color = None,
                    30..=37 => color = Some(palette(code - 30, false)),
                    90..=97 => color = Some(palette(code - 90, true)),
                    _ => {}
                }
            }
        } else {
            text.push(c);
        }
    }
    flush(&text, run_start, color, bold, &mut runs);
    Ansi { text, runs }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 解析颜色与重置() {
        let a = parse("\u{1b}[31mERR\u{1b}[0m ok");
        assert_eq!(a.text, "ERR ok");
        assert_eq!(a.runs.len(), 1);
        assert_eq!(a.runs[0].0, 0..3);
    }

    #[::core::prelude::v1::test]
    fn 无转义的文本原样返回() {
        let a = parse("plain");
        assert_eq!(a.text, "plain");
        assert!(a.runs.is_empty());
    }
}
