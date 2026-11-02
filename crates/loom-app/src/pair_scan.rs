//! 括号配对扫描:按语言的注释 / 字符串 / 字符字面量规则跳过它们里面的括号,用栈配出 `()` `[]` `{}`,每对记嵌套深度。
//! 供括号字符着色(`ide_rainbow.rs`)与配对竖线(`ide_pairs.rs`)共用。
//!
//! @author ixxxxoooo

/// 超过这个字节数的文件不做配对(扫描会变慢)。
pub const MAX_BYTES: usize = 512 * 1024;

/// 一对括号:开括号与闭括号的字节偏移,以及嵌套深度(最外层为 0)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pair {
    pub open: usize,
    pub close: usize,
    pub depth: usize,
}

/// 语言的词法规则:哪些内容里的括号不算数。
#[derive(Debug, Clone, Copy)]
pub struct Rules {
    line_comments: &'static [&'static str],
    block_comment: Option<(&'static str, &'static str)>,
    /// 字符串引号;默认到行尾为止,`multiline` 里的引号可以跨行
    quotes: &'static [u8],
    multiline: &'static [u8],
    /// 是否支持三引号(Python)
    triple: bool,
    /// 是否有字符字面量 `'x'` / `'\n'`(Rust:`'` 也用于生命周期,所以只跳过「紧跟结束引号」的写法)
    char_lit: bool,
}

const C_LIKE: Rules = Rules { line_comments: &["//"], block_comment: Some(("/*", "*/")), quotes: b"\"'", multiline: b"", triple: false, char_lit: false };
const HASH: Rules = Rules { line_comments: &["#"], block_comment: None, quotes: b"\"'", multiline: b"", triple: false, char_lit: false };

/// 按 `file_lang::language_for` 的语言名取规则;不认识的语言(Markdown、纯文本、HTML…)返回 `None`,不做配对。
pub fn rules_for(lang: &str) -> Option<Rules> {
    Some(match lang {
        // Rust 的 `'` 常是生命周期,不当引号
        "rust" => Rules { quotes: b"\"", char_lit: true, ..C_LIKE },
        "python" => Rules { triple: true, ..HASH },
        "javascript" | "typescript" | "tsx" | "go" => Rules { multiline: b"`", quotes: b"\"'`", ..C_LIKE },
        "c" | "cpp" | "csharp" | "java" | "kotlin" | "swift" | "scala" | "php" | "zig" => C_LIKE,
        "bash" | "yaml" | "toml" | "ruby" | "make" | "cmake" => HASH,
        "sql" => Rules { line_comments: &["--"], ..C_LIKE },
        "lua" => Rules { line_comments: &["--"], block_comment: None, ..C_LIKE },
        "css" => Rules { line_comments: &[], block_comment: Some(("/*", "*/")), ..C_LIKE },
        "json" => Rules { line_comments: &[], block_comment: None, quotes: b"\"", ..C_LIKE },
        "graphql" => HASH,
        "proto" => C_LIKE,
        _ => return None,
    })
}

fn kind_of(b: u8) -> Option<(u8, bool)> {
    Some(match b {
        b'(' => (0, true),
        b')' => (0, false),
        b'[' => (1, true),
        b']' => (1, false),
        b'{' => (2, true),
        b'}' => (2, false),
        _ => return None,
    })
}

/// 字符字面量(`'x'`、`'\n'`、`'\''`、`'\u{41}'`、`'中'`)的字节长度;`rest` 以 `'` 开头。不是字面量(如生命周期 `'a`)返回 `None`。
fn char_literal_len(rest: &[u8]) -> Option<usize> {
    match *rest.get(1)? {
        b'\\' => rest.get(3..)?.iter().position(|&c| c == b'\'').map(|p| 3 + p + 1),
        c => {
            let w = if c < 0x80 { 1 } else if c < 0xE0 { 2 } else if c < 0xF0 { 3 } else { 4 };
            (rest.get(1 + w) == Some(&b'\'')).then_some(2 + w)
        }
    }
}

/// 扫描全文,返回按开括号位置排序的配对。注释与字符串里的括号、没有配上的括号都忽略。
pub fn scan(text: &str, rules: &Rules) -> Vec<Pair> {
    let bytes = text.as_bytes();
    let n = bytes.len();
    let mut pairs = Vec::new();
    // (括号种类, 开括号位置)
    let mut stack: Vec<(u8, usize)> = Vec::new();
    let mut i = 0;
    while i < n {
        let rest = &bytes[i..];
        if rules.line_comments.iter().any(|p| rest.starts_with(p.as_bytes())) {
            i += rest.iter().position(|&c| c == b'\n').unwrap_or(rest.len());
            continue;
        }
        if let Some((open, close)) = rules.block_comment {
            if rest.starts_with(open.as_bytes()) {
                let from = open.len();
                i += rest[from..].windows(close.len()).position(|w| w == close.as_bytes()).map_or(rest.len(), |p| from + p + close.len());
                continue;
            }
        }
        let b = bytes[i];
        if rules.char_lit && b == b'\'' {
            if let Some(n) = char_literal_len(rest) {
                i += n;
                continue;
            }
        }
        if rules.quotes.contains(&b) {
            if rules.triple && rest.len() >= 3 && rest[1] == b && rest[2] == b {
                let tri = [b; 3];
                i += rest[3..].windows(3).position(|w| w == tri).map_or(rest.len(), |p| 3 + p + 3);
                continue;
            }
            let multi = rules.multiline.contains(&b);
            i += 1;
            while i < n {
                match bytes[i] {
                    b'\\' => i += 2,
                    c if c == b => {
                        i += 1;
                        break;
                    }
                    b'\n' if !multi => break,
                    _ => i += 1,
                }
            }
            continue;
        }
        if let Some((kind, is_open)) = kind_of(b) {
            if is_open {
                stack.push((kind, i));
            } else if stack.last().is_some_and(|&(k, _)| k == kind) {
                let (_, open) = stack.pop().unwrap();
                pairs.push(Pair { open, close: i, depth: stack.len() });
            }
        }
        i += 1;
    }
    pairs.sort_by_key(|p| p.open);
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(lang: &str, src: &str) -> Vec<(usize, usize, usize)> {
        scan(src, &rules_for(lang).unwrap()).iter().map(|p| (p.open, p.close, p.depth)).collect()
    }

    #[::core::prelude::v1::test]
    fn 嵌套括号带深度() {
        // f(a[0], {b})
        assert_eq!(pairs("rust", "f(a[0], {b})"), vec![(1, 11, 0), (3, 5, 1), (8, 10, 1)]);
    }

    #[::core::prelude::v1::test]
    fn 字符串与注释里的括号不算() {
        assert_eq!(pairs("python", "x = f(\")\")  # (\n"), vec![(5, 9, 0)]);
        assert_eq!(pairs("rust", "a(/* ) */ b) // ("), vec![(1, 11, 0)]);
        // 三引号可跨行
        assert_eq!(pairs("python", "f('''\n)\n''')"), vec![(1, 11, 0)]);
    }

    #[::core::prelude::v1::test]
    fn 转义引号不提前结束字符串() {
        assert_eq!(pairs("python", r#"f("a\")b")"#), vec![(1, 9, 0)]);
    }

    #[::core::prelude::v1::test]
    fn 未闭合的单行字符串到行尾为止() {
        // 第一行引号没闭合,不能吞掉后面的括号
        assert_eq!(pairs("javascript", "a = \"x\n(b)"), vec![(7, 9, 0)]);
    }

    #[::core::prelude::v1::test]
    fn 错配与未配的括号被忽略() {
        assert_eq!(pairs("rust", "(]"), vec![]);
        assert_eq!(pairs("rust", "((a)"), vec![(1, 3, 1)]);
    }

    #[::core::prelude::v1::test]
    fn 生命周期的单引号不当引号() {
        assert_eq!(pairs("rust", "fn f<'a>(x: &'a str) {}"), vec![(8, 19, 0), (21, 22, 0)]);
    }

    #[::core::prelude::v1::test]
    fn 字符字面量里的括号不算() {
        // b'(' 与 '{' 是字符,不是括号;'\'' 的转义引号不提前结束
        assert_eq!(pairs("rust", "f(b'(', '{', '\\'', '中')"), vec![(1, 24, 0)]);
        assert_eq!(pairs("rust", "match c { b'(' => 1, _ => 2 }"), vec![(8, 28, 0)]);
    }

    #[::core::prelude::v1::test]
    fn 不认识的语言不做配对() {
        assert!(rules_for("markdown").is_none());
        assert!(rules_for("text").is_none());
    }
}

