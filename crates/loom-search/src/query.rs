//! 搜索条件:关键字 + 三个开关 + 包含 / 排除 glob,负责编译成正则与路径过滤器。
//!
//! @author ixxxxoooo

use ignore::overrides::{Override, OverrideBuilder};
use regex::{Regex, RegexBuilder};
use std::path::Path;

/// 默认排除的目录(与 VS Code 的 `search.exclude` 一致);只在「使用忽略文件」开启时生效。
const DEFAULT_EXCLUDES: [&str; 2] = ["node_modules", "bower_components"];

/// 一次搜索的全部条件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    /// 关键字(正则模式下是正则)
    pub pattern: String,
    /// 区分大小写
    pub case_sensitive: bool,
    /// 全词匹配
    pub whole_word: bool,
    /// 按正则解释关键字
    pub regex: bool,
    /// 要包含的文件,逗号分隔的 glob;空 = 全部
    pub include: String,
    /// 要排除的文件,逗号分隔的 glob
    pub exclude: String,
    /// 遵守 `.gitignore` 等忽略文件并应用默认排除
    pub use_ignore_files: bool,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self { pattern: String::new(), case_sensitive: false, whole_word: false, regex: false, include: String::new(), exclude: String::new(), use_ignore_files: true }
    }
}

/// 是不是单词字符(字母、数字、下划线)。
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl SearchQuery {
    /// 以关键字创建,其余取默认。
    ///
    /// 参数:`pattern` 关键字。返回:搜索条件。
    pub fn new(pattern: impl Into<String>) -> Self {
        Self { pattern: pattern.into(), ..Self::default() }
    }

    /// 编译成正则:字面量会转义,全词在两端加 `\b`(字面量首 / 尾不是单词字符时该侧不加,否则永远匹配不到),按行匹配。
    ///
    /// 返回:正则;关键字为空或正则非法时返回人话错误。
    pub fn build_regex(&self) -> Result<Regex, String> {
        if self.pattern.is_empty() {
            return Err("请输入要搜索的内容".into());
        }
        let body = if self.regex { self.pattern.clone() } else { regex::escape(&self.pattern) };
        let body = if self.whole_word {
            let (left, right) = if self.regex {
                (true, true)
            } else {
                (self.pattern.chars().next().is_some_and(is_word), self.pattern.chars().last().is_some_and(is_word))
            };
            format!("{}(?:{body}){}", if left { r"\b" } else { "" }, if right { r"\b" } else { "" })
        } else {
            body
        };
        RegexBuilder::new(&body).case_insensitive(!self.case_sensitive).multi_line(true).size_limit(10 << 20).build().map_err(|e| {
            let msg = e.to_string();
            let last = msg.lines().last().unwrap_or("").trim().trim_start_matches("error:").trim().to_string();
            format!("正则表达式无效:{last}")
        })
    }

    /// 编译包含 / 排除过滤器(相对 `root`)。
    ///
    /// 规则:逗号分隔(花括号内的逗号不分);`src` 同时匹配名为 src 的文件与目录里的全部文件;
    /// `./src` 与 `/src` 锚定在根目录;排除优先于包含。
    pub fn build_globs(&self, root: &Path) -> Result<Override, String> {
        let mut b = OverrideBuilder::new(root);
        let bad = |t: &str, e: ignore::Error| format!("文件过滤写法无效「{t}」:{e}");
        for term in split_terms(&self.include) {
            for g in expand(&term) {
                b.add(&g).map_err(|e| bad(&term, e))?;
            }
        }
        let defaults = self.use_ignore_files.then(|| DEFAULT_EXCLUDES.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap_or_default();
        for term in split_terms(&self.exclude).into_iter().chain(defaults) {
            for g in expand(&term) {
                b.add(&format!("!{g}")).map_err(|e| bad(&term, e))?;
            }
        }
        b.build().map_err(|e| format!("文件过滤写法无效:{e}"))
    }
}

/// 按逗号拆分 glob 列表(花括号里的逗号不拆),去掉空项与首尾空白。
pub(crate) fn split_terms(s: &str) -> Vec<String> {
    let (mut out, mut cur, mut depth) = (Vec::new(), String::new(), 0i32);
    for c in s.chars() {
        match c {
            '{' => { depth += 1; cur.push(c) }
            '}' => { depth -= 1; cur.push(c) }
            ',' if depth <= 0 => { out.push(std::mem::take(&mut cur)) }
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out.into_iter().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect()
}

/// 把一项展开成「自身」与「自身下的全部内容」两条 glob;`./x` 视为锚定到根的 `/x`。
fn expand(term: &str) -> Vec<String> {
    let t = term.trim_end_matches('/');
    let t = match t.strip_prefix("./") {
        Some(rest) => format!("/{rest}"),
        None => t.to_string(),
    };
    vec![t.clone(), format!("{t}/**")]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 字面量会转义() {
        let re = SearchQuery::new("a.b(").build_regex().unwrap();
        assert!(re.is_match("xa.b(y") && !re.is_match("axb("));
    }

    #[test]
    fn 全词只在单词字符一侧加边界() {
        let mut q = SearchQuery::new("fn");
        q.whole_word = true;
        let re = q.build_regex().unwrap();
        assert!(re.is_match("a fn b") && !re.is_match("fnord"));
        let mut q = SearchQuery::new("->");
        q.whole_word = true;
        assert!(q.build_regex().unwrap().is_match("a -> b"), "非单词字符的关键字不应被 \\b 卡死");
    }

    #[test]
    fn 大小写开关() {
        let mut q = SearchQuery::new("Foo");
        assert!(q.build_regex().unwrap().is_match("foo"));
        q.case_sensitive = true;
        assert!(!q.build_regex().unwrap().is_match("foo"));
    }

    #[test]
    fn 非法正则给出人话错误() {
        let mut q = SearchQuery::new("(");
        q.regex = true;
        assert!(q.build_regex().unwrap_err().starts_with("正则表达式无效"));
        assert!(SearchQuery::new("").build_regex().is_err());
    }

    #[test]
    fn 花括号内逗号不拆分() {
        assert_eq!(split_terms(" src, *.{rs,toml} ,, a "), vec!["src", "*.{rs,toml}", "a"]);
        assert_eq!(expand("./src/"), vec!["/src", "/src/**"]);
    }
}
