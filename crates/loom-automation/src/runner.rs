//! 执行一次自动化:`claude -p <提示词> --output-format json`,阻塞直到结束。

use crate::{Automation, RunRecord, RunStatus, now_ts};
use serde_json::Value;
use std::path::Path;
use std::process::{Command, Stdio};

/// 摘要最多保留的字符数。
const MAX_SUMMARY: usize = 2000;

/// 运行一次并返回完成态的记录。启动失败、非零退出、`is_error` 都记为失败。
pub fn run_once(claude: &Path, automation: &Automation) -> RunRecord {
    let started_at = now_ts();
    let mut cmd = Command::new(claude);
    cmd.arg("-p")
        .arg(&automation.prompt)
        .args(["--output-format", "json", "--permission-mode", &automation.mode])
        .current_dir(&automation.project_path)
        .env_remove("CLAUDECODE")
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
    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => return finish(RunStatus::Failed, format!("无法启动 claude:{e}"), 0.0, None),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    match serde_json::from_str::<Value>(stdout.trim()) {
        Ok(v) => {
            let text = v.get("result").and_then(Value::as_str).unwrap_or_default().to_string();
            let is_error = v.get("is_error").and_then(Value::as_bool).unwrap_or(false) || !output.status.success();
            let cost = v.get("total_cost_usd").and_then(Value::as_f64).unwrap_or(0.0);
            let session = v.get("session_id").and_then(Value::as_str).map(String::from);
            let status = if is_error { RunStatus::Failed } else { RunStatus::Succeeded };
            finish(status, text, cost, session)
        }
        Err(_) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let detail = if stderr.trim().is_empty() { stdout.trim().to_string() } else { stderr.trim().to_string() };
            let status = if output.status.success() { RunStatus::Succeeded } else { RunStatus::Failed };
            finish(status, detail, 0.0, None)
        }
    }
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

    #[test]
    fn 成功的运行解析结果() {
        let s = script("ok", r#"echo '{"type":"result","is_error":false,"result":"一切正常","total_cost_usd":0.02,"session_id":"s1"}'"#);
        let r = run_once(&s, &sample("a"));
        assert_eq!(r.status, RunStatus::Succeeded);
        assert_eq!(r.summary, "一切正常");
        assert_eq!(r.session_id.as_deref(), Some("s1"));
        assert!((r.cost_usd - 0.02).abs() < 1e-9);
        assert!(r.finished_at.is_some());
    }

    #[test]
    fn 业务错误与非零退出都算失败() {
        let s = script("err", r#"echo '{"is_error":true,"result":"鉴权失败"}'"#);
        assert_eq!(run_once(&s, &sample("a")).status, RunStatus::Failed);
        let s = script("exit", "echo boom >&2; exit 2");
        let r = run_once(&s, &sample("a"));
        assert_eq!(r.status, RunStatus::Failed);
        assert!(r.summary.contains("boom"));
    }

    #[test]
    fn 可执行文件不存在() {
        let r = run_once(Path::new("/不存在/claude"), &sample("a"));
        assert_eq!(r.status, RunStatus::Failed);
        assert!(r.summary.contains("无法启动"));
    }
}
