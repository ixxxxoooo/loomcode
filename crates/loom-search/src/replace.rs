//! 替换:按行套用同一个正则(与搜索一致),支持 `$1` 捕获组与保留大小写,只改选中的匹配,原子写盘。
//!
//! @author ixxxxoooo

use crate::query::SearchQuery;
use std::collections::HashSet;
use std::path::Path;

/// 一处匹配的标识:(行号 1 起始, 起列, 止列),与 `Match` 的坐标一致。
pub type MatchKey = (u32, u32, u32);

/// 让替换文本沿用被替换文字的大小写习惯:全大写 → 全大写;全小写 → 全小写;首字母大写其余小写 → 首字母大写;其它不变。
///
/// 参数:`matched` 被替换的原文;`replacement` 替换文本。返回:调整后的替换文本。
pub fn apply_case(matched: &str, replacement: &str) -> String {
    let letters: Vec<char> = matched.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.is_empty() {
        return replacement.to_string();
    }
    if letters.iter().all(|c| c.is_uppercase()) && letters.len() > 1 {
        return replacement.to_uppercase();
    }
    if letters.iter().all(|c| c.is_lowercase()) {
        return replacement.to_lowercase();
    }
    if letters[0].is_uppercase() && letters[1..].iter().all(|c| c.is_lowercase()) {
        let mut cs = replacement.chars();
        return cs.next().map(|f| f.to_uppercase().collect::<String>() + cs.as_str()).unwrap_or_default();
    }
    replacement.to_string()
}

/// 把一行拆成(正文, 行尾)。
fn split_eol(piece: &str) -> (&str, &str) {
    if let Some(b) = piece.strip_suffix("\r\n") {
        (b, "\r\n")
    } else if let Some(b) = piece.strip_suffix('\n') {
        (b, "\n")
    } else {
        (piece, "")
    }
}

/// 在一个文件里替换。
///
/// 参数:`path` 文件;`q` 与搜索同一份条件;`replacement` 替换文本(正则模式下 `$1` / `${name}` 引用捕获组);
/// `preserve_case` 保留大小写;`only` 只替换这些匹配(`None` = 文件里全部)。
/// 返回:实际替换的处数;文件不是 UTF-8、读写失败时返回错误。`only` 里的匹配若已不存在会被跳过。
pub fn replace_in_file(path: &Path, q: &SearchQuery, replacement: &str, preserve_case: bool, only: Option<&HashSet<MatchKey>>) -> Result<usize, String> {
    let re = q.build_regex()?;
    let text = std::fs::read_to_string(path).map_err(|e| format!("读取 {} 失败:{e}", path.display()))?;
    let (mut out, mut count) = (String::with_capacity(text.len()), 0usize);
    for (i, piece) in text.split_inclusive('\n').enumerate() {
        let (body, eol) = split_eol(piece);
        let line_no = i as u32 + 1;
        let (mut last, mut last_byte, mut last_col) = (0usize, 0usize, 0u32);
        for caps in re.captures_iter(body) {
            let Some(m) = caps.get(0).filter(|m| !m.is_empty()) else { continue };
            let start = last_col + body[last_byte..m.start()].chars().count() as u32;
            let end = start + m.as_str().chars().count() as u32;
            (last_byte, last_col) = (m.start(), start);
            if only.is_some_and(|set| !set.contains(&(line_no, start, end))) {
                continue;
            }
            let mut rep = String::new();
            if q.regex { caps.expand(replacement, &mut rep) } else { rep.push_str(replacement) }
            if preserve_case {
                rep = apply_case(m.as_str(), &rep);
            }
            out.push_str(&body[last..m.start()]);
            out.push_str(&rep);
            last = m.end();
            count += 1;
        }
        out.push_str(&body[last..]);
        out.push_str(eol);
    }
    if count > 0 {
        write_atomic(path, &out)?;
    }
    Ok(count)
}

/// 先写同目录临时文件再改名,保留原权限,中途失败不会留下半截文件。
fn write_atomic(path: &Path, content: &str) -> Result<(), String> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.loom-replace"));
    let result = (|| -> std::io::Result<()> {
        std::fs::write(&tmp, content)?;
        if let Ok(meta) = std::fs::metadata(path) {
            let _ = std::fs::set_permissions(&tmp, meta.permissions());
        }
        std::fs::rename(&tmp, path)
    })();
    result.map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("写入 {} 失败:{e}", path.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str, content: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("loom-replace-{name}-{}.txt", std::process::id()));
        std::fs::write(&p, content).unwrap();
        p
    }

    #[test]
    fn 字面量替换保留换行风格() {
        let p = temp("lit", "foo 1\r\nbar foo\nfoo");
        let n = replace_in_file(&p, &SearchQuery::new("foo"), "$1x", false, None).unwrap();
        assert_eq!(n, 3);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "$1x 1\r\nbar $1x\n$1x", "字面量模式下 $ 不是捕获组");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn 正则捕获组() {
        let p = temp("re", "a=1\nb=2\n");
        let mut q = SearchQuery::new(r"(\w)=(\d)");
        q.regex = true;
        replace_in_file(&p, &q, "$2<-$1", false, None).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "1<-a\n2<-b\n");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn 只替换选中的那一处() {
        let p = temp("one", "foo foo\nfoo\n");
        let only: HashSet<MatchKey> = [(1, 4, 7)].into_iter().collect();
        assert_eq!(replace_in_file(&p, &SearchQuery::new("foo"), "X", false, Some(&only)).unwrap(), 1);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "foo X\nfoo\n");
        let stale: HashSet<MatchKey> = [(9, 0, 3)].into_iter().collect();
        assert_eq!(replace_in_file(&p, &SearchQuery::new("foo"), "X", false, Some(&stale)).unwrap(), 0);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn 保留大小写() {
        assert_eq!(apply_case("FOO", "bar"), "BAR");
        assert_eq!(apply_case("foo", "BAR"), "bar");
        assert_eq!(apply_case("Foo", "bar"), "Bar");
        assert_eq!(apply_case("fOo", "bar"), "bar");
        let p = temp("case", "Foo FOO foo");
        replace_in_file(&p, &SearchQuery::new("foo"), "bar", true, None).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "Bar BAR bar");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn 非utf8文件报错而不是写坏() {
        let p = std::env::temp_dir().join(format!("loom-replace-bin-{}.bin", std::process::id()));
        std::fs::write(&p, [0xff, 0xfe, b'f', b'o', b'o']).unwrap();
        assert!(replace_in_file(&p, &SearchQuery::new("foo"), "x", false, None).is_err());
        let _ = std::fs::remove_file(&p);
    }
}
