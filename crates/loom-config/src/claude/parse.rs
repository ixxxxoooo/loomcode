//! 纯函数解析:`settings.json`、`mcpServers` 对象,以及多层合并。
//!
//! @author ixxxxoooo

use super::{ClaudeConfig, HookEntry, LayerSettings, McpServer, McpSpec, PermissionRules, Rule, Scope, string_list};
use serde_json::Value;

/// 解析一层里的非列表型设置。
fn parse_layer(v: &Value) -> LayerSettings {
    let perms = v.get("permissions");
    LayerSettings {
        default_mode: perms.and_then(|p| p.get("defaultMode")).and_then(Value::as_str).map(str::to_string),
        additional_dirs: string_list(perms.and_then(|p| p.get("additionalDirectories"))),
        disable_bypass: perms.and_then(|p| p.get("disableBypassPermissionsMode")).and_then(Value::as_str) == Some("disable"),
        sandbox: v.get("sandbox").and_then(|s| s.get("enabled")).and_then(Value::as_bool),
        enable_all_project_mcp: v.get("enableAllProjectMcpServers").and_then(Value::as_bool).unwrap_or(false),
        enabled_mcpjson: string_list(v.get("enabledMcpjsonServers")),
        disabled_mcpjson: string_list(v.get("disabledMcpjsonServers")),
    }
}

/// 解析钩子:`hooks.<事件>[].{matcher, hooks[]}`。
fn parse_hooks(v: &Value, scope: Scope) -> Vec<HookEntry> {
    let mut hooks = Vec::new();
    let Some(map) = v.get("hooks").and_then(Value::as_object) else { return hooks };
    for (event, groups) in map {
        for g in groups.as_array().into_iter().flatten() {
            let matcher = g.get("matcher").and_then(Value::as_str).unwrap_or("").to_string();
            for h in g.get("hooks").and_then(Value::as_array).into_iter().flatten() {
                let kind = h.get("type").and_then(Value::as_str).unwrap_or("command").to_string();
                let body = ["command", "url", "prompt"].iter().find_map(|k| h.get(*k).and_then(Value::as_str));
                if let Some(cmd) = body {
                    let timeout = h.get("timeout").and_then(Value::as_u64);
                    hooks.push(HookEntry { event: event.clone(), matcher: matcher.clone(), kind, command: cmd.to_string(), timeout, scope });
                }
            }
        }
    }
    hooks
}

/// 解析一份 `settings.json`,所有条目标记为 `scope` 层。
///
/// 参数:`json` 文件文本;`scope` 来源层。返回:解析结果,格式错误时返回空配置(仍带该层的默认设置)。
pub fn parse_settings(json: &str, scope: Scope) -> ClaudeConfig {
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return ClaudeConfig { layers: vec![(scope, LayerSettings::default())], ..Default::default() };
    };
    let perms = v.get("permissions");
    let rules = |k: &str| string_list(perms.and_then(|p| p.get(k))).into_iter().map(|text| Rule { text, scope }).collect();
    ClaudeConfig {
        permissions: PermissionRules { allow: rules("allow"), deny: rules("deny"), ask: rules("ask") },
        hooks: parse_hooks(&v, scope),
        mcp_servers: v.get("mcpServers").map(|m| parse_mcp_servers(m, scope)).unwrap_or_default(),
        env_names: v.get("env").and_then(Value::as_object).map(|m| m.keys().cloned().collect()).unwrap_or_default(),
        default_model: v.get("model").and_then(Value::as_str).map(str::to_string),
        layers: vec![(scope, parse_layer(&v))],
    }
}

/// 解析单个服务器配置的可编辑部分。
pub(crate) fn parse_spec(cfg: &Value) -> McpSpec {
    let url = cfg.get("url").and_then(Value::as_str).unwrap_or("").to_string();
    let transport = cfg.get("type").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| if url.is_empty() { "stdio".into() } else { "http".into() });
    McpSpec { transport, command: cfg.get("command").and_then(Value::as_str).unwrap_or("").to_string(), args: string_list(cfg.get("args")), url }
}

/// 解析 `mcpServers` 对象(`.mcp.json`、`~/.claude.json` 通用)。启用状态先只看旧式 `disabled` 字段,`load` 再按层修正。
///
/// 参数:`v` 为 `mcpServers` 的值;`scope` 来源层。返回:服务器列表。
pub fn parse_mcp_servers(v: &Value, scope: Scope) -> Vec<McpServer> {
    let Some(map) = v.as_object() else { return Vec::new() };
    map.iter()
        .map(|(name, cfg)| McpServer {
            name: name.clone(),
            spec: parse_spec(cfg),
            enabled: !cfg.get("disabled").and_then(Value::as_bool).unwrap_or(false),
            needs_approval: false,
            scope,
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
    base.layers.extend(over.layers);
    base
}
