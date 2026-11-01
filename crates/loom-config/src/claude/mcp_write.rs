//! MCP 服务器的增删改与启停。
//!
//! 存放位置(与 `claude mcp add --scope` 一致):用户层 `~/.claude.json` 顶层 `mcpServers`;
//! 项目层 `<项目>/.mcp.json`;本地层 `~/.claude.json` 的 `projects[<项目>].mcpServers`。
//!
//! @author ixxxxoooo

use super::load::project_key;
use super::write::{edit_json, object_at, set_string_list};
use super::{McpSpec, Scope};
use serde_json::{Map, Value};
use std::io::{Error, Result};
use std::path::{Path, PathBuf};

/// 某一层 MCP 配置所在的文件与键路径。
#[derive(Debug, Clone, PartialEq)]
pub struct McpLocation {
    pub file: PathBuf,
    pub keys: Vec<String>,
}

impl McpLocation {
    /// 计算位置。
    ///
    /// 参数:`home` 家目录;`project` 当前项目;`scope` 层。返回:位置;项目层 / 本地层但没有项目时为 `None`。
    pub fn of(home: &Path, project: Option<&Path>, scope: Scope) -> Option<Self> {
        let claude_json = home.join(".claude.json");
        let servers = "mcpServers".to_string();
        match scope {
            Scope::User => Some(Self { file: claude_json, keys: vec![servers] }),
            Scope::Project => project.map(|p| Self { file: p.join(".mcp.json"), keys: vec![servers] }),
            Scope::Local => project.map(|p| Self { file: claude_json, keys: vec!["projects".into(), project_key(p), servers] }),
        }
    }

    fn key_refs(&self) -> Vec<&str> {
        self.keys.iter().map(String::as_str).collect()
    }
}

fn locate(home: &Path, project: Option<&Path>, scope: Scope) -> Result<McpLocation> {
    McpLocation::of(home, project, scope).ok_or_else(|| Error::other("先打开一个项目"))
}

/// 校验名称与配置。
///
/// 参数:`name` 服务器名;`spec` 配置。返回:错误提示,通过时为 `Ok`。
pub fn validate(name: &str, spec: &McpSpec) -> std::result::Result<(), String> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)) {
        return Err("名称只能包含字母、数字、- _ .".into());
    }
    match spec.transport.as_str() {
        "stdio" if spec.command.trim().is_empty() => Err("stdio 服务器需要填写命令".into()),
        "stdio" => Ok(()),
        "http" | "sse" if !(spec.url.starts_with("http://") || spec.url.starts_with("https://")) => Err("URL 需以 http:// 或 https:// 开头".into()),
        "http" | "sse" => Ok(()),
        other => Err(format!("不支持的传输方式:{other}")),
    }
}

/// 新增或修改服务器;改名时移走旧条目,env、headers 等未编辑的字段原样保留。
///
/// 参数:`home` / `project` / `scope` 目标层;`old_name` 原名(新增时为 `None`);`name` 新名;`spec` 配置。返回:IO 结果。
pub fn upsert_mcp(home: &Path, project: Option<&Path>, scope: Scope, old_name: Option<&str>, name: &str, spec: &McpSpec) -> Result<()> {
    validate(name, spec).map_err(Error::other)?;
    let loc = locate(home, project, scope)?;
    edit_json(&loc.file, |root| {
        let servers = object_at(root, &loc.key_refs())?;
        let mut entry = old_name.and_then(|o| servers.remove(o)).or_else(|| servers.remove(name)).unwrap_or_else(|| Value::Object(Map::new()));
        let obj = entry.as_object_mut().ok_or_else(|| Error::other("服务器配置不是对象"))?;
        obj.insert("type".into(), Value::String(spec.transport.clone()));
        if spec.transport == "stdio" {
            obj.remove("url");
            obj.remove("headers");
            obj.insert("command".into(), Value::String(spec.command.trim().to_string()));
            obj.insert("args".into(), Value::Array(spec.args.iter().cloned().map(Value::String).collect()));
        } else {
            for k in ["command", "args", "env"] {
                obj.remove(k);
            }
            obj.insert("url".into(), Value::String(spec.url.trim().to_string()));
        }
        servers.insert(name.to_string(), entry);
        Ok(true)
    })
}

/// 原样写入一个服务器配置(粘贴 JSON 用)。
///
/// 参数:目标层;`name` 服务器名;`value` 配置对象。返回:IO 结果。
pub fn upsert_mcp_raw(home: &Path, project: Option<&Path>, scope: Scope, name: &str, value: Value) -> Result<()> {
    let spec = super::parse::parse_spec(&value);
    validate(name, &spec).map_err(Error::other)?;
    let loc = locate(home, project, scope)?;
    edit_json(&loc.file, |root| {
        object_at(root, &loc.key_refs())?.insert(name.to_string(), value);
        Ok(true)
    })
}

/// 删除服务器。
///
/// 参数:所在层;`name` 服务器名。返回:IO 结果。
pub fn remove_mcp(home: &Path, project: Option<&Path>, scope: Scope, name: &str) -> Result<()> {
    let loc = locate(home, project, scope)?;
    if !loc.file.exists() {
        return Ok(());
    }
    edit_json(&loc.file, |root| Ok(object_at(root, &loc.key_refs())?.remove(name).is_some()))
}

/// 启用 / 禁用服务器。
///
/// 项目层(`.mcp.json`)写本地层 `settings.local.json` 的 `enabledMcpjsonServers` / `disabledMcpjsonServers`
/// (启用即同时批准);其余层写 `~/.claude.json` 当前项目的 `disabledMcpServers`。
///
/// 参数:所在层;`name` 服务器名;`enabled` 目标状态。返回:IO 结果;没有项目时报错。
pub fn set_mcp_enabled(home: &Path, project: Option<&Path>, scope: Scope, name: &str, enabled: bool) -> Result<()> {
    let project = project.ok_or_else(|| Error::other("启停需要先打开一个项目"))?;
    if scope == Scope::Project {
        let local = project.join(".claude/settings.local.json");
        set_string_list(&local, &["disabledMcpjsonServers"], name, !enabled)?;
        set_string_list(&local, &["enabledMcpjsonServers"], name, enabled)
    } else {
        let key = project_key(project);
        set_string_list(&home.join(".claude.json"), &["projects", &key, "disabledMcpServers"], name, !enabled)
    }
}

/// 解析粘贴的 JSON:支持 `{"mcpServers":{...}}`、`{"名称":{...}}` 两种写法。
///
/// 参数:`text` 粘贴内容。返回:(名称, 配置) 列表,或错误提示。
pub fn parse_mcp_paste(text: &str) -> std::result::Result<Vec<(String, Value)>, String> {
    let v: Value = serde_json::from_str(text.trim()).map_err(|e| format!("JSON 格式错误:{e}"))?;
    let map = v.get("mcpServers").unwrap_or(&v).as_object().ok_or("需要一个 JSON 对象")?;
    if map.contains_key("command") || map.contains_key("url") {
        return Err("请写成 {\"名称\": {...}} 的形式".into());
    }
    let list: Vec<_> = map.iter().filter(|(_, c)| c.is_object()).map(|(n, c)| (n.clone(), c.clone())).collect();
    if list.is_empty() { Err("没有找到服务器配置".into()) } else { Ok(list) }
}
