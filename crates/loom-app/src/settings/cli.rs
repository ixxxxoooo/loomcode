//! 运行 claude 命令行的公共函数(阻塞,调用方在后台线程使用)。
//!
//! @author ixxxxoooo

use super::ops_actions::strip_ansi;
use std::path::Path;
use std::time::Duration;

/// 运行 `claude <args>`,等待至多 `secs` 秒。
///
/// 参数:`bin` claude 路径;`args` 参数(调用方已校验);`cwd` 工作目录(不存在则忽略);`env` 额外环境变量。
/// 返回:成功时的输出;失败或超时时的错误文字。输出已去颜色,家目录替换为 `~`。
pub fn run(bin: &Path, args: &[String], cwd: Option<&str>, env: Vec<(String, String)>, secs: u64) -> Result<String, String> {
    let mut cmd = std::process::Command::new(bin);
    cmd.args(args).envs(env).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    if let Some(dir) = cwd.filter(|d| Path::new(d).is_dir()) {
        cmd.current_dir(dir);
    }
    let mut child = cmd.spawn().map_err(|e| format!("无法运行 {}:{e}", bin.display()))?;
    for _ in 0..secs * 10 {
        if child.try_wait().ok().flatten().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let timed_out = child.try_wait().ok().flatten().is_none();
    if timed_out {
        let _ = child.kill();
    }
    let out = child.wait_with_output().map_err(|e| format!("读取输出失败:{e}"))?;
    let text = strip_ansi(&format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)));
    let text = loom_config::diag::redact_home(text.trim());
    if timed_out {
        Err(format!("超过 {secs} 秒未完成,已终止。{text}"))
    } else if out.status.success() {
        Ok(text)
    } else {
        Err(text)
    }
}
