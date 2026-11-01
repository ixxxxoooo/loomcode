//! 极简 YAML frontmatter 读取:只取顶层 `键: 值` 的标量,足够读 name / description 等字段。
//!
//! @author ixxxxoooo

/// 拆出 frontmatter。
///
/// 参数:`text` Markdown 全文。返回:(键值对, 正文);没有 frontmatter 时键值对为空、正文为全文。
pub fn split_frontmatter(text: &str) -> (Vec<(String, String)>, &str) {
    let Some(rest) = text.strip_prefix("---\n").or_else(|| text.strip_prefix("---\r\n")) else { return (Vec::new(), text) };
    let Some(end) = rest.find("\n---") else { return (Vec::new(), text) };
    let head = &rest[..end];
    let body = rest[end + 4..].trim_start_matches(['\r', '\n']);
    let pairs = head
        .lines()
        .filter(|l| !l.starts_with([' ', '\t', '-', '#']))
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().trim_matches(['"', '\'']).to_string()))
        .filter(|(k, _)| !k.is_empty())
        .collect();
    (pairs, body)
}

/// 取某个键的值。
pub fn get<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}
