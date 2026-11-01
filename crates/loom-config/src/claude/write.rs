//! 受控写入 Claude 配置文件:保留未知字段,先写临时文件再改名(原子替换)。
//!
//! @author ixxxxoooo

use serde_json::{Map, Value};
use std::io::{Error, Result};
use std::path::Path;

/// 读取 JSON 对象;文件不存在时返回空对象,内容不是合法 JSON 对象时报错(避免覆盖用户文件)。
fn read_object(path: &Path) -> Result<Value> {
    match std::fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(Value::Object(Map::new())),
        Ok(text) => {
            let v: Value = serde_json::from_str(&text).map_err(|e| Error::other(format!("{} 不是合法 JSON:{e}", path.display())))?;
            if v.is_object() { Ok(v) } else { Err(Error::other("配置根不是对象")) }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Value::Object(Map::new())),
        Err(e) => Err(e),
    }
}

/// 原子写入格式化后的 JSON。
fn write_object(path: &Path, root: &Value) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(root).map_err(Error::other)? + "\n")?;
    std::fs::rename(&tmp, path)
}

/// 向 `settings.json` 追加一条权限规则(已存在则不重复)。
///
/// 参数:`path` 目标层文件;`kind` 为 `allow` / `deny` / `ask`;`rule` 规则文本。返回:IO 结果。
pub fn add_permission_rule(path: &Path, kind: &str, rule: &str) -> Result<()> {
    let mut root = read_object(path)?;
    let obj = root.as_object_mut().ok_or_else(|| Error::other("配置根不是对象"))?;
    let perms = obj.entry("permissions").or_insert_with(|| Value::Object(Map::new()));
    let list = perms
        .as_object_mut()
        .ok_or_else(|| Error::other("permissions 不是对象"))?
        .entry(kind.to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let arr = list.as_array_mut().ok_or_else(|| Error::other("规则列表不是数组"))?;
    if !arr.iter().any(|x| x.as_str() == Some(rule)) {
        arr.push(Value::String(rule.to_string()));
    }
    write_object(path, &root)
}

/// 从 `settings.json` 移除一条权限规则(文件或规则不存在时什么也不做)。
///
/// 参数:`path` 规则所在层文件;`kind` 种类;`rule` 规则文本。返回:IO 结果。
pub fn remove_permission_rule(path: &Path, kind: &str, rule: &str) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let mut root = read_object(path)?;
    let Some(arr) = root.get_mut("permissions").and_then(|p| p.get_mut(kind)).and_then(Value::as_array_mut) else { return Ok(()) };
    let before = arr.len();
    arr.retain(|x| x.as_str() != Some(rule));
    if arr.len() == before {
        return Ok(());
    }
    write_object(path, &root)
}
