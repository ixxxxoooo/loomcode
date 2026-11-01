//! Claude Code 插件:读取已安装插件与插件市场(`~/.claude/plugins/*.json`)、各层 `enabledPlugins` 启停,
//! 以及传给 `claude plugin ...` 的参数校验与构造。安装、更新、卸载本身由界面层调用 claude CLI 完成。
//!
//! @author ixxxxoooo

use crate::claude::{Scope, edit_json, scope_file};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 一个已安装的插件(同一个插件装在多个层会有多条)。
#[derive(Debug, Clone, PartialEq)]
pub struct PluginEntry {
    /// `名称@市场`
    pub id: String,
    pub name: String,
    pub market: String,
    pub version: String,
    pub scope: Scope,
    pub description: String,
    pub author: String,
    /// 组件概况,如「2 个命令」
    pub components: Vec<String>,
    pub enabled: bool,
}

/// 一个插件市场。
#[derive(Debug, Clone, PartialEq)]
pub struct Marketplace {
    pub name: String,
    /// GitHub 仓库、Git 地址或本地路径
    pub source: String,
    /// 最近更新日期(`YYYY-MM-DD`)
    pub updated: String,
}

/// 市场里可安装的一个插件(来自市场的 `marketplace.json`)。
#[derive(Debug, Clone, PartialEq)]
pub struct MarketPlugin {
    pub name: String,
    pub market: String,
    pub description: String,
}

impl MarketPlugin {
    /// 安装用的 id:`名称@市场`。
    pub fn id(&self) -> String {
        format!("{}@{}", self.name, self.market)
    }
}

/// 解析一个市场的 `marketplace.json`。
///
/// 参数:`dir_name` 市场目录名(清单里没有 `name` 时用它);`json` 文件文本。返回:该市场提供的插件。
pub fn parse_market_plugins(dir_name: &str, json: &str) -> Vec<MarketPlugin> {
    let Ok(v) = serde_json::from_str::<Value>(json) else { return Vec::new() };
    let market = v["name"].as_str().filter(|n| !n.is_empty()).unwrap_or(dir_name).to_string();
    v["plugins"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| Some(MarketPlugin { name: p["name"].as_str()?.to_string(), market: market.clone(), description: p["description"].as_str().unwrap_or("").to_string() }))
        .collect()
}

/// 读取所有已添加市场里可安装的插件(`~/.claude/plugins/marketplaces/*/.claude-plugin/marketplace.json`)。
pub fn list_market_plugins(home: &Path) -> Vec<MarketPlugin> {
    let Ok(rd) = std::fs::read_dir(plugins_dir(home).join("marketplaces")) else { return Vec::new() };
    let mut dirs: Vec<_> = rd.flatten().filter(|e| e.path().is_dir()).collect();
    dirs.sort_by_key(|e| e.file_name());
    dirs.into_iter().flat_map(|e| parse_market_plugins(&e.file_name().to_string_lossy(), &read(&e.path().join(".claude-plugin/marketplace.json")))).collect()
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn plugins_dir(home: &Path) -> PathBuf {
    home.join(".claude/plugins")
}

/// 解析 `installed_plugins.json`。
///
/// 参数:`json` 文件文本。返回:(id, 层, 版本, 安装目录, 所属项目路径) 列表。
pub fn parse_installed(json: &str) -> Vec<(String, Scope, String, PathBuf, Option<String>)> {
    let Ok(v) = serde_json::from_str::<Value>(json) else { return Vec::new() };
    let mut out = Vec::new();
    for (id, installs) in v["plugins"].as_object().into_iter().flatten() {
        for i in installs.as_array().into_iter().flatten() {
            let scope = match i["scope"].as_str() {
                Some("project") => Scope::Project,
                Some("local") => Scope::Local,
                _ => Scope::User,
            };
            out.push((
                id.clone(),
                scope,
                i["version"].as_str().unwrap_or("").to_string(),
                PathBuf::from(i["installPath"].as_str().unwrap_or("")),
                i["projectPath"].as_str().map(str::to_string),
            ));
        }
    }
    out
}

/// 解析 `known_marketplaces.json`。
///
/// 参数:`json` 文件文本。返回:按名称排序的市场列表。
pub fn parse_markets(json: &str) -> Vec<Marketplace> {
    let Ok(v) = serde_json::from_str::<Value>(json) else { return Vec::new() };
    let mut out: Vec<Marketplace> = v
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, m)| {
            let s = &m["source"];
            let source = s["repo"].as_str().or_else(|| s["url"].as_str()).or_else(|| s["path"].as_str()).unwrap_or("").to_string();
            Marketplace { name: name.clone(), source, updated: m["lastUpdated"].as_str().unwrap_or("").chars().take(10).collect() }
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// 数某个目录下满足条件的条目数。
fn count(dir: &Path, pred: impl Fn(&Path) -> bool) -> usize {
    std::fs::read_dir(dir).into_iter().flatten().flatten().filter(|e| pred(&e.path())).count()
}

/// 读插件自带的说明与组件概况。
///
/// 参数:`install` 安装目录。返回:(描述, 作者, 组件概况)。
fn read_manifest(install: &Path) -> (String, String, Vec<String>) {
    let v: Value = serde_json::from_str(&read(&install.join(".claude-plugin/plugin.json"))).unwrap_or(Value::Null);
    let description = v["description"].as_str().unwrap_or("").to_string();
    let author = v["author"].as_str().or_else(|| v["author"]["name"].as_str()).unwrap_or("").to_string();
    let md = |p: &Path| p.extension().is_some_and(|e| e == "md");
    let mut parts = Vec::new();
    for (label, n) in [
        ("个命令", count(&install.join("commands"), md)),
        ("个子 Agent", count(&install.join("agents"), md)),
        ("个 Skill", count(&install.join("skills"), |p| p.join("SKILL.md").is_file())),
    ] {
        if n > 0 {
            parts.push(format!("{n} {label}"));
        }
    }
    if install.join("hooks").exists() || v.get("hooks").is_some() {
        parts.push("Hooks".into());
    }
    if install.join(".mcp.json").exists() || v.get("mcpServers").is_some() {
        parts.push("MCP".into());
    }
    if v.get("lspServers").is_some() || install.join(".lsp.json").exists() {
        parts.push("LSP".into());
    }
    (description, author, parts)
}

/// 各层 `enabledPlugins` 合并(后层覆盖前层)。
fn enabled_map(home: &Path, project: Option<&Path>) -> HashMap<String, bool> {
    let mut map = HashMap::new();
    for scope in Scope::ALL {
        let Some(path) = scope_file(home, project, scope) else { continue };
        let v: Value = serde_json::from_str(&read(&path)).unwrap_or(Value::Null);
        for (id, on) in v["enabledPlugins"].as_object().into_iter().flatten() {
            if let Some(b) = on.as_bool() {
                map.insert(id.clone(), b);
            }
        }
    }
    map
}

/// 列出已安装插件:用户层的,加上属于当前项目的项目层 / 本地层的。
///
/// 参数:`home` 家目录;`project` 当前项目。返回:按名称排序的插件。
pub fn list_installed(home: &Path, project: Option<&Path>) -> Vec<PluginEntry> {
    let enabled = enabled_map(home, project);
    let proj = project.map(|p| p.to_string_lossy().trim_end_matches('/').to_string());
    let mut out: Vec<PluginEntry> = parse_installed(&read(&plugins_dir(home).join("installed_plugins.json")))
        .into_iter()
        .filter(|(_, scope, _, _, pp)| *scope == Scope::User || (proj.is_some() && pp.as_deref().map(|p| p.trim_end_matches('/')) == proj.as_deref()))
        .map(|(id, scope, version, path, _)| {
            let (name, market) = id.split_once('@').map(|(n, m)| (n.to_string(), m.to_string())).unwrap_or((id.clone(), String::new()));
            let (description, author, components) = read_manifest(&path);
            PluginEntry { enabled: enabled.get(&id).copied().unwrap_or(false), id, name, market, version, scope, description, author, components }
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name).then(a.scope.label().cmp(b.scope.label())));
    out
}

/// 列出已添加的插件市场。
pub fn list_markets(home: &Path) -> Vec<Marketplace> {
    parse_markets(&read(&plugins_dir(home).join("known_marketplaces.json")))
}

/// 在某一层启用 / 禁用插件(写 `enabledPlugins.<id>`,保留其余字段)。
///
/// 参数:`home` / `project` / `scope` 目标层;`id` 插件 id;`on` 目标状态。返回:IO 结果。
pub fn set_plugin_enabled(home: &Path, project: Option<&Path>, scope: Scope, id: &str, on: bool) -> std::io::Result<()> {
    let path = scope_file(home, project, scope).ok_or_else(|| std::io::Error::other("先打开一个项目"))?;
    edit_json(&path, |root| {
        let obj = crate::claude::object_at(root, &["enabledPlugins"])?;
        Ok(obj.insert(id.to_string(), Value::Bool(on)) != Some(Value::Bool(on)))
    })
}

/// 校验传给 CLI 的参数:非空、不以 `-` 开头(防止被当成选项)、没有空白与控制字符。
fn safe_arg(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('-') && !s.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// 校验插件标识:`名称` 或 `名称@市场`。
pub fn valid_plugin_id(id: &str) -> bool {
    safe_arg(id) && id.chars().all(|c| c.is_ascii_alphanumeric() || "-_.@/".contains(c)) && id.matches('@').count() <= 1
}

/// 校验市场来源:GitHub `owner/repo`、Git / https 地址或本地路径。
pub fn valid_market_source(s: &str) -> bool {
    safe_arg(s) && (!s.contains("..") || s.starts_with('/'))
}

/// 校验市场名称。
pub fn valid_market_name(s: &str) -> bool {
    safe_arg(s) && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
}

/// 层对应的 CLI 参数值。
fn scope_arg(scope: Scope) -> &'static str {
    match scope {
        Scope::User => "user",
        Scope::Project => "project",
        Scope::Local => "local",
    }
}

/// 构造 `claude plugin <动作> ...` 的参数;插件标识不合法时返回 `None`。
///
/// 参数:`action` 为 `install` / `update` / `uninstall`;`id` 插件标识;`scope` 层。返回:参数列表。
pub fn plugin_args(action: &str, id: &str, scope: Scope) -> Option<Vec<String>> {
    if !valid_plugin_id(id) || !["install", "update", "uninstall"].contains(&action) {
        return None;
    }
    Some(["plugin", action, "-s", scope_arg(scope), id].map(str::to_string).to_vec())
}

/// 构造 `claude plugin marketplace <动作> ...` 的参数;`add` 的参数是来源,`remove` / `update` 是名称(`update` 可空表示全部)。
pub fn market_args(action: &str, arg: &str) -> Option<Vec<String>> {
    let ok = match action {
        "add" => valid_market_source(arg),
        "remove" => valid_market_name(arg),
        "update" => arg.is_empty() || valid_market_name(arg),
        _ => false,
    };
    ok.then(|| {
        let mut v: Vec<String> = ["plugin", "marketplace", action].map(str::to_string).to_vec();
        if !arg.is_empty() {
            v.push(arg.to_string());
        }
        v
    })
}

#[cfg(test)]
#[path = "plugins_tests.rs"]
mod tests;
