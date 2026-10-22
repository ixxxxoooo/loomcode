//! Claude Code 配置的只读解析与受控写入:权限规则、钩子、MCP 服务器、环境变量、CLAUDE.md。
//!
//! 全部以纯函数解析 JSON 文本,便于测试;读写文件的薄封装放在最后。

use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

/// 权限规则(allow / deny / ask 三组)。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PermissionRules {
    pub allow: Vec<String>,
    pub deny: Vec<String>,
    pub ask: Vec<String>,
}

/// 一条钩子。
#[derive(Debug, Clone, PartialEq)]
pub struct HookEntry {
    /// 事件名,如 `PreToolUse`
    pub event: String,
    /// 工具匹配,如 `Bash`;空表示全部
    pub matcher: String,
    pub command: String,
}

/// 一个 MCP 服务器。
#[derive(Debug, Clone, PartialEq)]
pub struct McpServer {
    pub name: String,
    /// `stdio` / `http` / `sse`
    pub transport: String,
    /// 命令行(stdio)或 URL
    pub target: String,
    pub enabled: bool,
}

/// 聚合后的 Claude 配置视图。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClaudeConfig {
    pub permissions: PermissionRules,
    pub hooks: Vec<HookEntry>,
    pub mcp_servers: Vec<McpServer>,
    /// 环境变量名(值不读取)
    pub env_names: Vec<String>,
    pub default_model: Option<String>,
}

fn string_list(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// 解析 `settings.json`(用户级、项目级、本地级格式相同)。
pub fn parse_settings(json: &str) -> ClaudeConfig {
    let Ok(v) = serde_json::from_str::<Value>(json) else { return ClaudeConfig::default() };
    let perms = v.get("permissions");
    let permissions = PermissionRules {
        allow: string_list(perms.and_then(|p| p.get("allow"))),
        deny: string_list(perms.and_then(|p| p.get("deny"))),
        ask: string_list(perms.and_then(|p| p.get("ask"))),
    };
    let mut hooks = Vec::new();
    if let Some(map) = v.get("hooks").and_then(Value::as_object) {
        for (event, groups) in map {
            for g in groups.as_array().into_iter().flatten() {
                let matcher = g.get("matcher").and_then(Value::as_str).unwrap_or("").to_string();
                for h in g.get("hooks").and_then(Value::as_array).into_iter().flatten() {
                    if let Some(cmd) = h.get("command").and_then(Value::as_str) {
                        hooks.push(HookEntry { event: event.clone(), matcher: matcher.clone(), command: cmd.to_string() });
                    }
                }
            }
        }
    }
    let env_names = v.get("env").and_then(Value::as_object).map(|m| m.keys().cloned().collect()).unwrap_or_default();
    let mcp_servers = v.get("mcpServers").map(parse_mcp_servers).unwrap_or_default();
    ClaudeConfig { permissions, hooks, mcp_servers, env_names, default_model: v.get("model").and_then(Value::as_str).map(str::to_string) }
}

/// 解析 `mcpServers` 对象(`.mcp.json` 与 `~/.claude.json` 通用)。
pub fn parse_mcp_servers(v: &Value) -> Vec<McpServer> {
    let Some(map) = v.as_object() else { return Vec::new() };
    map.iter()
        .map(|(name, cfg)| {
            let url = cfg.get("url").and_then(Value::as_str);
            let transport = cfg.get("type").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| if url.is_some() { "http".into() } else { "stdio".into() });
            let target = match url {
                Some(u) => u.to_string(),
                None => {
                    let cmd = cfg.get("command").and_then(Value::as_str).unwrap_or("");
                    let args = string_list(cfg.get("args")).join(" ");
                    format!("{cmd} {args}").trim().to_string()
                }
            };
            McpServer { name: name.clone(), transport, target, enabled: !cfg.get("disabled").and_then(Value::as_bool).unwrap_or(false) }
        })
        .collect()
}

/// 合并两份配置(后者追加在后,规则去重)。
pub fn merge(mut base: ClaudeConfig, over: ClaudeConfig) -> ClaudeConfig {
    fn extend_unique(dst: &mut Vec<String>, src: Vec<String>) {
        for s in src {
            if !dst.contains(&s) {
                dst.push(s);
            }
        }
    }
    extend_unique(&mut base.permissions.allow, over.permissions.allow);
    extend_unique(&mut base.permissions.deny, over.permissions.deny);
    extend_unique(&mut base.permissions.ask, over.permissions.ask);
    base.hooks.extend(over.hooks);
    for s in over.mcp_servers {
        if !base.mcp_servers.iter().any(|m| m.name == s.name) {
            base.mcp_servers.push(s);
        }
    }
    extend_unique(&mut base.env_names, over.env_names);
    if over.default_model.is_some() {
        base.default_model = over.default_model;
    }
    base
}

/// 读取用户级 + 项目级配置(`~/.claude/settings.json`、`<项目>/.claude/settings.json`、`settings.local.json`、`.mcp.json`)。
pub fn load(home: &Path, project: Option<&Path>) -> ClaudeConfig {
    let read = |p: PathBuf| std::fs::read_to_string(p).unwrap_or_default();
    let mut cfg = parse_settings(&read(home.join(".claude/settings.json")));
    if let Some(proj) = project {
        cfg = merge(cfg, parse_settings(&read(proj.join(".claude/settings.json"))));
        cfg = merge(cfg, parse_settings(&read(proj.join(".claude/settings.local.json"))));
        if let Ok(v) = serde_json::from_str::<Value>(&read(proj.join(".mcp.json"))) {
            if let Some(m) = v.get("mcpServers") {
                for s in parse_mcp_servers(m) {
                    if !cfg.mcp_servers.iter().any(|x| x.name == s.name) {
                        cfg.mcp_servers.push(s);
                    }
                }
            }
        }
    }
    cfg
}

/// 向项目级 `settings.local.json` 追加一条权限规则(保留其余字段)。
pub fn add_permission_rule(path: &Path, kind: &str, rule: &str) -> std::io::Result<()> {
    let mut root: Value = std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| Value::Object(Map::new()));
    let obj = root.as_object_mut().ok_or_else(|| std::io::Error::other("配置根不是对象"))?;
    let perms = obj.entry("permissions").or_insert_with(|| Value::Object(Map::new()));
    let list = perms
        .as_object_mut()
        .ok_or_else(|| std::io::Error::other("permissions 不是对象"))?
        .entry(kind.to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let arr = list.as_array_mut().ok_or_else(|| std::io::Error::other("规则列表不是数组"))?;
    if !arr.iter().any(|x| x.as_str() == Some(rule)) {
        arr.push(Value::String(rule.to_string()));
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(&root).map_err(std::io::Error::other)?)
}

/// 从配置里移除一条权限规则。
pub fn remove_permission_rule(path: &Path, kind: &str, rule: &str) -> std::io::Result<()> {
    let Ok(text) = std::fs::read_to_string(path) else { return Ok(()) };
    let mut root: Value = serde_json::from_str(&text).map_err(std::io::Error::other)?;
    if let Some(arr) = root.get_mut("permissions").and_then(|p| p.get_mut(kind)).and_then(Value::as_array_mut) {
        arr.retain(|x| x.as_str() != Some(rule));
    }
    std::fs::write(path, serde_json::to_string_pretty(&root).map_err(std::io::Error::other)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "model": "opus",
        "env": {"ANTHROPIC_AUTH_TOKEN": "secret-value", "FOO": "1"},
        "permissions": {"allow": ["Bash(git status)", "Read"], "deny": ["Bash(rm -rf:*)"]},
        "hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "echo hi"}]}]},
        "mcpServers": {
            "fs": {"command": "npx", "args": ["-y", "server-fs"]},
            "web": {"type": "http", "url": "https://x.test/mcp", "disabled": true}
        }
    }"#;

    #[test]
    fn 解析权限与钩子() {
        let c = parse_settings(SAMPLE);
        assert_eq!(c.permissions.allow, vec!["Bash(git status)", "Read"]);
        assert_eq!(c.permissions.deny.len(), 1);
        assert_eq!(c.hooks, vec![HookEntry { event: "PreToolUse".into(), matcher: "Bash".into(), command: "echo hi".into() }]);
        assert_eq!(c.default_model.as_deref(), Some("opus"));
    }

    #[test]
    fn 环境变量只保留名称() {
        let c = parse_settings(SAMPLE);
        assert_eq!(c.env_names, vec!["ANTHROPIC_AUTH_TOKEN", "FOO"]);
        assert!(!format!("{c:?}").contains("secret-value"));
    }

    #[test]
    fn 解析_mcp_服务器() {
        let c = parse_settings(SAMPLE);
        assert_eq!(c.mcp_servers[0], McpServer { name: "fs".into(), transport: "stdio".into(), target: "npx -y server-fs".into(), enabled: true });
        assert_eq!(c.mcp_servers[1].transport, "http");
        assert!(!c.mcp_servers[1].enabled);
    }

    #[test]
    fn 合并去重() {
        let a = parse_settings(r#"{"permissions":{"allow":["A","B"]}}"#);
        let b = parse_settings(r#"{"permissions":{"allow":["B","C"]}}"#);
        assert_eq!(merge(a, b).permissions.allow, vec!["A", "B", "C"]);
    }

    #[test]
    fn 增删权限规则保留其余字段() {
        let dir = std::env::temp_dir().join(format!("ucode-claude-{}", std::process::id()));
        let path = dir.join(".claude/settings.local.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"model":"opus"}"#).unwrap();
        add_permission_rule(&path, "allow", "Bash(ls)").unwrap();
        add_permission_rule(&path, "allow", "Bash(ls)").unwrap();
        let c = parse_settings(&std::fs::read_to_string(&path).unwrap());
        assert_eq!(c.permissions.allow, vec!["Bash(ls)"]);
        assert_eq!(c.default_model.as_deref(), Some("opus"));
        remove_permission_rule(&path, "allow", "Bash(ls)").unwrap();
        assert!(parse_settings(&std::fs::read_to_string(&path).unwrap()).permissions.allow.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}
