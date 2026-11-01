//! 系统崩溃报告(`~/Library/Logs/DiagnosticReports/loomcode-*.ips`)的导入:
//! 解析出异常类型、终止原因、崩溃线程的调用栈,写成一条 `CrashRecord`。
//!
//! @author ixxxxoooo

use crate::records::CrashRecord;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// 解析后的 .ips 摘要。
#[derive(Debug, Clone, PartialEq)]
pub struct IpsSummary {
    pub pid: u32,
    pub app_name: String,
    pub os_version: String,
    /// 进程启动与崩溃捕获时间(本地时间串,已截到秒)
    pub launch: String,
    pub captured: String,
    pub exception: String,
    pub termination: String,
    pub thread_name: String,
    /// 崩溃线程的前若干帧(已还原 Rust 符号名)
    pub frames: Vec<String>,
    /// 应用特定信息(如 `abort() called`)
    pub notes: Vec<String>,
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// 截到秒:`2026-11-01 21:08:32.8179 +0800` → `2026-11-01 21:08:32`。
fn to_secs(t: &str) -> String {
    t.chars().take(19).collect()
}

/// 解析 .ips 文本(第一行是头部 JSON,其后是正文 JSON)。
pub fn parse(text: &str) -> Option<IpsSummary> {
    let (head, body) = text.split_once('\n')?;
    let head: Value = serde_json::from_str(head).ok()?;
    let body: Value = serde_json::from_str(body).ok()?;
    let fault = body.get("faultingThread").and_then(Value::as_u64).unwrap_or(0) as usize;
    let thread = body.get("threads").and_then(Value::as_array).and_then(|t| t.get(fault));
    let images: Vec<String> = body.get("usedImages").and_then(Value::as_array).map(|a| a.iter().map(|i| s(i, "name")).collect()).unwrap_or_default();
    let frames = thread
        .and_then(|t| t.get("frames"))
        .and_then(Value::as_array)
        .map(|fs| {
            fs.iter()
                .take(14)
                .map(|f| {
                    let sym = f.get("symbol").and_then(Value::as_str).map(|x| rustc_demangle::demangle(x).to_string()).unwrap_or_else(|| "?".into());
                    let img = f.get("imageIndex").and_then(Value::as_u64).and_then(|i| images.get(i as usize)).cloned().unwrap_or_default();
                    format!("{img} {sym}")
                })
                .collect()
        })
        .unwrap_or_default();
    let exc = body.get("exception").cloned().unwrap_or(Value::Null);
    let term = body.get("termination").cloned().unwrap_or(Value::Null);
    let notes = body
        .get("asi")
        .and_then(Value::as_object)
        .map(|m| m.values().filter_map(Value::as_array).flatten().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    Some(IpsSummary {
        pid: body.get("pid").and_then(Value::as_u64).unwrap_or(0) as u32,
        app_name: s(&head, "app_name"),
        os_version: s(&head, "os_version"),
        launch: to_secs(&s(&body, "procLaunch")),
        captured: to_secs(&s(&body, "captureTime")),
        exception: format!("{} ({})", s(&exc, "type"), s(&exc, "signal")),
        termination: s(&term, "indicator"),
        thread_name: thread.map(|t| s(t, "name")).filter(|n| !n.is_empty()).unwrap_or_else(|| format!("线程 {fault}")),
        frames,
        notes,
    })
}

impl IpsSummary {
    /// 转成统一的崩溃记录。
    pub fn to_record(&self, file: &str, version: &str) -> CrashRecord {
        let mut detail = format!("来源:{file}\n系统:{}\n进程启动:{}\n崩溃时间:{}\n异常:{}\n终止原因:{}\n崩溃线程:{}\n", self.os_version, self.launch, self.captured, self.exception, self.termination, self.thread_name);
        if !self.notes.is_empty() {
            detail.push_str(&format!("附加信息:{}\n", self.notes.join(" / ")));
        }
        detail.push_str("调用栈(前几帧):\n");
        for (i, f) in self.frames.iter().enumerate() {
            detail.push_str(&format!("{i:>3} {f}\n"));
        }
        CrashRecord {
            kind: "ips".into(),
            time: self.captured.clone(),
            pid: self.pid,
            run_id: String::new(),
            version: version.into(),
            mode: String::new(),
            uptime_s: 0,
            summary: format!("{} · {} · 线程 {}", self.exception, self.termination, self.thread_name),
            detail: crate::logger::redact_home(&detail),
            path: PathBuf::new(),
        }
    }
}

/// 系统崩溃报告目录。
pub fn reports_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Logs/DiagnosticReports"))
}

/// 目录里属于本程序的 .ips 文件名(进程名 `loomcode` 开头,忽略大小写),按文件名排序。
pub fn list_reports(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<PathBuf> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            let n = p.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_ascii_lowercase();
            n.starts_with("loomcode-") && n.ends_with(".ips")
        })
        .collect();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../tests/fixtures/sample.ips");

    #[test]
    fn 解析真实报告() {
        let r = parse(SAMPLE).unwrap();
        assert_eq!(r.pid, 30070);
        assert_eq!(r.app_name, "loomcode");
        assert!(r.exception.contains("SIGABRT"), "{}", r.exception);
        assert_eq!(r.termination, "Abort trap: 6");
        assert_eq!(r.thread_name, "main");
        assert_eq!(r.captured, "2026-11-01 21:08:32");
        assert_eq!(r.launch, "2026-11-01 21:06:34");
        assert!(r.notes.iter().any(|n| n.contains("abort() called")));
        // Rust 符号被还原成可读名字
        assert!(r.frames.iter().any(|f| f.contains("panic_cannot_unwind")), "{:?}", r.frames);
        assert!(!r.frames.iter().any(|f| f.contains("_RNv")));
    }

    #[test]
    fn 转成记录() {
        let rec = parse(SAMPLE).unwrap().to_record("loomcode-x.ips", "0.1.0");
        assert_eq!(rec.kind, "ips");
        assert_eq!(rec.pid, 30070);
        assert!(rec.summary.contains("Abort trap"));
        assert!(rec.detail.contains("调用栈"));
    }

    #[test]
    fn 非法内容返回空() {
        assert!(parse("不是 json").is_none());
        assert!(parse("{}\n不是 json").is_none());
    }

    #[test]
    fn 只列出本程序的报告() {
        let dir = std::env::temp_dir().join(format!("loom-ips-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for n in ["loomcode-2026-11-01-1.ips", "mysqld-2026-11-01-1.ips", "LoomCode-2026-11-02-1.ips", "loomcode-x.txt"] {
            std::fs::write(dir.join(n), "").unwrap();
        }
        assert_eq!(list_reports(&dir).len(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }
}
