//! 执行一次自动化:`claude -p <提示词> --output-format json`,阻塞直到结束或超时。
//!
//! 子进程放在独立的进程组里,超时时整组结束(claude 跑的命令也一起结束,
//! 否则孙进程握着输出管道,读线程会一直等下去)。
//!
//! @author ixxxxoooo

use crate::{Automation, RunRecord, RunStatus, now_ts};
use serde_json::Value;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// 摘要最多保留的字符数。
const MAX_SUMMARY: usize = 2000;
/// 默认超时:30 分钟。
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// 轮询子进程是否结束的间隔。
const POLL: Duration = Duration::from_millis(200);

/// 运行参数。
#[derive(Debug, Clone)]
pub struct RunOptions {
    /// 额外环境变量(服务商映射、代理等,与交互会话一致)。
    pub env: Vec<(String, String)>,
    /// 超时时长,到点结束整个进程组并记为失败。
    pub timeout: Duration,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self { env: Vec::new(), timeout: DEFAULT_TIMEOUT }
    }
}

/// 运行一次并返回完成态的记录。启动失败、非零退出、`is_error`、超时都记为失败。
/// 参数:`claude` 可执行文件,`automation` 要运行的自动化,`opts` 环境变量与超时。返回:运行记录。
pub fn run_once(claude: &Path, automation: &Automation, opts: &RunOptions) -> RunRecord {
    let started_at = now_ts();
    let mut cmd = Command::new(claude);
    cmd.arg("-p")
        .arg(&automation.prompt)
        .args(["--output-format", "json", "--permission-mode", &automation.mode])
        .current_dir(&automation.project_path)
        .env_remove("CLAUDECODE")
        .envs(opts.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(model) = automation.model.as_deref().filter(|m| !m.is_empty() && *m != "default") {
        cmd.args(["--model", model]);
    }
    let finish = |status, summary: String, cost_usd, session_id| RunRecord {
        started_at,
        finished_at: Some(now_ts()),
        status,
        summary: truncate(&summary),
        cost_usd,
        session_id,
    };
    let spawned = {
        let _fork = loom_diag::fork_guard();
        cmd.spawn()
    };
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => return finish(RunStatus::Failed, format!("无法启动 claude:{e}"), 0.0, None),
    };
    let out = reader(child.stdout.take());
    let err = reader(child.stderr.take());
    let Some(success) = wait_with_timeout(&mut child, opts.timeout) else {
        loom_diag::warn!("自动化「{}」运行超过 {} 秒,已结束进程组 {}", automation.name, opts.timeout.as_secs(), child.id());
        let mins = opts.timeout.as_secs().div_ceil(60);
        return finish(RunStatus::Failed, format!("运行超时(超过 {mins} 分钟),已终止"), 0.0, None);
    };
    let stdout = String::from_utf8_lossy(&out.join().unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&err.join().unwrap_or_default()).into_owned();
    match serde_json::from_str::<Value>(stdout.trim()) {
        Ok(v) => {
            let text = v.get("result").and_then(Value::as_str).unwrap_or_default().to_string();
            let is_error = v.get("is_error").and_then(Value::as_bool).unwrap_or(false) || !success;
            let cost = v.get("total_cost_usd").and_then(Value::as_f64).unwrap_or(0.0);
            let session = v.get("session_id").and_then(Value::as_str).map(String::from);
            let status = if is_error { RunStatus::Failed } else { RunStatus::Succeeded };
            finish(status, text, cost, session)
        }
        Err(_) => {
            let detail = if stderr.trim().is_empty() { stdout.trim().to_string() } else { stderr.trim().to_string() };
            let status = if success { RunStatus::Succeeded } else { RunStatus::Failed };
            finish(status, detail, 0.0, None)
        }
    }
}

/// 在后台线程读完一个管道。
fn reader<R: Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut p) = pipe {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    })
}

/// 等子进程结束。参数:`child` 子进程,`timeout` 超时。返回:是否成功退出;超时则结束整组并返回 `None`。
fn wait_with_timeout(child: &mut Child, timeout: Duration) -> Option<bool> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status.success()),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(POLL),
            Ok(None) => break,
            Err(_) => return Some(false),
        }
    }
    // 进程组号 = 子进程 pid(`process_group(0)`),负号表示整组
    let _ = Command::new("/bin/kill").args(["-KILL", &format!("-{}", child.id())]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    let _ = child.kill();
    let _ = child.wait();
    None
}

fn truncate(s: &str) -> String {
    if s.chars().count() <= MAX_SUMMARY {
        return s.to_string();
    }
    format!("{}…", s.chars().take(MAX_SUMMARY).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::sample;
    use std::os::unix::fs::PermissionsExt;

    fn script(name: &str, body: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("loom-auto-{name}-{}.sh", std::process::id()));
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    fn run(p: &Path) -> RunRecord {
        run_once(p, &sample("a"), &RunOptions::default())
    }

    #[test]
    fn 成功的运行解析结果() {
        let s = script("ok", r#"echo '{"type":"result","is_error":false,"result":"一切正常","total_cost_usd":0.02,"session_id":"s1"}'"#);
        let r = run(&s);
        assert_eq!(r.status, RunStatus::Succeeded);
        assert_eq!(r.summary, "一切正常");
        assert_eq!(r.session_id.as_deref(), Some("s1"));
        assert!((r.cost_usd - 0.02).abs() < 1e-9);
        assert!(r.finished_at.is_some());
    }

    #[test]
    fn 业务错误与非零退出都算失败() {
        let s = script("err", r#"echo '{"is_error":true,"result":"鉴权失败"}'"#);
        assert_eq!(run(&s).status, RunStatus::Failed);
        let s = script("exit", "echo boom >&2; exit 2");
        let r = run(&s);
        assert_eq!(r.status, RunStatus::Failed);
        assert!(r.summary.contains("boom"));
    }

    #[test]
    fn 可执行文件不存在() {
        let r = run(Path::new("/不存在/claude"));
        assert_eq!(r.status, RunStatus::Failed);
        assert!(r.summary.contains("无法启动"));
    }

    #[test]
    fn 注入额外环境变量() {
        let s = script("env", r#"printf '{"result":"%s"}' "$LOOM_AUTO_TEST""#);
        let opts = RunOptions { env: vec![("LOOM_AUTO_TEST".into(), "已注入".into())], ..RunOptions::default() };
        assert_eq!(run_once(&s, &sample("a"), &opts).summary, "已注入");
    }

    #[test]
    fn 超时结束整个进程组() {
        // 子 shell 里的 sleep 是孙进程,也握着输出管道;只杀 claude 本身会让读线程一直等
        let s = script("slow", "sleep 30 & wait");
        let opts = RunOptions { timeout: Duration::from_millis(500), ..RunOptions::default() };
        let t = Instant::now();
        let r = run_once(&s, &sample("a"), &opts);
        assert_eq!(r.status, RunStatus::Failed);
        assert!(r.summary.contains("超时"), "{}", r.summary);
        assert!(t.elapsed() < Duration::from_secs(5), "超时后应很快返回");
    }
}
