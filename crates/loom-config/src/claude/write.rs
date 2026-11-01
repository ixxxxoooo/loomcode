//! 受控写入 Claude 配置文件:保留未知字段;覆盖前把原文件备份为 `<文件名>.loom-backup`;
//! 先写临时文件再改名(原子替换);原文件不是合法 JSON 对象时报错,不覆盖。
//!
//! @author ixxxxoooo

use serde_json::{Map, Value};
use std::io::{Error, Result};
use std::path::{Path, PathBuf};

/// 备份文件路径:`settings.json` → `settings.json.loom-backup`。
pub fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".loom-backup");
    path.with_file_name(name)
}

/// 读取 JSON 对象;文件不存在或为空时返回空对象。
fn read_object(path: &Path) -> Result<Value> {
    match std::fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(Value::Object(Map::new())),
        Ok(text) => {
            let v: Value = serde_json::from_str(&text).map_err(|e| Error::other(format!("{} 不是合法 JSON:{e}", path.display())))?;
            if v.is_object() { Ok(v) } else { Err(Error::other(format!("{} 的根不是对象", path.display()))) }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Value::Object(Map::new())),
        Err(e) => Err(e),
    }
}

/// 备份后原子写入格式化的 JSON。
fn write_object(path: &Path, root: &Value) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if path.exists() {
        std::fs::copy(path, backup_path(path))?;
    }
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, serde_json::to_string_pretty(root).map_err(Error::other)? + "\n")?;
    std::fs::rename(&tmp, path)
}

/// 读出 JSON、交给 `f` 修改、有变化才写回。
///
/// 参数:`path` 目标文件;`f` 修改函数,返回是否有变化。返回:IO 结果。
pub fn edit_json(path: &Path, f: impl FnOnce(&mut Value) -> Result<bool>) -> Result<()> {
    let mut root = read_object(path)?;
    if f(&mut root)? { write_object(path, &root) } else { Ok(()) }
}

/// 沿 `keys` 取(必要时创建)嵌套对象。
///
/// 参数:`root` 根对象;`keys` 键路径。返回:最内层对象;路径上有非对象值时报错。
pub fn object_at<'a>(root: &'a mut Value, keys: &[&str]) -> Result<&'a mut Map<String, Value>> {
    let mut cur = root;
    for k in keys {
        cur = cur.as_object_mut().ok_or_else(|| Error::other(format!("{k} 的上级不是对象")))?.entry(k.to_string()).or_insert_with(|| Value::Object(Map::new()));
    }
    cur.as_object_mut().ok_or_else(|| Error::other(format!("{} 不是对象", keys.join("."))))
}

/// 设置或删除一个键。
///
/// 参数:`path` 文件;`keys` 键路径(最后一个是要设置的键);`value` 新值,`None` 表示删除。返回:IO 结果。
pub fn set_value(path: &Path, keys: &[&str], value: Option<Value>) -> Result<()> {
    let Some((last, parents)) = keys.split_last() else { return Ok(()) };
    edit_json(path, |root| {
        let obj = object_at(root, parents)?;
        Ok(match value {
            Some(v) => obj.insert(last.to_string(), v.clone()).as_ref() != Some(&v),
            None => obj.remove(*last).is_some(),
        })
    })
}

/// 让字符串列表包含或不包含某一项;列表变空时删除该键。
///
/// 参数:`path` 文件;`keys` 列表的键路径;`item` 项;`present` 是否应包含。返回:IO 结果。
pub fn set_string_list(path: &Path, keys: &[&str], item: &str, present: bool) -> Result<()> {
    let Some((last, parents)) = keys.split_last() else { return Ok(()) };
    edit_json(path, |root| {
        let obj = object_at(root, parents)?;
        let list = obj.entry(last.to_string()).or_insert_with(|| Value::Array(Vec::new()));
        let arr = list.as_array_mut().ok_or_else(|| Error::other(format!("{last} 不是数组")))?;
        let has = arr.iter().any(|x| x.as_str() == Some(item));
        let changed = has != present;
        if present && !has {
            arr.push(Value::String(item.to_string()));
        } else if !present {
            arr.retain(|x| x.as_str() != Some(item));
        }
        if arr.is_empty() {
            obj.remove(*last);
        }
        Ok(changed)
    })
}

/// 向 `settings.json` 追加一条权限规则(已存在则不重复)。
///
/// 参数:`path` 目标层文件;`kind` 为 `allow` / `deny` / `ask`;`rule` 规则文本。返回:IO 结果。
pub fn add_permission_rule(path: &Path, kind: &str, rule: &str) -> Result<()> {
    set_string_list(path, &["permissions", kind], rule, true)
}

/// 从 `settings.json` 移除一条权限规则(文件或规则不存在时什么也不做)。
///
/// 参数:`path` 规则所在层文件;`kind` 种类;`rule` 规则文本。返回:IO 结果。
pub fn remove_permission_rule(path: &Path, kind: &str, rule: &str) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    set_string_list(path, &["permissions", kind], rule, false)
}
