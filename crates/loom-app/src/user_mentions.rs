//! 用户消息卡里的文件引用:输入框里的 `@路径` 是带图标的胶囊,发出去后正文里只剩文本,
//! 这里把正文里的 `@路径` 重新认出来,渲染成同款胶囊,和文字一起折行排版。
//!
//! 切分规则(纯函数 [`segments`],有测试):`@` 必须在行首或空白之后;路径到空白或全角标点为止;
//! 路径里要有 `/` 或 `.`(`@张三` 这类普通提及不当文件);末尾的半角标点不算路径。
//!
//! @author ixxxxoooo

use crate::file_icons::file_icon_img;
use gpui_kit::component::{ActiveTheme, h_flex};
use gpui_kit::*;
use std::path::Path;

/// 正文切出来的一段。
#[derive(Debug, PartialEq, Eq)]
pub enum Seg<'a> {
    /// 普通文字。
    Text(&'a str),
    /// 文件引用,值是 `@` 后面的路径。
    Mention(&'a str),
}

/// 路径的终止符:全角标点(路径里几乎不会出现,紧跟在路径后面时当作句子的标点)。
fn ends_mention(c: char) -> bool {
    c.is_whitespace() || "，。、；：！？）】」』》".contains(c)
}

/// 正文里有没有可渲染成胶囊的文件引用。
pub fn has_mentions(text: &str) -> bool {
    segments(text).iter().any(|s| matches!(s, Seg::Mention(_)))
}

/// 把一行(或整段)正文切成文字与文件引用。
///
/// 参数:`text` 正文。返回:按出现顺序的分段,拼起来等于原文(引用段不含开头的 `@`,拼接时补回)。
pub fn segments(text: &str) -> Vec<Seg<'_>> {
    let mut out = Vec::new();
    let (mut plain_from, mut prev_ws) = (0usize, true);
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '@' && prev_ws {
            let start = i + 1;
            let mut end = start;
            while let Some(&(j, d)) = chars.peek() {
                if ends_mention(d) {
                    break;
                }
                end = j + d.len_utf8();
                chars.next();
            }
            let raw = &text[start..end];
            let path = raw.trim_end_matches(|c: char| ",;:!?.)]}\"'".contains(c));
            if path.len() >= 2 && path.contains(['/', '.']) {
                if plain_from < i {
                    out.push(Seg::Text(&text[plain_from..i]));
                }
                out.push(Seg::Mention(path));
                plain_from = start + path.len();
                prev_ws = false;
                // 被削掉的末尾标点回到普通文字里
                continue;
            }
            prev_ws = false;
            continue;
        }
        prev_ws = c.is_whitespace();
    }
    if plain_from < text.len() {
        out.push(Seg::Text(&text[plain_from..]));
    }
    out
}

/// 把文字再切成可折行的小块:连续的半角非空白字符是一块,其余字符(汉字等)各一块;
/// 行首 / 标点不单独成块,而是粘在前一块后面,避免一行以「。」开头。空白返回 `None` 作为间隔。
fn pieces(text: &str) -> Vec<Option<String>> {
    let mut out: Vec<Option<String>> = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<Option<String>>| {
        if !cur.is_empty() {
            out.push(Some(std::mem::take(cur)));
        }
    };
    for c in text.chars() {
        if c.is_whitespace() {
            flush(&mut cur, &mut out);
            out.push(None);
        } else if c.is_ascii() {
            cur.push(c);
        } else if "，。、；：！？）】」』》”’".contains(c) {
            // 全角收尾标点粘在前一块上
            cur.push(c);
            flush(&mut cur, &mut out);
        } else {
            flush(&mut cur, &mut out);
            out.push(Some(c.to_string()));
        }
    }
    flush(&mut cur, &mut out);
    // 全角标点单独成块时(前一块是汉字)并回去
    let mut merged: Vec<Option<String>> = Vec::new();
    for p in out {
        match (&p, merged.last_mut()) {
            (Some(s), Some(Some(prev))) if s.chars().all(|c| "，。、；：！？）】」』》”’".contains(c)) => prev.push_str(s),
            _ => merged.push(p),
        }
    }
    merged
}

/// 文件引用胶囊:文件类型图标 + 文件名,和输入框里的标记同款。
fn chip(path: &str, cx: &App) -> AnyElement {
    let t = cx.theme();
    let name = Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string());
    // 绝对路径才去问文件系统是不是文件夹;相对路径按文件处理
    let is_dir = Path::new(path).is_absolute() && Path::new(path).is_dir();
    h_flex().h(px(22.)).px_1p5().gap_1().items_center().rounded(px(5.)).border_1().border_color(t.border).bg(t.muted.opacity(0.7)).text_size(px(13.)).mx(px(1.))
        .child(file_icon_img(&name, is_dir, false, 14.))
        .child(div().max_w(px(240.)).truncate().child(name))
        .into_any_element()
}

/// 一行正文(不含换行)渲染成可折行的一排:文字分块、胶囊夹在中间。
fn line(text: &str, line_h: f32, cx: &App) -> AnyElement {
    let mut row = h_flex().w_full().min_w_0().flex_wrap().items_center().min_h(px(line_h));
    for seg in segments(text) {
        match seg {
            Seg::Mention(p) => row = row.child(chip(p, cx)),
            Seg::Text(s) => {
                for p in pieces(s) {
                    row = match p {
                        Some(w) => row.child(div().flex_none().whitespace_nowrap().child(w)),
                        None => row.child(div().flex_none().w(px(4.))),
                    };
                }
            }
        }
    }
    row.into_any_element()
}

/// 把含文件引用的正文渲染成多行(每个换行一行,空行保留行高)。
///
/// 参数:`text` 正文;`line_h` 行高;返回:纵向排列的各行。
pub fn render(text: &str, line_h: f32, cx: &App) -> AnyElement {
    let mut col = div().flex().flex_col().w_full().min_w_0();
    for l in text.split('\n') {
        col = col.child(line(l, line_h, cx));
    }
    col.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 识别绝对路径与相对路径() {
        assert_eq!(segments("看 @/a/b.rs 这个"), vec![Seg::Text("看 "), Seg::Mention("/a/b.rs"), Seg::Text(" 这个")]);
        assert_eq!(segments("@src/a.rs"), vec![Seg::Mention("src/a.rs")]);
    }

    #[::core::prelude::v1::test]
    fn 中文路径与全角标点收尾() {
        assert_eq!(segments("@/Users/x/新空间报表.xlsx。给我 sql"), vec![Seg::Mention("/Users/x/新空间报表.xlsx"), Seg::Text("。给我 sql")]);
    }

    #[::core::prelude::v1::test]
    fn 半角标点不算路径() {
        assert_eq!(segments("见 @a/b.rs."), vec![Seg::Text("见 "), Seg::Mention("a/b.rs"), Seg::Text(".")]);
    }

    #[::core::prelude::v1::test]
    fn 普通提及与邮箱不当文件() {
        assert!(!has_mentions("@张三 你看下"));
        assert!(!has_mentions("发给 a@b.com"));
        assert!(!has_mentions("没有引用"));
        assert!(has_mentions("a @./x"));
    }

    #[::core::prelude::v1::test]
    fn 拼回原文() {
        let t = "前 @/a/b.rs 中 @c.md。后";
        let s: String = segments(t).iter().map(|s| match s { Seg::Text(x) => x.to_string(), Seg::Mention(p) => format!("@{p}") }).collect();
        assert_eq!(s, t);
    }

    #[::core::prelude::v1::test]
    fn 分块_汉字逐字_英文成词_空白为间隔() {
        let p = pieces("ab cd。你好");
        assert_eq!(p, vec![Some("ab".into()), None, Some("cd。".into()), Some("你".into()), Some("好".into())]);
    }
}
