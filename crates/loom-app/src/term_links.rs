//! 终端里的链接:从一行文字里认出 URL 与文件路径(`path:行:列`),⌘ 点击打开。
//!
//! 识别是纯函数(可测试);是否真的存在、怎么打开由 `TerminalView::open_link_at` 决定。
//!
//! @author ixxxxoooo

use gpui_kit::*;

/// 打开终端里点到的文件(可带行号);由 Agent 工作台与 IDE 窗口各自处理。
#[derive(Clone, PartialEq, Action)]
#[action(namespace = loomcode, no_json)]
pub struct OpenTermPath {
    /// 绝对路径
    pub path: String,
    /// 1 起始的行号;没有则为 0
    pub line: u32,
}

/// 认出的链接目标。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkTarget {
    Url(String),
    /// 路径(可能是相对路径或 `~/` 开头)与行号、列号(没有则为 0)
    Path { path: String, line: u32, col: u32 },
}

/// 分隔符:一个「词」到这些字符为止。
fn is_delim(c: char) -> bool {
    c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '<' | '>' | '|' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';')
}

/// 在一行里找出第 `col` 列所在的链接。
///
/// 参数:`row` 按列展开的字符(宽字符后面跟一个 `'\0'` 占位);`col` 鼠标所在列。
/// 返回:链接目标与它占的列范围;该处不是链接返回 `None`。
pub fn find_at(row: &[char], col: usize) -> Option<(LinkTarget, std::ops::Range<usize>)> {
    // 点在宽字符的后半格上,按前半格算
    let col = if row.get(col) == Some(&'\0') { col.saturating_sub(1) } else { col };
    if is_delim(*row.get(col)?) {
        return None;
    }
    let (mut s, mut e) = (col, col);
    while s > 0 && !is_delim(row[s - 1]) {
        s -= 1;
    }
    while e + 1 < row.len() && !is_delim(row[e + 1]) {
        e += 1;
    }
    let word: String = row[s..=e].iter().filter(|c| **c != '\0').collect();
    // 词后面紧跟 `(行,列)` 的写法(如 `main.ts(12,5)`)
    let mut tail = String::new();
    if e + 1 < row.len() && row[e + 1] == '(' {
        let mut k = e + 2;
        while k < row.len() && (row[k].is_ascii_digit() || row[k] == ',') {
            k += 1;
        }
        if k < row.len() && row[k] == ')' && k > e + 2 {
            tail = row[e + 1..=k].iter().collect();
            e = k;
        }
    }
    parse_word(&word, &tail).map(|t| (t, s..e + 1))
}

/// 去掉词尾的标点。
fn trim_tail(w: &str) -> &str {
    w.trim_end_matches(['.', ',', ';', ':', '!', '?'])
}

/// 解析一个词:URL,或路径加可选的行列号。
fn parse_word(word: &str, paren: &str) -> Option<LinkTarget> {
    let w = trim_tail(word);
    if w.starts_with("http://") || w.starts_with("https://") {
        return (w.len() > 8).then(|| LinkTarget::Url(w.to_string()));
    }
    // 取尾部的 `:行` 或 `:行:列`
    let mut path = w;
    let (mut line, mut col) = (0u32, 0u32);
    let mut nums: Vec<u32> = Vec::new();
    while let Some((head, tail)) = path.rsplit_once(':') {
        match tail.parse::<u32>() {
            Ok(n) if nums.len() < 2 => {
                nums.push(n);
                path = head;
            }
            _ => break,
        }
    }
    nums.reverse();
    if let [l] = nums[..] {
        line = l;
    } else if let [l, c] = nums[..] {
        (line, col) = (l, c);
    }
    if line == 0 && !paren.is_empty() {
        let inner = paren.trim_matches(['(', ')']);
        let mut it = inner.split(',').filter_map(|x| x.parse::<u32>().ok());
        line = it.next().unwrap_or(0);
        col = it.next().unwrap_or(0);
    }
    let pathy = path.contains('/') || path.rsplit_once('.').is_some_and(|(stem, ext)| !stem.is_empty() && (1..=6).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()) && ext.chars().any(|c| c.is_ascii_alphabetic()));
    (pathy && !path.is_empty() && !path.ends_with('/')).then(|| LinkTarget::Path { path: path.to_string(), line, col })
}

/// 把路径解析成绝对路径:`~/` 展开,相对路径接在 `cwd` 后面;结果不是存在的文件返回 `None`。
pub fn resolve_path(path: &str, cwd: &str) -> Option<std::path::PathBuf> {
    let p = if let Some(rest) = path.strip_prefix("~/") {
        std::path::PathBuf::from(std::env::var("HOME").ok()?).join(rest)
    } else {
        let p = std::path::PathBuf::from(path);
        if p.is_absolute() { p } else { std::path::PathBuf::from(cwd).join(p) }
    };
    p.is_file().then_some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    fn at(s: &str, col: usize) -> Option<LinkTarget> {
        find_at(&row(s), col).map(|x| x.0)
    }

    #[::core::prelude::v1::test]
    fn 认出网址并去掉尾部标点() {
        assert_eq!(at("see https://example.com/a?b=1. ok", 8), Some(LinkTarget::Url("https://example.com/a?b=1".into())));
        assert_eq!(at("see https://example.com ok", 1), None, "点在普通单词上不是链接");
    }

    #[::core::prelude::v1::test]
    fn 路径加行列号() {
        let p = |path: &str, line, col| Some(LinkTarget::Path { path: path.into(), line, col });
        assert_eq!(at("error at src/main.rs:42:7: bad", 12), p("src/main.rs", 42, 7));
        assert_eq!(at("src/lib.rs:9", 3), p("src/lib.rs", 9, 0));
        assert_eq!(at("  --> crates/a/b.rs:10:5", 10), p("crates/a/b.rs", 10, 5));
        assert_eq!(at("main.ts(12,5): error", 2), p("main.ts", 12, 5));
        assert_eq!(at("open ~/notes/todo.md now", 8), p("~/notes/todo.md", 0, 0));
    }

    #[::core::prelude::v1::test]
    fn 不像路径的不认() {
        assert_eq!(at("hello world", 1), None);
        assert_eq!(at("version 1.2", 9), None, "纯数字小数不是路径");
        assert_eq!(at("dir/", 1), None);
        assert_eq!(at("a b", 1), None, "点在空格上");
    }

    #[::core::prelude::v1::test]
    fn 返回占的列范围() {
        let (_, r) = find_at(&row("x src/a.rs:3 y"), 4).unwrap();
        assert_eq!(r, 2..12);
    }

    #[::core::prelude::v1::test]
    fn 相对路径按目录解析_不存在返回空() {
        let dir = std::env::temp_dir().join(format!("loom-link-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/a.rs"), "x").unwrap();
        let cwd = dir.to_string_lossy().to_string();
        assert_eq!(resolve_path("src/a.rs", &cwd), Some(dir.join("src/a.rs")));
        assert_eq!(resolve_path("src/none.rs", &cwd), None);
        assert_eq!(resolve_path("src", &cwd), None, "目录不算");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
