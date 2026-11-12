//! 工具行的展示信息(对标 OhMyAgent 的 `ToolPresentation`):把一次工具调用拆成「动词 + 对象 + 附注」。
//!
//! @author ixxxxoooo

use loom_core::*;

/// 一行工具调用的展示信息。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Present {
    /// 动词(次要色),如「读取」「运行」;没有则为空
    pub verb: String,
    /// 对象(主色):文件路径、命令、搜索词、网址……
    pub subject: String,
    /// 附注(三级色),如行范围、搜索目录、命令说明
    pub detail: Option<String>,
    /// 对象是否是文件路径(显示文件图标 + 文件名)
    pub is_path: bool,
}

fn s<'a>(input: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    input.get(key).and_then(|v| v.as_str()).filter(|v| !v.is_empty())
}

/// 去掉标题开头的动词(「读取 src/a.rs」→「src/a.rs」),标题没有动词时原样返回。
fn strip_verb(title: &str) -> String {
    for v in ["读取", "编辑", "写入", "删除", "移动", "搜索网页:", "搜索", "运行", "获取", "访问"] {
        if let Some(rest) = title.strip_prefix(v) {
            let rest = rest.trim_start();
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }
    title.to_string()
}

/// 读取 / 编辑的行范围附注:有 `offset` 与 `limit` 时显示 `L起-止`。
fn line_range(input: &serde_json::Value) -> Option<String> {
    let offset = input.get("offset").and_then(|v| v.as_u64())?;
    match input.get("limit").and_then(|v| v.as_u64()) {
        Some(limit) => Some(format!("L{}-{}", offset, offset + limit.saturating_sub(1))),
        None => Some(format!("L{offset}")),
    }
}

/// `mcp__github__get_pr` → (`github`, `get pr`);不是 MCP 工具名返回 `None`。
fn mcp_parts(name: &str) -> Option<(String, String)> {
    let rest = name.strip_prefix("mcp__")?;
    let (server, tool) = rest.split_once("__")?;
    Some((server.to_string(), tool.replace('_', " ")))
}

/// 工具结果的第一段文本。
fn result_text(tool: &ToolCall) -> Option<&str> {
    tool.content.iter().find_map(|c| if let ToolContent::Text(t) = c { Some(t.as_str()) } else { None })
}

/// 搜索 / 查找的结果数附注:优先用工具自己报的数(「Found 3 files」),其次数行;
/// 「No matches / No files found」显示「无匹配」。没有结果文本时返回 `None`。
fn result_count(tool: &ToolCall) -> Option<String> {
    let text = result_text(tool)?;
    let first = text.lines().find(|l| !l.trim().is_empty())?.trim();
    let lower = first.to_lowercase();
    if lower.starts_with("no matches") || lower.starts_with("no files found") || lower.starts_with("no results") {
        return Some("无匹配".into());
    }
    // 「Found 3 files」「Found 12 lines」
    if let Some(rest) = first.strip_prefix("Found ")
        && let Some((n, unit)) = rest.split_once(' ')
        && let Ok(n) = n.parse::<usize>()
    {
        return Some(if unit.starts_with("file") { format!("{n} 个文件") } else { format!("{n} 处") });
    }
    // 「路径:行号:内容」的命中行(内容模式);上下文行不算
    let hits = text.lines().filter(|l| l.splitn(3, ':').nth(1).is_some_and(|m| !m.is_empty() && m.chars().all(|c| c.is_ascii_digit()))).count();
    if hits > 0 && tool.name != "Glob" {
        return Some(format!("{hits} 处"));
    }
    let n = text.lines().filter(|l| !l.trim().is_empty()).count();
    (n > 0).then(|| if tool.name == "Glob" || s(&tool.input, "output_mode") != Some("content") { format!("{n} 个文件") } else { format!("{n} 处") })
}

/// 读取结果里写进标题的「· N 行」(翻译器在结果到达时追加)。
fn read_lines(tool: &ToolCall) -> Option<String> {
    let (_, tail) = tool.title.rsplit_once(" · ")?;
    (tail.ends_with(" 行") && tail.trim_end_matches(" 行").chars().all(|c| c.is_ascii_digit())).then(|| tail.to_string())
}

/// 网址缩成「域名 + 截断的路径」,不是网址就原样返回。
fn short_url(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let path = path.trim_end_matches('/');
    if path.is_empty() {
        return host.to_string();
    }
    let path: String = if path.chars().count() > 28 { path.chars().take(27).chain(std::iter::once('…')).collect() } else { path.to_string() };
    format!("{host}/{path}")
}

/// 后台任务类工具的对象:任务 id 的前 8 位(没有就空)。
fn task_label(tool: &ToolCall) -> String {
    let id = tool.task_id.clone().or_else(|| ["task_id", "bash_id", "shell_id"].iter().find_map(|k| s(&tool.input, k).map(str::to_string)));
    id.map(|i| i.chars().take(8).collect()).unwrap_or_default()
}

/// 由工具调用算出展示信息。
///
/// 参数:`tool` 工具调用。返回:动词 / 对象 / 附注。
pub fn present(tool: &ToolCall) -> Present {
    let input = &tool.input;
    let path = s(input, "file_path").or_else(|| s(input, "path")).or_else(|| s(input, "notebook_path"));
    let by_path = |verb: &str, detail: Option<String>| Present {
        verb: verb.into(),
        subject: path.map(str::to_string).unwrap_or_else(|| strip_verb(&tool.title)),
        detail,
        is_path: path.is_some(),
    };
    match tool.kind {
        ToolKind::Read => by_path("读取", match (line_range(input), read_lines(tool)) {
            (Some(r), Some(n)) => Some(format!("{r} · {n}")),
            (a, b) => a.or(b),
        }),
        ToolKind::Edit | ToolKind::Write => {
            let new_file = tool.name == "Write" || tool.content.iter().any(|c| matches!(c, ToolContent::Diff { old, .. } if old.is_empty()));
            by_path(if new_file { "写入" } else { "编辑" }, None)
        }
        ToolKind::Delete => by_path("删除", None),
        ToolKind::Move => by_path("移动", None),
        ToolKind::Search => Present {
            verb: if tool.name == "Glob" { "查找文件" } else { "搜索" }.into(),
            subject: s(input, "pattern").or_else(|| s(input, "query")).map(str::to_string).unwrap_or_else(|| strip_verb(&tool.title)),
            detail: {
                let scope = s(input, "path").map(|p| p.rsplit('/').next().unwrap_or(p).to_string()).or_else(|| s(input, "glob").map(str::to_string));
                match (scope, result_count(tool)) {
                    (Some(a), Some(b)) => Some(format!("{a} · {b}")),
                    (a, b) => a.or(b),
                }
            },
            is_path: false,
        },
        ToolKind::Execute if matches!(tool.name.as_str(), "BashOutput" | "TaskOutput") => Present { verb: "读取后台输出".into(), subject: task_label(tool), ..Default::default() },
        ToolKind::Execute if matches!(tool.name.as_str(), "KillShell" | "KillBash" | "TaskStop") => Present { verb: "结束后台任务".into(), subject: task_label(tool), ..Default::default() },
        ToolKind::Execute if tool.name == "Monitor" => Present {
            verb: "监听".into(),
            subject: s(input, "description").map(str::to_string).or_else(|| s(input, "command").and_then(|c| c.lines().find(|l| !l.trim().is_empty())).map(str::to_string)).unwrap_or_default(),
            ..Default::default()
        },
        ToolKind::Execute => {
            let cmd = s(input, "command").map(str::to_string).unwrap_or_else(|| strip_verb(&tool.title));
            Present { verb: "运行".into(), subject: cmd.lines().find(|l| !l.trim().is_empty()).unwrap_or("").to_string(), detail: s(input, "description").map(str::to_string), is_path: false }
        }
        ToolKind::Fetch => Present {
            verb: if tool.name == "WebSearch" { "联网搜索" } else { "获取" }.into(),
            subject: s(input, "url").map(short_url).or_else(|| s(input, "query").map(str::to_string)).unwrap_or_else(|| strip_verb(&tool.title)),
            ..Default::default()
        },
        ToolKind::Think => Present { verb: "思考".into(), subject: tool.title.clone(), ..Default::default() },
        ToolKind::Mcp => match mcp_parts(&tool.name) {
            // 对象后面带第一个字符串参数,一眼看出调的是哪一个(如 get pr · #123)
            Some((server, name)) => Present {
                verb: server,
                subject: name,
                detail: input.as_object().and_then(|o| o.values().find_map(|v| v.as_str().filter(|x| !x.is_empty() && x.chars().count() <= 40).map(str::to_string))),
                is_path: false,
            },
            None => Present { subject: tool.title.clone(), ..Default::default() },
        },
        _ => Present { subject: if tool.title.is_empty() { tool.name.clone() } else { tool.title.clone() }, ..Default::default() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(name: &str, title: &str, input: serde_json::Value) -> ToolCall {
        let mut t = ToolCall::new("1", name, title);
        t.input = input;
        t
    }

    #[::core::prelude::v1::test]
    fn 读取带行范围() {
        let p = present(&tool("Read", "读取 a.rs", serde_json::json!({"file_path": "/x/a.rs", "offset": 10, "limit": 20})));
        assert_eq!((p.verb.as_str(), p.subject.as_str(), p.detail.as_deref(), p.is_path), ("读取", "/x/a.rs", Some("L10-29"), true));
    }

    #[::core::prelude::v1::test]
    fn 命令取首行并带说明() {
        let p = present(&tool("Bash", "运行 ls", serde_json::json!({"command": "\nls -la\npwd", "description": "列出文件"})));
        assert_eq!((p.verb.as_str(), p.subject.as_str(), p.detail.as_deref()), ("运行", "ls -la", Some("列出文件")));
    }

    #[::core::prelude::v1::test]
    fn 没有入参时从标题去掉动词() {
        let p = present(&tool("Read", "读取 src/main.rs", serde_json::json!({})));
        assert_eq!((p.subject.as_str(), p.is_path), ("src/main.rs", false));
    }

    #[::core::prelude::v1::test]
    fn 搜索附注带结果数() {
        let mut t = tool("Grep", "搜索 foo", serde_json::json!({"pattern": "foo", "path": "/x/src"}));
        t.kind = ToolKind::Search;
        t.content = vec![ToolContent::Text("a.rs:1:foo\nb.rs:2:foo\n".into())];
        assert_eq!(present(&t).detail.as_deref(), Some("src · 2 处"));
        let mut g = tool("Glob", "查找", serde_json::json!({"pattern": "*.rs"}));
        g.kind = ToolKind::Search;
        g.content = vec![ToolContent::Text("a.rs\nb.rs\nc.rs".into())];
        assert_eq!(present(&g).detail.as_deref(), Some("3 个文件"));
    }

    #[::core::prelude::v1::test]
    fn 无匹配与工具自报的数() {
        let mut t = tool("Grep", "搜索 foo", serde_json::json!({"pattern": "foo"}));
        t.kind = ToolKind::Search;
        t.content = vec![ToolContent::Text("No matches found".into())];
        assert_eq!(present(&t).detail.as_deref(), Some("无匹配"));
        t.content = vec![ToolContent::Text("Found 3 files\na.rs\nb.rs\nc.rs".into())];
        assert_eq!(present(&t).detail.as_deref(), Some("3 个文件"));
        // 内容模式只数命中行,不数上下文行
        let mut c = tool("Grep", "搜索 foo", serde_json::json!({"pattern": "foo", "output_mode": "content"}));
        c.kind = ToolKind::Search;
        c.content = vec![ToolContent::Text("a.rs:10:foo\na.rs-11-bar\nb.rs:2:foo".into())];
        assert_eq!(present(&c).detail.as_deref(), Some("2 处"));
    }

    #[::core::prelude::v1::test]
    fn 后台工具不再显示成运行命令() {
        let mut o = tool("BashOutput", "读取后台输出", serde_json::json!({"bash_id": "abcdef123456"}));
        o.kind = ToolKind::Execute;
        let p = present(&o);
        assert_eq!((p.verb.as_str(), p.subject.as_str()), ("读取后台输出", "abcdef12"));
        let mut m = tool("Monitor", "监听 x", serde_json::json!({"description": "盯日志", "command": "tail -f a"}));
        m.kind = ToolKind::Execute;
        let p = present(&m);
        assert_eq!((p.verb.as_str(), p.subject.as_str(), p.detail), ("监听", "盯日志", None));
    }

    #[::core::prelude::v1::test]
    fn 读取附带行数() {
        let mut t = tool("Read", "读取 a.rs · 120 行", serde_json::json!({"file_path": "/x/a.rs"}));
        t.kind = ToolKind::Read;
        assert_eq!(present(&t).detail.as_deref(), Some("120 行"));
        t.input = serde_json::json!({"file_path": "/x/a.rs", "offset": 10, "limit": 20});
        assert_eq!(present(&t).detail.as_deref(), Some("L10-29 · 120 行"));
    }

    #[::core::prelude::v1::test]
    fn 网址缩成域名加路径() {
        assert_eq!(short_url("https://docs.rs/"), "docs.rs");
        assert_eq!(short_url("https://docs.rs/tokio/latest/tokio/"), "docs.rs/tokio/latest/tokio");
        assert!(short_url("https://a.com/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").ends_with('…'));
    }

    #[::core::prelude::v1::test]
    fn mcp工具拆出服务名() {
        let p = present(&tool("mcp__github__get_pr", "x", serde_json::json!({})));
        assert_eq!((p.verb.as_str(), p.subject.as_str()), ("github", "get pr"));
    }
}
