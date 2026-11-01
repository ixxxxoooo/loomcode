//! 崩溃与异常记录:每条一个 JSON 文件,放在 `~/.loomcode/crashes/`。
//! 诊断页列出它们,导出诊断包时附上摘要。
//!
//! @author ixxxxoooo

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 一条崩溃 / 异常记录。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CrashRecord {
    /// panic / signal / nsexception / ips / abnormal-exit / hang
    pub kind: String,
    /// 本地时间串
    pub time: String,
    pub pid: u32,
    pub run_id: String,
    pub version: String,
    pub mode: String,
    /// 出事时已运行的秒数
    pub uptime_s: u64,
    /// 一句话摘要
    pub summary: String,
    /// 完整细节(消息、调用栈、面包屑、日志尾部),可整体复制
    pub detail: String,
    /// 记录文件路径(读取时填写,不写入文件)
    #[serde(skip)]
    pub path: PathBuf,
}

impl CrashRecord {
    /// 类别的中文名。
    pub fn kind_label(&self) -> &'static str {
        match self.kind.as_str() {
            "panic" => "程序崩溃(panic)",
            "signal" => "原生崩溃(信号)",
            "nsexception" => "Objective-C 异常",
            "ips" => "系统崩溃报告",
            "abnormal-exit" => "异常退出",
            "hang" => "主线程卡死",
            _ => "其他",
        }
    }
}

/// 文件名用的时间戳 `YYYYMMDD-HHMMSS`(取自本地时间串)。
pub fn file_stamp() -> String {
    let t = crate::logger::timestamp();
    t.chars().take(19).filter(|c| c.is_ascii_digit() || *c == ' ').map(|c| if c == ' ' { '-' } else { c }).collect()
}

/// 写入一条记录,返回文件路径;目录未初始化或写入失败返回 `None`。
pub fn write(rec: &CrashRecord) -> Option<PathBuf> {
    write_in(crate::logger::dirs()?.crashes.as_path(), rec)
}

/// 写到指定目录(测试用)。同一秒内重名时加序号。
pub fn write_in(dir: &Path, rec: &CrashRecord) -> Option<PathBuf> {
    let _ = std::fs::create_dir_all(dir);
    // 文件名 `时间戳-序号-类别-pid`:同一秒内靠序号保持字典序等于时间序
    let stamp = file_stamp();
    let mut n = 1;
    let mut path = dir.join(format!("{stamp}-{n:02}-{}-{}.json", rec.kind, rec.pid));
    while path.exists() {
        n += 1;
        path = dir.join(format!("{stamp}-{n:02}-{}-{}.json", rec.kind, rec.pid));
    }
    let text = serde_json::to_string_pretty(rec).ok()?;
    std::fs::write(&path, text).ok()?;
    Some(path)
}

/// 读取目录里的全部记录,新的在前;最多 `limit` 条。
pub fn list_in(dir: &Path, limit: usize) -> Vec<CrashRecord> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<PathBuf> = rd.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
    // 文件名以时间戳开头,字典序即时间序
    files.sort();
    files.reverse();
    files
        .into_iter()
        .filter_map(|p| {
            let mut rec: CrashRecord = serde_json::from_str(&std::fs::read_to_string(&p).ok()?).ok()?;
            rec.path = p;
            Some(rec)
        })
        .take(limit)
        .collect()
}

/// 列出 `~/.loomcode/crashes` 里的记录。
pub fn list(limit: usize) -> Vec<CrashRecord> {
    crate::logger::dirs().map(|d| list_in(&d.crashes, limit)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 写入后可读回且新的在前() {
        let dir = std::env::temp_dir().join(format!("loom-rec-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let a = CrashRecord { kind: "panic".into(), pid: 1, summary: "甲".into(), ..Default::default() };
        let p1 = write_in(&dir, &a).unwrap();
        let p2 = write_in(&dir, &CrashRecord { summary: "乙".into(), ..a.clone() }).unwrap();
        assert_ne!(p1, p2, "同一秒内重名要加序号");
        let all = list_in(&dir, 10);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].path, p2);
        assert_eq!(list_in(&dir, 1).len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn 时间戳格式() {
        let s = file_stamp();
        assert_eq!(s.len(), 15, "{s}");
        assert_eq!(&s[8..9], "-");
    }
}
