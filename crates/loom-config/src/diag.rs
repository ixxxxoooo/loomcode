//! 诊断:落盘日志(`~/.loomcode/logs/loomcode.log`,超过 2 MB 轮转一份)、读取末尾、导出诊断包、
//! 进程资源采样、连通性测试。日志与诊断包里家目录一律显示为 `~`,不写入任何密钥。
//!
//! @author ixxxxoooo

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static LOCK: Mutex<()> = Mutex::new(());
const MAX_LOG: u64 = 2 * 1024 * 1024;

/// 设定日志文件(只生效一次)。
///
/// 参数:`dir` 日志目录。
pub fn init(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    let _ = LOG_PATH.set(dir.join("loomcode.log"));
}

/// 当前日志文件路径。
pub fn log_path() -> Option<&'static Path> {
    LOG_PATH.get().map(PathBuf::as_path)
}

/// 把家目录替换为 `~`。
pub fn redact_home(text: &str) -> String {
    match std::env::var("HOME") {
        Ok(h) if !h.is_empty() => text.replace(&h, "~"),
        _ => text.to_string(),
    }
}

/// 当前本地时间(`YYYY-MM-DD HH:MM:SS`),取自系统 `date`,失败时用 Unix 秒。
fn now() -> String {
    std::process::Command::new("date").arg("+%Y-%m-%d %H:%M:%S").output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs().to_string()).unwrap_or_default()
    })
}

/// 写一条日志,同时打印到标准错误(开发版的 `target/dev.log` 也能看到)。
///
/// 参数:`level` 级别(INFO / WARN / ERROR);`msg` 内容。
pub fn log(level: &str, msg: &str) {
    let line = format!("{} [{level}] {}", now(), redact_home(msg));
    eprintln!("{line}");
    let Some(path) = LOG_PATH.get() else { return };
    let _guard = LOCK.lock();
    if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_LOG) {
        let _ = std::fs::rename(path, path.with_extension("log.1"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}

/// 错误日志。
pub fn error(msg: &str) {
    log("ERROR", msg);
}

/// 普通日志。
pub fn info(msg: &str) {
    log("INFO", msg);
}

/// 日志末尾 `n` 行。
pub fn tail(n: usize) -> Vec<String> {
    let Some(path) = LOG_PATH.get() else { return Vec::new() };
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].iter().map(|s| s.to_string()).collect()
}

/// 本进程及子进程的资源占用:(pid, 内存 MB, CPU %, 命令名)。
pub fn process_stats() -> Vec<(u32, f64, f64, String)> {
    let me = std::process::id();
    let Ok(out) = std::process::Command::new("ps").args(["-A", "-o", "pid=,ppid=,rss=,%cpu=,comm="]).output() else { return Vec::new() };
    let rows: Vec<(u32, u32, f64, f64, String)> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let (pid, ppid, rss, cpu) = (it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse::<f64>().ok()?, it.next()?.parse().ok()?);
            let comm = it.collect::<Vec<_>>().join(" ");
            Some((pid, ppid, rss / 1024., cpu, comm.rsplit('/').next().unwrap_or("").to_string()))
        })
        .collect();
    // 本进程 + 子进程 + 孙进程(claude 由 shell 包装时)
    let mut keep = vec![me];
    for _ in 0..3 {
        for r in &rows {
            if keep.contains(&r.1) && !keep.contains(&r.0) {
                keep.push(r.0);
            }
        }
    }
    rows.into_iter().filter(|r| keep.contains(&r.0)).map(|r| (r.0, r.2, r.3, r.4)).collect()
}

/// 用 curl 测一个网址的连通性(遵守代理设置;任何 HTTP 状态都算连通)。
///
/// 参数:`url` 网址;`proxy` 代理地址(空为沿用环境变量)。返回:耗时毫秒或错误提示。
pub fn probe(url: &str, proxy: Option<&str>) -> Result<u64, String> {
    let mut cmd = std::process::Command::new("curl");
    cmd.args(["-s", "-o", "/dev/null", "-w", "%{http_code} %{time_total}", "--max-time", "8"]);
    if let Some(p) = proxy.filter(|p| !p.is_empty()) {
        cmd.args(["-x", p]);
    }
    let out = cmd.arg(url).output().map_err(|e| format!("无法运行 curl:{e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let mut it = text.split_whitespace();
    let code = it.next().unwrap_or("000");
    let secs: f64 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0.);
    if code == "000" { Err("连接失败或超时".into()) } else { Ok((secs * 1000.) as u64) }
}

/// 导出诊断包(纯文本):调用方给出的概要信息 + 最近 500 行日志,家目录替换为 `~`。
///
/// 参数:`dir` 输出目录;`sections` (标题, 内容) 列表。返回:生成的文件路径。
pub fn export_bundle(dir: &Path, sections: &[(&str, String)]) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let stamp = now().replace([' ', ':'], "-");
    let path = dir.join(format!("loomcode-diagnostics-{stamp}.txt"));
    let mut text = String::from("LoomCode 诊断信息\n=================\n\n");
    for (title, body) in sections {
        text.push_str(&format!("## {title}\n{body}\n\n"));
    }
    text.push_str("## 最近日志\n");
    text.push_str(&tail(500).join("\n"));
    std::fs::write(&path, redact_home(&text))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 家目录脱敏() {
        let home = std::env::var("HOME").unwrap_or_default();
        if !home.is_empty() {
            assert_eq!(redact_home(&format!("{home}/.claude/settings.json")), "~/.claude/settings.json");
        }
    }

    #[test]
    fn 进程采样包含自身() {
        let me = std::process::id();
        assert!(process_stats().iter().any(|p| p.0 == me));
    }

    #[test]
    fn 导出诊断包() {
        let dir = std::env::temp_dir().join(format!("loom-diag-{}", std::process::id()));
        let p = export_bundle(&dir, &[("版本", "1.0".into())]).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("## 版本\n1.0"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
