//! 代码文本的语法高亮:把一段文本按语言算出各段样式,供 `StyledText::with_highlights` 使用。
//!
//! @author ixxxxoooo

use gpui_kit::component::highlighter::SyntaxHighlighter;
use gpui_kit::component::{ActiveTheme, Rope};
use gpui_kit::*;
use std::ops::Range;

/// 高亮文本过大时不再着色,避免卡顿。
const MAX_BYTES: usize = 200_000;

/// 缓存条目上限(超过就清空重来,简单且够用)。
const CACHE_CAP: usize = 512;

type Styles = std::sync::Arc<Vec<(Range<usize>, HighlightStyle)>>;

thread_local! {
    /// (语言, 主题指针, 文本哈希) → 高亮结果。对话里的工具卡片、差异块每帧都会重绘,
    /// 新建 `SyntaxHighlighter` 要编译语法查询(毫秒级),不缓存的话回合进行中(持续出帧)界面会明显卡顿、动画不流畅。
    static CACHE: std::cell::RefCell<std::collections::HashMap<(String, usize, u64), Styles>> = Default::default();
}

fn hash_of(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// 计算语法高亮样式(带缓存:同一语言、同一主题、同一文本只算一次)。
///
/// 参数:`text` 文本;`lang` 语言名(`text` 或空表示不着色);`cx` 取语法主题。
/// 返回:按位置排序的 (字节区间, 样式);不支持的语言或文本过大返回空。
pub fn styles(text: &str, lang: &str, cx: &App) -> Vec<(Range<usize>, HighlightStyle)> {
    if lang.is_empty() || lang == "text" || text.is_empty() || text.len() > MAX_BYTES {
        return Vec::new();
    }
    let theme = cx.theme().highlight_theme.clone();
    let key = (lang.to_string(), std::sync::Arc::as_ptr(&theme) as *const () as usize, hash_of(text));
    if let Some(hit) = CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return hit.as_ref().clone();
    }
    let mut hl = SyntaxHighlighter::new(lang);
    hl.update(None, &Rope::from(text), Some(std::time::Duration::from_millis(80)));
    let out: Styles = std::sync::Arc::new(hl.styles(&(0..text.len()), &*theme));
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() >= CACHE_CAP {
            c.clear();
        }
        c.insert(key, out.clone());
    });
    out.as_ref().clone()
}

/// 带 bash 语法高亮的命令文本,可在前面加一段不着色的前缀(如提示符 `$ `)。
///
/// 参数:`cmd` 命令(整段,多行也行);`prefix` 前缀;`cx` 取语法主题。返回:可直接 `.child(..)` 的带样式文本。
pub fn shell_text(cmd: &str, prefix: &str, cx: &App) -> StyledText {
    let shifted: Vec<(Range<usize>, HighlightStyle)> = styles(cmd, "bash", cx).into_iter().map(|(r, s)| (r.start + prefix.len()..r.end + prefix.len(), s)).collect();
    StyledText::new(format!("{prefix}{cmd}")).with_highlights(shifted)
}

/// 把整段文本的高亮样式按行切开:`lines` 是按 `\n` 拼成该文本的各行,返回每行一组相对行首的样式。
///
/// 参数:`lines` 各行文本;`styles` 整段文本上的样式(字节区间)。
pub fn split_by_line(lines: &[&str], styles: &[(Range<usize>, HighlightStyle)]) -> Vec<Vec<(Range<usize>, HighlightStyle)>> {
    let mut out = vec![Vec::new(); lines.len()];
    let mut start = 0;
    for (i, l) in lines.iter().enumerate() {
        let end = start + l.len();
        for (r, st) in styles {
            let (a, b) = (r.start.max(start), r.end.min(end));
            if a < b {
                out[i].push((a - start..b - start, *st));
            }
        }
        start = end + 1;
    }
    out
}

/// 在已有的语法高亮上叠一段背景(如引用里的命中词):把与 `range` 重叠的样式切开,重叠部分加上背景色。
///
/// 参数:`base` 按位置排序且互不重叠的样式;`range` 要加背景的区间;`bg` 背景色。返回:仍然有序且互不重叠的样式。
pub fn overlay_background(base: &[(Range<usize>, HighlightStyle)], range: Range<usize>, bg: Hsla) -> Vec<(Range<usize>, HighlightStyle)> {
    if range.is_empty() {
        return base.to_vec();
    }
    let mut out = Vec::new();
    let mut cursor = range.start;
    let mut placed = false;
    for (r, st) in base {
        if r.end <= range.start || r.start >= range.end {
            if r.start >= range.end && !placed {
                out.push((range.clone(), HighlightStyle { background_color: Some(bg), ..Default::default() }));
                placed = true;
            }
            out.push((r.clone(), *st));
            continue;
        }
        if r.start < range.start {
            out.push((r.start..range.start, *st));
        }
        // 命中区间里没被样式覆盖的空隙补上纯背景
        if r.start > cursor {
            out.push((cursor..r.start, HighlightStyle { background_color: Some(bg), ..Default::default() }));
        }
        let (a, b) = (r.start.max(range.start), r.end.min(range.end));
        out.push((a..b, HighlightStyle { background_color: Some(bg), ..*st }));
        cursor = b;
        if r.end > range.end {
            out.push((range.end..r.end, *st));
            cursor = range.end;
        }
        placed = true;
    }
    if cursor < range.end && !placed {
        out.push((range.clone(), HighlightStyle { background_color: Some(bg), ..Default::default() }));
    } else if cursor < range.end {
        out.push((cursor..range.end, HighlightStyle { background_color: Some(bg), ..Default::default() }));
    }
    out.sort_by_key(|(r, _)| r.start);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hs() -> HighlightStyle {
        HighlightStyle { color: Some(hsla(0.1, 0.5, 0.5, 1.)), ..Default::default() }
    }

    #[::core::prelude::v1::test]
    fn 叠背景后仍有序且不重叠() {
        let bg = hsla(0.6, 1., 0.5, 0.3);
        let base = vec![(0..3, hs()), (5..9, hs())];
        let out = overlay_background(&base, 2..7, bg);
        assert!(out.windows(2).all(|w| w[0].0.end <= w[1].0.start), "{out:?}");
        let covered: usize = out.iter().filter(|(_, s)| s.background_color == Some(bg)).map(|(r, _)| r.len()).sum();
        assert_eq!(covered, 5);
        // 没有样式的文本也能加背景
        let out = overlay_background(&[], 1..4, bg);
        assert_eq!(out.len(), 1);
    }

    #[::core::prelude::v1::test]
    fn 按行切开样式() {
        let lines = ["ab", "cde"];
        let out = split_by_line(&lines, &[(1..4, hs())]);
        assert_eq!(out[0][0].0, 1..2);
        assert_eq!(out[1][0].0, 0..1);
    }
}
