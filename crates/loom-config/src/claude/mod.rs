//! Claude Code 配置的解析与受控写入:权限规则、钩子、MCP 服务器、环境变量。
//!
//! 每一项都带「来源层」(用户 / 项目 / 本地),界面据此显示来源并把修改写回对应文件。
//! 解析全部是纯函数,便于测试;读文件在 `load`,写文件在 `write`。
//!
//! @author ixxxxoooo

mod load;
mod write;

#[cfg(test)]
mod tests;

pub use load::{load, scope_file};
pub use write::{add_permission_rule, remove_permission_rule};

use serde_json::Value;

/// 配置来源层。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    /// `~/.claude/settings.json`(MCP 为 `~/.claude.json` 顶层)
    User,
    /// `<项目>/.claude/settings.json`(MCP 为 `<项目>/.mcp.json`)
    Project,
    /// `<项目>/.claude/settings.local.json`(MCP 为 `~/.claude.json` 的 `projects[<项目>]`)
    Local,
}

impl Scope {
    pub const ALL: [Scope; 3] = [Scope::User, Scope::Project, Scope::Local];

    /// 界面标签。
    pub fn label(self) -> &'static str {
        match self {
            Scope::User => "用户",
            Scope::Project => "项目",
            Scope::Local => "本地",
        }
    }

    /// 是否依赖已打开的项目。
    pub fn needs_project(self) -> bool {
        !matches!(self, Scope::User)
    }
}

/// 一条权限规则及其来源层。
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub text: String,
    pub scope: Scope,
}

/// 权限规则(allow / deny / ask 三组)。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PermissionRules {
    pub allow: Vec<Rule>,
    pub deny: Vec<Rule>,
    pub ask: Vec<Rule>,
}

impl PermissionRules {
    /// 按种类(`allow` / `deny` / `ask`)取可变列表;未知种类归入 `ask`。
    pub fn list_mut(&mut self, kind: &str) -> &mut Vec<Rule> {
        match kind {
            "allow" => &mut self.allow,
            "deny" => &mut self.deny,
            _ => &mut self.ask,
        }
    }
}

/// 一条钩子。
#[derive(Debug, Clone, PartialEq)]
pub struct HookEntry {
    /// 事件名,如 `PreToolUse`
    pub event: String,
    /// 工具匹配,如 `Bash`;空表示全部
    pub matcher: String,
    pub command: String,
    pub scope: Scope,
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
    pub scope: Scope,
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

/// 解析一份 `settings.json`,所有条目标记为 `scope` 层。
///
/// 参数:`json` 文件文本;`scope` 来源层。返回:解析结果,格式错误时返回空配置。
pub fn parse_settings(json: &str, scope: Scope) -> ClaudeConfig {
    let Ok(v) = serde_json::from_str::<Value>(json) else { return ClaudeConfig::default() };
    let perms = v.get("permissions");
    let rules = |k: &str| string_list(perms.and_then(|p| p.get(k))).into_iter().map(|text| Rule { text, scope }).collect();
    let permissions = PermissionRules { allow: rules("allow"), deny: rules("deny"), ask: rules("ask") };
    let mut hooks = Vec::new();
    if let Some(map) = v.get("hooks").and_then(Value::as_object) {
        for (event, groups) in map {
            for g in groups.as_array().into_iter().flatten() {
                let matcher = g.get("matcher").and_then(Value::as_str).unwrap_or("").to_string();
                for h in g.get("hooks").and_then(Value::as_array).into_iter().flatten() {
                    if let Some(cmd) = h.get("command").and_then(Value::as_str) {
                        hooks.push(HookEntry { event: event.clone(), matcher: matcher.clone(), command: cmd.to_string(), scope });
                    }
                }
            }
        }
    }
    let env_names = v.get("env").and_then(Value::as_object).map(|m| m.keys().cloned().collect()).unwrap_or_default();
    let mcp_servers = v.get("mcpServers").map(|m| parse_mcp_servers(m, scope)).unwrap_or_default();
    ClaudeConfig { permissions, hooks, mcp_servers, env_names, default_model: v.get("model").and_then(Value::as_str).map(str::to_string) }
}

/// 解析 `mcpServers` 对象(`.mcp.json`、`~/.claude.json` 通用)。
///
/// 参数:`v` 为 `mcpServers` 的值;`scope` 来源层。返回:服务器列表。
pub fn parse_mcp_servers(v: &Value, scope: Scope) -> Vec<McpServer> {
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
            McpServer { name: name.clone(), transport, target, enabled: !cfg.get("disabled").and_then(Value::as_bool).unwrap_or(false), scope }
        })
        .collect()
}

/// 合并两份配置:规则按「文本 + 来源层」去重;MCP 同名保留先出现的(调用方按优先级从高到低传入)。
///
/// 参数:`base` 已有配置;`over` 追加的配置。返回:合并结果;`model` 以后者为准。
pub fn merge(mut base: ClaudeConfig, over: ClaudeConfig) -> ClaudeConfig {
    fn extend_rules(dst: &mut Vec<Rule>, src: Vec<Rule>) {
        for r in src {
            if !dst.contains(&r) {
                dst.push(r);
            }
        }
    }
    extend_rules(&mut base.permissions.allow, over.permissions.allow);
    extend_rules(&mut base.permissions.deny, over.permissions.deny);
    extend_rules(&mut base.permissions.ask, over.permissions.ask);
    base.hooks.extend(over.hooks);
    for s in over.mcp_servers {
        if !base.mcp_servers.iter().any(|m| m.name == s.name) {
            base.mcp_servers.push(s);
        }
    }
    for n in over.env_names {
        if !base.env_names.contains(&n) {
            base.env_names.push(n);
        }
    }
    if over.default_model.is_some() {
        base.default_model = over.default_model;
    }
    base
}
