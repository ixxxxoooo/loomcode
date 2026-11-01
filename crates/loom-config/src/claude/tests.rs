//! Claude 配置解析、分层加载与写入的测试。
//!
//! @author ixxxxoooo

use super::load::claude_json_mcp;
use super::*;
use std::path::Path;
pub(super) use super::tests_write::{put, temp_dir};

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

#[test]
fn 解析权限与钩子() {
    let c = parse_settings(SAMPLE, Scope::User);
    assert_eq!(texts(&c.permissions.allow), vec!["Bash(git status)", "Read"]);
    assert_eq!(c.permissions.deny.len(), 1);
    assert!(c.permissions.allow.iter().all(|r| r.scope == Scope::User));
    assert_eq!(c.hooks, vec![HookEntry { event: "PreToolUse".into(), matcher: "Bash".into(), kind: "command".into(), command: "echo hi".into(), timeout: None, scope: Scope::User }]);
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
    assert_eq!(c.mcp_servers[0].spec.target(), "npx -y server-fs");
    assert_eq!(c.mcp_servers[0].scope, Scope::Project);
    assert_eq!(c.mcp_servers[1].spec.transport, "http");
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
    let cj = claude_json_mcp(json, Some(Path::new("/p/")));
    assert_eq!(cj.local[0].name, "l");
    assert_eq!(cj.local[0].scope, Scope::Local);
    assert_eq!(cj.user[0].scope, Scope::User);
    assert!(claude_json_mcp(json, None).local.is_empty());
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
    assert_eq!((dup.spec.target(), dup.scope), ("local".to_string(), Scope::Local));
    assert_eq!(c.mcp_servers.len(), 3);
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn 解析钩子类型超时与层设置() {
    let json = r#"{"hooks":{"Stop":[{"hooks":[{"type":"http","url":"https://h","timeout":5}]}]},
        "permissions":{"defaultMode":"plan","additionalDirectories":["../lib"],"disableBypassPermissionsMode":"disable"},
        "sandbox":{"enabled":true},"enabledMcpjsonServers":["a"]}"#;
    let c = parse_settings(json, Scope::Project);
    assert_eq!((c.hooks[0].kind.as_str(), c.hooks[0].command.as_str(), c.hooks[0].timeout), ("http", "https://h", Some(5)));
    let l = c.layer(Scope::Project);
    assert_eq!(l.default_mode.as_deref(), Some("plan"));
    assert_eq!(l.additional_dirs, vec!["../lib"]);
    assert!(l.disable_bypass);
    assert_eq!(l.sandbox, Some(true));
    assert_eq!(l.enabled_mcpjson, vec!["a"]);
    assert_eq!(c.layer(Scope::User), LayerSettings::default());
}

#[test]
fn mcp_启用与批准状态() {
    let home = temp_dir("state");
    let proj = home.join("proj");
    put(&proj.join(".mcp.json"), r#"{"mcpServers":{"ok":{"command":"a"},"pending":{"command":"b"},"off":{"command":"c"}}}"#);
    put(&proj.join(".claude/settings.local.json"), r#"{"enabledMcpjsonServers":["ok"],"disabledMcpjsonServers":["off"]}"#);
    put(&home.join(".claude.json"), &format!(r#"{{"mcpServers":{{"u":{{"command":"u"}}}},"projects":{{"{}":{{"disabledMcpServers":["u"]}}}}}}"#, proj.display()));
    let c = load(&home, Some(&proj));
    let get = |n: &str| c.mcp_servers.iter().find(|m| m.name == n).unwrap().clone();
    assert!(get("ok").enabled && !get("ok").needs_approval);
    assert!(get("pending").enabled && get("pending").needs_approval);
    assert!(!get("off").enabled && !get("off").needs_approval);
    assert!(!get("u").enabled);
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn 层设置只读白名单_env_值() {
    let json = r#"{"model":"opus","alwaysThinkingEnabled":true,"outputStyle":"Explanatory","apiKeyHelper":"~/bin/key.sh",
        "env":{"MAX_THINKING_TOKENS":"8000","ANTHROPIC_AUTH_TOKEN":"secret-token","DISABLE_TELEMETRY":1}}"#;
    let l = parse_settings(json, Scope::User).layer(Scope::User);
    assert_eq!((l.model.as_deref(), l.always_thinking, l.output_style.as_deref()), (Some("opus"), Some(true), Some("Explanatory")));
    assert_eq!(l.env("MAX_THINKING_TOKENS"), Some("8000"));
    assert_eq!(l.env("DISABLE_TELEMETRY"), Some("1"));
    assert_eq!(l.env_names.len(), 3);
    assert!(!format!("{l:?}").contains("secret-token"));
}
