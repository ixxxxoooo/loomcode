//! Claude 配置解析、分层加载与写入的测试。
//!
//! @author ixxxxoooo

use super::load::claude_json_mcp;
use super::*;
use std::path::{Path, PathBuf};

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

fn texts(rules: &[Rule]) -> Vec<&str> {
    rules.iter().map(|r| r.text.as_str()).collect()
}

/// 每个测试用独立临时目录。
fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("loom-claude-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn 解析权限与钩子() {
    let c = parse_settings(SAMPLE, Scope::User);
    assert_eq!(texts(&c.permissions.allow), vec!["Bash(git status)", "Read"]);
    assert_eq!(c.permissions.deny.len(), 1);
    assert!(c.permissions.allow.iter().all(|r| r.scope == Scope::User));
    assert_eq!(c.hooks, vec![HookEntry { event: "PreToolUse".into(), matcher: "Bash".into(), command: "echo hi".into(), scope: Scope::User }]);
    assert_eq!(c.default_model.as_deref(), Some("opus"));
}

#[test]
fn 环境变量只保留名称() {
    let c = parse_settings(SAMPLE, Scope::User);
    assert_eq!(c.env_names, vec!["ANTHROPIC_AUTH_TOKEN", "FOO"]);
    assert!(!format!("{c:?}").contains("secret-value"));
}

#[test]
fn 解析_mcp_服务器() {
    let c = parse_settings(SAMPLE, Scope::Project);
    assert_eq!(c.mcp_servers[0], McpServer { name: "fs".into(), transport: "stdio".into(), target: "npx -y server-fs".into(), enabled: true, scope: Scope::Project });
    assert_eq!(c.mcp_servers[1].transport, "http");
    assert!(!c.mcp_servers[1].enabled);
}

#[test]
fn 合并按来源层去重() {
    let a = parse_settings(r#"{"permissions":{"allow":["A","B"]}}"#, Scope::User);
    let b = parse_settings(r#"{"permissions":{"allow":["B","C"]}}"#, Scope::User);
    assert_eq!(texts(&merge(a.clone(), b).permissions.allow), vec!["A", "B", "C"]);
    let c = parse_settings(r#"{"permissions":{"allow":["B"]}}"#, Scope::Local);
    let m = merge(a, c);
    assert_eq!(m.permissions.allow.len(), 3);
    assert_eq!(m.permissions.allow[2].scope, Scope::Local);
}

#[test]
fn 读取_claude_json_的用户与本地_mcp() {
    let json = r#"{"mcpServers":{"u":{"command":"a"}},"projects":{"/p":{"mcpServers":{"l":{"url":"https://l"}}}}}"#;
    let (local, user) = claude_json_mcp(json, Some(Path::new("/p")));
    assert_eq!(local[0].name, "l");
    assert_eq!(local[0].scope, Scope::Local);
    assert_eq!(user[0].scope, Scope::User);
    assert!(claude_json_mcp(json, None).0.is_empty());
}

#[test]
fn 分层加载与_mcp_优先级() {
    let home = temp_dir("home");
    let proj = home.join("proj");
    put(&home.join(".claude/settings.json"), r#"{"permissions":{"allow":["U"]}}"#);
    put(&proj.join(".claude/settings.json"), r#"{"permissions":{"deny":["P"]}}"#);
    put(&proj.join(".claude/settings.local.json"), r#"{"permissions":{"ask":["L"]}}"#);
    put(&proj.join(".mcp.json"), r#"{"mcpServers":{"dup":{"command":"proj"},"p":{"command":"p"}}}"#);
    let claude_json = format!(r#"{{"mcpServers":{{"dup":{{"command":"user"}},"u":{{"command":"u"}}}},"projects":{{"{}":{{"mcpServers":{{"dup":{{"command":"local"}}}}}}}}}}"#, proj.display());
    put(&home.join(".claude.json"), &claude_json);

    let only_user = load(&home, None);
    assert_eq!(texts(&only_user.permissions.allow), vec!["U"]);
    assert!(only_user.permissions.deny.is_empty());

    let c = load(&home, Some(&proj));
    assert_eq!(c.permissions.deny[0], Rule { text: "P".into(), scope: Scope::Project });
    assert_eq!(c.permissions.ask[0].scope, Scope::Local);
    let dup = c.mcp_servers.iter().find(|m| m.name == "dup").unwrap();
    assert_eq!((dup.target.as_str(), dup.scope), ("local", Scope::Local));
    assert_eq!(c.mcp_servers.len(), 3);
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn 增删权限规则保留其余字段() {
    let dir = temp_dir("rw");
    let path = dir.join(".claude/settings.local.json");
    put(&path, r#"{"model":"opus"}"#);
    add_permission_rule(&path, "allow", "Bash(ls)").unwrap();
    add_permission_rule(&path, "allow", "Bash(ls)").unwrap();
    let c = parse_settings(&std::fs::read_to_string(&path).unwrap(), Scope::Local);
    assert_eq!(texts(&c.permissions.allow), vec!["Bash(ls)"]);
    assert_eq!(c.default_model.as_deref(), Some("opus"));
    remove_permission_rule(&path, "allow", "Bash(ls)").unwrap();
    assert!(parse_settings(&std::fs::read_to_string(&path).unwrap(), Scope::Local).permissions.allow.is_empty());
    assert!(!path.with_extension("json.tmp").exists());
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
