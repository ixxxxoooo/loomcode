//! 项目信任:读写 `~/.claude.json` 里 `projects[<项目>].hasTrustDialogAccepted`。
//! 未信任的项目,Claude Code 首次打开时会先询问是否允许项目里的 Hooks / MCP / env。
//!
//! @author ixxxxoooo

use super::load::project_key;
use super::write::set_value;
use serde_json::Value;
use std::io::Result;
use std::path::Path;

/// 读取一批项目的信任状态。
///
/// 参数:`home` 家目录;`projects` 项目目录列表。返回:与输入同序的信任状态(没有记录视为未信任)。
pub fn read_trust(home: &Path, projects: &[String]) -> Vec<bool> {
    let v: Value = std::fs::read_to_string(home.join(".claude.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
    projects
        .iter()
        .map(|p| v.get("projects").and_then(|m| m.get(project_key(Path::new(p)))).and_then(|e| e.get("hasTrustDialogAccepted")).and_then(Value::as_bool).unwrap_or(false))
        .collect()
}

/// 设置某个项目的信任状态(原子写入并备份,保留其它字段)。
///
/// 参数:`home` 家目录;`project` 项目目录;`trusted` 目标状态。返回:IO 结果。
pub fn set_trust(home: &Path, project: &Path, trusted: bool) -> Result<()> {
    let key = project_key(project);
    set_value(&home.join(".claude.json"), &["projects", &key, "hasTrustDialogAccepted"], Some(Value::Bool(trusted)))
}

#[cfg(test)]
mod tests {
    use super::super::tests_write::{put, temp_dir};
    use super::*;

    #[test]
    fn 读写项目信任并保留其它字段() {
        let home = temp_dir("trust");
        put(&home.join(".claude.json"), r#"{"projects":{"/a":{"hasTrustDialogAccepted":true,"allowedTools":[]},"/b":{}},"x":1}"#);
        let list = vec!["/a".to_string(), "/b".to_string(), "/c/".to_string()];
        assert_eq!(read_trust(&home, &list), vec![true, false, false]);
        set_trust(&home, Path::new("/b"), true).unwrap();
        set_trust(&home, Path::new("/c/"), true).unwrap();
        set_trust(&home, Path::new("/a"), false).unwrap();
        assert_eq!(read_trust(&home, &list), vec![false, true, true]);
        let v: Value = serde_json::from_str(&std::fs::read_to_string(home.join(".claude.json")).unwrap()).unwrap();
        assert_eq!(v["x"], 1);
        assert!(v["projects"]["/a"]["allowedTools"].is_array());
        let _ = std::fs::remove_dir_all(home);
    }
}
