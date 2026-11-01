//! 插件读取、启停与参数校验的测试。
//!
//! @author ixxxxoooo

use super::*;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("loom-plugins-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn 读取已安装插件与市场() {
    let home = temp_dir("list");
    let proj = home.join("proj");
    let install = home.join(".claude/plugins/cache/m/demo/1.0.0");
    put(&install.join(".claude-plugin/plugin.json"), r#"{"description":"演示插件","author":{"name":"某人"}}"#);
    put(&install.join("commands/a.md"), "x");
    put(&install.join("commands/b.md"), "x");
    put(&install.join(".mcp.json"), "{}");
    let installed = format!(
        r#"{{"version":2,"plugins":{{
            "demo@m":[{{"scope":"user","version":"1.0.0","installPath":"{}"}}],
            "other@m":[{{"scope":"project","version":"2","installPath":"/nope","projectPath":"{}"}}],
            "elsewhere@m":[{{"scope":"project","version":"2","installPath":"/nope","projectPath":"/another"}}]}}}}"#,
        install.display(), proj.display()
    );
    put(&home.join(".claude/plugins/installed_plugins.json"), &installed);
    put(&home.join(".claude/plugins/known_marketplaces.json"), r#"{"m":{"source":{"source":"github","repo":"o/r"},"lastUpdated":"2026-11-01T04:54:26Z"}}"#);
    put(&home.join(".claude/settings.json"), r#"{"enabledPlugins":{"demo@m":true,"other@m":false}}"#);
    put(&proj.join(".claude/settings.local.json"), r#"{"enabledPlugins":{"other@m":true}}"#);

    let user_only = list_installed(&home, None);
    assert_eq!(user_only.len(), 1);
    let p = &user_only[0];
    assert_eq!((p.name.as_str(), p.market.as_str(), p.version.as_str(), p.enabled), ("demo", "m", "1.0.0", true));
    assert_eq!((p.description.as_str(), p.author.as_str()), ("演示插件", "某人"));
    assert_eq!(p.components, vec!["2 个命令", "MCP"]);

    let with_proj = list_installed(&home, Some(&proj));
    assert_eq!(with_proj.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["demo", "other"]);
    assert!(with_proj[1].enabled, "本地层的 true 覆盖用户层的 false");

    let markets = list_markets(&home);
    assert_eq!(markets, vec![Marketplace { name: "m".into(), source: "o/r".into(), updated: "2026-11-01".into() }]);
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn 启停写到指定层并保留其它字段() {
    let home = temp_dir("toggle");
    put(&home.join(".claude/settings.json"), r#"{"model":"opus"}"#);
    set_plugin_enabled(&home, None, Scope::User, "a@m", false).unwrap();
    set_plugin_enabled(&home, None, Scope::User, "a@m", true).unwrap();
    let v: Value = serde_json::from_str(&read(&home.join(".claude/settings.json"))).unwrap();
    assert_eq!(v["enabledPlugins"]["a@m"], true);
    assert_eq!(v["model"], "opus");
    assert!(set_plugin_enabled(&home, None, Scope::Project, "a@m", true).is_err());
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn 参数校验拒绝选项注入() {
    assert!(valid_plugin_id("demo@market"));
    assert!(valid_plugin_id("demo"));
    for bad in ["", "-rf", "--scope", "a b", "a@b@c", "a;b", "a\nb", "$(x)"] {
        assert!(!valid_plugin_id(bad), "{bad}");
    }
    assert!(valid_market_source("owner/repo"));
    assert!(valid_market_source("https://github.com/o/r.git"));
    assert!(valid_market_source("/Users/me/market"));
    for bad in ["", "-x", "--sparse", "a b", "../x"] {
        assert!(!valid_market_source(bad), "{bad}");
    }
    assert_eq!(plugin_args("install", "demo@m", Scope::Project).unwrap(), ["plugin", "install", "-s", "project", "demo@m"]);
    assert!(plugin_args("install", "--yes", Scope::User).is_none());
    assert!(plugin_args("rm -rf", "demo", Scope::User).is_none());
    assert_eq!(market_args("update", "").unwrap(), ["plugin", "marketplace", "update"]);
    assert_eq!(market_args("add", "o/r").unwrap(), ["plugin", "marketplace", "add", "o/r"]);
    assert!(market_args("remove", "-x").is_none());
}

#[test]
fn 解析市场插件清单() {
    let json = r#"{"name":"demo-mkt","plugins":[{"name":"a","description":"甲"},{"name":"b"},{"description":"无名"}]}"#;
    let list = parse_market_plugins("dir", json);
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].id(), "a@demo-mkt");
    assert_eq!(list[1].description, "");
    assert_eq!(parse_market_plugins("dir", r#"{"plugins":[{"name":"x"}]}"#)[0].market, "dir");
    assert!(parse_market_plugins("d", "不是 json").is_empty());
}
