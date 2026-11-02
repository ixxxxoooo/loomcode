//! 结果行的预览文字:去掉行首缩进、匹配离行首太远时前面补「…」、限制长度。
//!
//! @author ixxxxoooo

use std::ops::Range;

/// 预览文字显示的最大字符数。
const MAX_CHARS: usize = 200;
/// 匹配前最多保留的上下文字符数(更早的用「…」代替)。
const LEAD_CHARS: usize = 24;

/// 一行结果的预览:`text` 已去缩进 / 截断,`hl` 是匹配在 `text` 里的字节区间。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    pub text: String,
    pub hl: Range<usize>,
}

/// 生成预览。
///
/// 参数:`line` 原始行(不含换行);`start` / `end` 匹配的字符列区间(0 起始,左闭右开)。
/// 返回:预览。
pub(crate) fn make_preview(line: &str, start: usize, end: usize) -> Preview {
    let chars: Vec<char> = line.chars().collect();
    let indent = chars.iter().take_while(|c| c.is_whitespace()).count().min(start);
    let begin = indent.max(start.saturating_sub(LEAD_CHARS));
    let (hit_start, hit_end) = (start.min(chars.len()), end.min(chars.len()).max(start.min(chars.len())));
    let stop = (begin + MAX_CHARS).min(chars.len()).max(hit_end);
    let mut text = String::new();
    if begin > indent {
        text.push('…');
    }
    let lead_bytes = text.len();
    let before: String = chars[begin.min(hit_start)..hit_start].iter().collect();
    let hit: String = chars[hit_start..hit_end].iter().collect();
    let after: String = chars[hit_end..stop].iter().collect();
    let hl_start = lead_bytes + before.len();
    text.push_str(&before);
    text.push_str(&hit);
    text.push_str(&after);
    if stop < chars.len() {
        text.push('…');
    }
    Preview { text, hl: hl_start..hl_start + hit.len() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 去掉行首缩进并标出匹配() {
        let p = make_preview("    let foo = 1;", 8, 11);
        assert_eq!(p.text, "let foo = 1;");
        assert_eq!(&p.text[p.hl.clone()], "foo");
    }

    #[test]
    fn 匹配太靠后时前面补省略号() {
        let line = format!("{}needle tail", "x".repeat(80));
        let p = make_preview(&line, 80, 86);
        assert!(p.text.starts_with('…'));
        assert_eq!(&p.text[p.hl.clone()], "needle");
    }

    #[test]
    fn 超长行被截断且高亮不越界() {
        let line = format!("needle{}", "y".repeat(1000));
        let p = make_preview(&line, 0, 6);
        assert!(p.text.ends_with('…') && p.hl.end <= p.text.len());
        let p = make_preview("中文关键字测试", 2, 5);
        assert_eq!(&p.text[p.hl.clone()], "关键字");
    }
}
