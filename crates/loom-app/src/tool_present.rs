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
        ToolKind::Read => by_path("读取", line_range(input)),
        ToolKind::Edit | ToolKind::Write => {
            let new_file = tool.name == "Write" || tool.content.iter().any(|c| matches!(c, ToolContent::Diff { old, .. } if old.is_empty()));
            by_path(if new_file { "写入" } else { "编辑" }, None)
        }
        ToolKind::Delete => by_path("删除", None),
        ToolKind::Move => by_path("移动", None),
        ToolKind::Search => Present {
            verb: if tool.name == "Glob" { "查找文件" } else { "搜索" }.into(),
            subject: s(input, "pattern").or_else(|| s(input, "query")).map(str::to_string).unwrap_or_else(|| strip_verb(&tool.title)),
            detail: s(input, "path").map(|p| p.rsplit('/').next().unwrap_or(p).to_string()).or_else(|| s(input, "glob").map(str::to_string)),
            is_path: false,
        },
        ToolKind::Execute => {
            let cmd = s(input, "command").map(str::to_string).unwrap_or_else(|| strip_verb(&tool.title));
            Present { verb: "运行".into(), subject: cmd.lines().find(|l| !l.trim().is_empty()).unwrap_or("").to_string(), detail: s(input, "description").map(str::to_string), is_path: false }
        }
        ToolKind::Fetch => Present {
            verb: if tool.name == "WebSearch" { "联网搜索" } else { "获取" }.into(),
            subject: s(input, "url").or_else(|| s(input, "query")).map(str::to_string).unwrap_or_else(|| strip_verb(&tool.title)),
            ..Default::default()
        },
        ToolKind::Think => Present { verb: "思考".into(), subject: tool.title.clone(), ..Default::default() },
        ToolKind::Mcp => match mcp_parts(&tool.name) {
            Some((server, name)) => Present { verb: server, subject: name, ..Default::default() },
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
    fn mcp工具拆出服务名() {
        let p = present(&tool("mcp__github__get_pr", "x", serde_json::json!({})));
        assert_eq!((p.verb.as_str(), p.subject.as_str()), ("github", "get pr"));
    }
}
