//! 钩子的增删:`hooks.<事件>[] = {matcher, hooks: [{type, command, timeout}]}`。
//!
//! @author ixxxxoooo

use super::write::{edit_json, object_at};
use serde_json::{Map, Value, json};
use std::io::{Error, Result};
use std::path::Path;

/// 常用钩子事件(界面下拉用)。
pub const HOOK_EVENTS: &[&str] = &["PreToolUse", "PostToolUse", "UserPromptSubmit", "Notification", "Stop", "SubagentStop", "PreCompact", "SessionStart", "SessionEnd"];

/// 追加一条 command 类型的钩子;同事件下已有相同 matcher 的分组就并进去。
///
/// 参数:`path` 目标层 settings.json;`event` 事件;`matcher` 工具匹配(可空);`command` 命令;`timeout` 超时秒数。返回:IO 结果。
pub fn add_hook(path: &Path, event: &str, matcher: &str, command: &str, timeout: Option<u64>) -> Result<()> {
    if event.trim().is_empty() || command.trim().is_empty() {
        return Err(Error::other("事件和命令不能为空"));
    }
    edit_json(path, |root| {
        let hooks = object_at(root, &["hooks"])?;
        let groups = hooks.entry(event.to_string()).or_insert_with(|| Value::Array(Vec::new())).as_array_mut().ok_or_else(|| Error::other("钩子事件不是数组"))?;
        let mut handler = Map::new();
        handler.insert("type".into(), json!("command"));
        handler.insert("command".into(), json!(command.trim()));
        if let Some(t) = timeout {
            handler.insert("timeout".into(), json!(t));
        }
        let same = groups.iter_mut().find(|g| g.get("matcher").and_then(Value::as_str).unwrap_or("") == matcher);
        match same.and_then(|g| g.get_mut("hooks")).and_then(Value::as_array_mut) {
            Some(list) => list.push(Value::Object(handler)),
            None => {
                let mut g = Map::new();
                if !matcher.is_empty() {
                    g.insert("matcher".into(), json!(matcher));
                }
                g.insert("hooks".into(), json!([handler]));
                groups.push(Value::Object(g));
            }
        }
        Ok(true)
    })
}

/// 删除第一条匹配「事件 + matcher + 命令」的钩子;分组、事件变空时一并删除。
///
/// 参数:`path` 所在层 settings.json;`event` / `matcher` / `command` 定位。返回:IO 结果。
pub fn remove_hook(path: &Path, event: &str, matcher: &str, command: &str) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    edit_json(path, |root| {
        let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else { return Ok(false) };
        let Some(groups) = hooks.get_mut(event).and_then(Value::as_array_mut) else { return Ok(false) };
        let body = |h: &Value| ["command", "url", "prompt"].iter().find_map(|k| h.get(*k).and_then(Value::as_str)).map(str::to_string);
        let mut removed = false;
        for g in groups.iter_mut() {
            if removed || g.get("matcher").and_then(Value::as_str).unwrap_or("") != matcher {
                continue;
            }
            if let Some(list) = g.get_mut("hooks").and_then(Value::as_array_mut) {
                if let Some(i) = list.iter().position(|h| body(h).as_deref() == Some(command)) {
                    list.remove(i);
                    removed = true;
                }
            }
        }
        groups.retain(|g| g.get("hooks").and_then(Value::as_array).is_some_and(|l| !l.is_empty()));
        if groups.is_empty() {
            hooks.remove(event);
        }
        Ok(removed)
    })
}
