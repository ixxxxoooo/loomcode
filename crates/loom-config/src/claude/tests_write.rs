//! Claude 配置写入的测试:权限规则、通用键、MCP、钩子、备份。
//!
//! @author ixxxxoooo

use super::*;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// 每个测试用独立临时目录。
pub fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("loom-claude-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 写入测试文件(自动建目录)。
pub fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn 增删权限规则保留其余字段并备份() {
    let dir = temp_dir("rw");
    let path = dir.join(".claude/settings.local.json");
    put(&path, r#"{"model":"opus"}"#);
    add_permission_rule(&path, "allow", "Bash(ls)").unwrap();
    add_permission_rule(&path, "allow", "Bash(ls)").unwrap();
    let v = read_json(&path);
    assert_eq!(v["permissions"]["allow"], json!(["Bash(ls)"]));
    assert_eq!(v["model"], "opus");
    assert_eq!(std::fs::read_to_string(backup_path(&path)).unwrap(), r#"{"model":"opus"}"#);
    remove_permission_rule(&path, "allow", "Bash(ls)").unwrap();
    assert!(read_json(&path)["permissions"].get("allow").is_none());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn 非法_json_不会被覆盖() {
    let dir = temp_dir("bad");
    let path = dir.join("settings.json");
    put(&path, "{ 坏掉的");
    assert!(add_permission_rule(&path, "allow", "X").is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ 坏掉的");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn 设置与删除嵌套键() {
    let dir = temp_dir("set");
    let path = dir.join("settings.json");
    set_value(&path, &["permissions", "defaultMode"], Some(json!("plan"))).unwrap();
    set_value(&path, &["sandbox", "enabled"], Some(json!(true))).unwrap();
    assert_eq!(read_json(&path)["permissions"]["defaultMode"], "plan");
    set_value(&path, &["permissions", "defaultMode"], None).unwrap();
    let v = read_json(&path);
    assert!(v["permissions"].get("defaultMode").is_none());
    assert_eq!(v["sandbox"]["enabled"], true);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn mcp_增改删与改名保留_env() {
    let home = temp_dir("mcp");
    let proj = home.join("proj");
    put(&home.join(".claude.json"), r#"{"numStartups":3,"mcpServers":{"old":{"command":"a","env":{"K":"v"}}}}"#);
    let spec = McpSpec { transport: "stdio".into(), command: "npx".into(), args: vec!["-y".into(), "x".into()], ..Default::default() };
    upsert_mcp(&home, None, Scope::User, Some("old"), "new", &spec).unwrap();
    let v = read_json(&home.join(".claude.json"));
    assert_eq!(v["numStartups"], 3);
    assert!(v["mcpServers"].get("old").is_none());
    assert_eq!(v["mcpServers"]["new"]["env"]["K"], "v");
    assert_eq!(v["mcpServers"]["new"]["args"], json!(["-y", "x"]));

    let http = McpSpec { transport: "http".into(), url: "https://m".into(), ..Default::default() };
    upsert_mcp(&home, Some(&proj), Scope::Local, None, "web", &http).unwrap();
    let key = proj.to_string_lossy().to_string();
    assert_eq!(read_json(&home.join(".claude.json"))["projects"][&key]["mcpServers"]["web"]["url"], "https://m");
    upsert_mcp(&home, Some(&proj), Scope::Project, None, "p", &spec).unwrap();
    assert!(proj.join(".mcp.json").exists());

    let c = load(&home, Some(&proj));
    assert_eq!(c.mcp_servers.len(), 3);
    remove_mcp(&home, Some(&proj), Scope::Local, "web").unwrap();
    assert!(load(&home, Some(&proj)).mcp_servers.iter().all(|m| m.name != "web"));
    assert!(upsert_mcp(&home, None, Scope::Project, None, "x", &spec).is_err());
    assert!(upsert_mcp(&home, None, Scope::User, None, "坏 名", &spec).is_err());
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn mcp_启停写到正确位置() {
    let home = temp_dir("toggle");
    let proj = home.join("proj");
    put(&proj.join(".mcp.json"), r#"{"mcpServers":{"p":{"command":"a"}}}"#);
    put(&home.join(".claude.json"), r#"{"mcpServers":{"u":{"command":"u"}}}"#);
    set_mcp_enabled(&home, Some(&proj), Scope::Project, "p", true).unwrap();
    set_mcp_enabled(&home, Some(&proj), Scope::User, "u", false).unwrap();
    let c = load(&home, Some(&proj));
    let get = |n: &str| c.mcp_servers.iter().find(|m| m.name == n).unwrap().clone();
    assert!(get("p").enabled && !get("p").needs_approval);
    assert!(!get("u").enabled);
    set_mcp_enabled(&home, Some(&proj), Scope::Project, "p", false).unwrap();
    set_mcp_enabled(&home, Some(&proj), Scope::User, "u", true).unwrap();
    let c = load(&home, Some(&proj));
    assert!(c.mcp_servers.iter().find(|m| m.name == "p").is_some_and(|m| !m.enabled));
    assert!(c.mcp_servers.iter().find(|m| m.name == "u").is_some_and(|m| m.enabled));
    assert!(set_mcp_enabled(&home, None, Scope::User, "u", false).is_err());
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn 解析粘贴的_mcp_json() {
    assert_eq!(parse_mcp_paste(r#"{"mcpServers":{"a":{"command":"x"}}}"#).unwrap()[0].0, "a");
    assert_eq!(parse_mcp_paste(r#"{"b":{"url":"https://b"}}"#).unwrap()[0].0, "b");
    assert!(parse_mcp_paste(r#"{"command":"x"}"#).is_err());
    assert!(parse_mcp_paste("不是 json").is_err());
}

#[test]
fn 钩子增删并清理空分组() {
    let dir = temp_dir("hooks");
    let path = dir.join("settings.json");
    add_hook(&path, "PreToolUse", "Bash", "echo 1", Some(10)).unwrap();
    add_hook(&path, "PreToolUse", "Bash", "echo 2", None).unwrap();
    add_hook(&path, "Stop", "", "say done", None).unwrap();
    let v = read_json(&path);
    assert_eq!(v["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
    assert_eq!(v["hooks"]["PreToolUse"][0]["hooks"][0]["timeout"], 10);
    assert!(v["hooks"]["Stop"][0].get("matcher").is_none());
    let c = parse_settings(&std::fs::read_to_string(&path).unwrap(), Scope::User);
    assert_eq!(c.hooks.len(), 3);
    remove_hook(&path, "PreToolUse", "Bash", "echo 1").unwrap();
    remove_hook(&path, "PreToolUse", "Bash", "echo 2").unwrap();
    remove_hook(&path, "Stop", "", "say done").unwrap();
    assert!(read_json(&path)["hooks"].as_object().unwrap().is_empty());
    assert!(add_hook(&path, "Stop", "", " ", None).is_err());
    let _ = std::fs::remove_dir_all(dir);
}
