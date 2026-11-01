//! 从磁盘读取并合并各层 Claude 配置,并按 Claude Code 的规则计算 MCP 启用 / 批准状态。
//!
//! @author ixxxxoooo

use super::{ClaudeConfig, McpServer, Scope, merge, parse_mcp_servers, parse_settings, string_list};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// 某一层 `settings.json` 的路径。
///
/// 参数:`home` 家目录;`project` 当前项目根;`scope` 层。返回:路径,项目层但没有项目时为 `None`。
pub fn scope_file(home: &Path, project: Option<&Path>, scope: Scope) -> Option<PathBuf> {
    match scope {
        Scope::User => Some(home.join(".claude/settings.json")),
        Scope::Project => project.map(|p| p.join(".claude/settings.json")),
        Scope::Local => project.map(|p| p.join(".claude/settings.local.json")),
    }
}

/// `~/.claude.json` 里 `projects` 的键:项目路径去掉末尾 `/`。
pub(crate) fn project_key(project: &Path) -> String {
    project.to_string_lossy().trim_end_matches('/').to_string()
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

/// `~/.claude.json` 里与 MCP 相关的内容。
#[derive(Debug, Default)]
pub(crate) struct ClaudeJsonMcp {
    /// 本地层(`projects[<项目>].mcpServers`)
    pub local: Vec<McpServer>,
    /// 用户层(顶层 `mcpServers`)
    pub user: Vec<McpServer>,
    /// `projects[<项目>].disabledMcpServers`
    pub disabled: Vec<String>,
}

/// 从 `~/.claude.json` 文本里取 MCP。
///
/// 参数:`json` 文件文本;`project` 当前项目根。返回:本地层、用户层服务器与项目内禁用名单。
pub(crate) fn claude_json_mcp(json: &str, project: Option<&Path>) -> ClaudeJsonMcp {
    let Ok(v) = serde_json::from_str::<Value>(json) else { return ClaudeJsonMcp::default() };
    let entry = project.and_then(|p| v.get("projects")?.get(project_key(p)));
    ClaudeJsonMcp {
        local: entry.and_then(|e| e.get("mcpServers")).map(|m| parse_mcp_servers(m, Scope::Local)).unwrap_or_default(),
        user: v.get("mcpServers").map(|m| parse_mcp_servers(m, Scope::User)).unwrap_or_default(),
        disabled: string_list(entry.and_then(|e| e.get("disabledMcpServers"))),
    }
}

/// 按各层设置修正启用 / 批准状态。
///
/// 规则(与 Claude Code 一致):项目层服务器被任一层 `disabledMcpjsonServers` 列出即禁用,
/// 未被 `enableAllProjectMcpServers` 或 `enabledMcpjsonServers` 批准则待批准;
/// 其余层服务器被 `~/.claude.json` 当前项目的 `disabledMcpServers` 列出即禁用。
fn apply_mcp_state(cfg: &mut ClaudeConfig, disabled: &[String]) {
    let layers: Vec<_> = cfg.layers.iter().map(|(_, l)| l.clone()).collect();
    for s in &mut cfg.mcp_servers {
        if s.scope == Scope::Project {
            let off = layers.iter().any(|l| l.disabled_mcpjson.contains(&s.name));
            let approved = layers.iter().any(|l| l.enable_all_project_mcp || l.enabled_mcpjson.contains(&s.name));
            s.enabled = s.enabled && !off;
            s.needs_approval = !approved && !off;
        } else if disabled.contains(&s.name) {
            s.enabled = false;
        }
    }
}

/// 读取并合并全部配置层。
///
/// 权限 / 钩子 / env:用户 → 项目 → 本地依次合并。
/// MCP:按 Claude Code 的优先级 本地 > 项目 > 用户 去重(同名只保留优先级最高的一条)。
///
/// 参数:`home` 家目录;`project` 当前项目根(没有打开项目时为 `None`)。返回:合并后的配置。
pub fn load(home: &Path, project: Option<&Path>) -> ClaudeConfig {
    let mut cfg = ClaudeConfig::default();
    for scope in Scope::ALL {
        if let Some(path) = scope_file(home, project, scope) {
            cfg = merge(cfg, parse_settings(&read(&path), scope));
        }
    }
    let from_settings = std::mem::take(&mut cfg.mcp_servers);
    let cj = claude_json_mcp(&read(&home.join(".claude.json")), project);
    let project_mcp = project
        .and_then(|p| serde_json::from_str::<Value>(&read(&p.join(".mcp.json"))).ok())
        .and_then(|v| v.get("mcpServers").map(|m| parse_mcp_servers(m, Scope::Project)))
        .unwrap_or_default();
    for s in cj.local.into_iter().chain(project_mcp).chain(cj.user).chain(from_settings) {
        if !cfg.mcp_servers.iter().any(|m| m.name == s.name) {
            cfg.mcp_servers.push(s);
        }
    }
    apply_mcp_state(&mut cfg, &cj.disabled);
    cfg
}
