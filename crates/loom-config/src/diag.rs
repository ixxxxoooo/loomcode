//! 诊断:落盘日志(`~/.loomcode/logs/loomcode.log`,超过 2 MB 轮转一份)、读取末尾、导出诊断包、
//! 进程资源采样、连通性测试。日志与诊断包里家目录一律显示为 `~`,不写入任何密钥。
//!
//! @author ixxxxoooo

use std::path::{Path, PathBuf};

pub use loom_diag::{log_path, redact_home, tail};

/// 初始化日志(只生效一次):日志、崩溃记录、运行标记都放在 `<home>/.loomcode`。
///
/// 参数:`home` 用户家目录。
pub fn init(home: &Path) {
    loom_diag::init_home(home);
}

/// 写一条日志(旧接口:级别按字符串给出)。
pub fn log(level: &str, msg: &str) {
    loom_diag::log_str(level, msg);
}

/// 错误日志。
pub fn error(msg: &str) {
    loom_diag::log(loom_diag::Level::Error, "loom_config", format_args!("{msg}"));
}

/// 普通日志。
pub fn info(msg: &str) {
    loom_diag::log(loom_diag::Level::Info, "loom_config", format_args!("{msg}"));
}

/// 当前本地时间(`YYYY-MM-DD HH:MM:SS`)。
fn now() -> String {
    loom_diag::timestamp().chars().take(19).collect()
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
