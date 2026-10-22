//! 启动 `claude` 子进程所需的参数、环境变量,以及可执行文件的查找。

use std::path::{Path, PathBuf};
use ucode_core::{BackendConfig, Effort, PermissionMode};

/// 启动选项。
#[derive(Debug, Clone, Default)]
pub struct ClaudeOptions {
    /// 工作目录
    pub cwd: String,
    /// 新会话使用的 id(与 `resume` 互斥)
    pub session_id: Option<String>,
    /// 恢复已有会话
    pub resume: Option<String>,
    pub model: Option<String>,
    pub mode: Option<PermissionMode>,
    pub effort: Option<Effort>,
    /// 额外授权访问的目录(`--add-dir`)
    pub add_dirs: Vec<String>,
    /// 预先允许的工具规则(`--allowedTools`)
    pub allowed_tools: Vec<String>,
    /// MCP 配置(JSON 文件路径或字符串,`--mcp-config`)
    pub mcp_config: Option<String>,
}

impl ClaudeOptions {
    /// 由界面层的后端配置转换而来;没有恢复会话时自动分配一个新的会话 id。
    pub fn from_config(config: &BackendConfig) -> Self {
        let session_id = if config.resume.is_none() { Some(uuid::Uuid::new_v4().to_string()) } else { None };
        Self {
            cwd: config.cwd.clone(),
            session_id,
            resume: config.resume.clone(),
            model: config.model.clone(),
            mode: config.mode,
            effort: config.effort,
            ..Default::default()
        }
    }

    /// 当前会话的 id(新建时是预分配的,恢复时是被恢复的那个)。
    pub fn effective_session_id(&self) -> String {
        self.resume.clone().or_else(|| self.session_id.clone()).unwrap_or_default()
    }

    /// 命令行参数。
    pub fn args(&self) -> Vec<String> {
        let mut args: Vec<String> = [
            "--output-format",
            "stream-json",
            "--verbose",
            "--input-format",
            "stream-json",
            "--include-partial-messages",
            "--permission-prompt-tool=stdio",
        ]
        .into_iter()
        .map(String::from)
        .collect();

        let mut push = |flag: &str, value: &str| {
            args.push(flag.into());
            args.push(value.into());
        };
        if let Some(resume) = &self.resume {
            push("--resume", resume);
        } else if let Some(id) = &self.session_id {
            push("--session-id", id);
        }
        if let Some(model) = self.model.as_deref().filter(|m| !m.is_empty() && *m != "default") {
            push("--model", model);
        }
        if let Some(mode) = self.mode {
            push("--permission-mode", mode.cli_value());
        }
        if let Some(effort) = self.effort {
            push("--effort", effort.cli_value());
        }
        for dir in &self.add_dirs {
            push("--add-dir", dir);
        }
        if !self.allowed_tools.is_empty() {
            push("--allowedTools", &self.allowed_tools.join(","));
        }
        if let Some(mcp) = &self.mcp_config {
            push("--mcp-config", mcp);
        }
        args
    }

    /// 需要额外注入的环境变量。
    pub fn env(&self) -> Vec<(String, String)> {
        vec![
            ("CLAUDE_CODE_ENTRYPOINT".into(), "sdk-ts".into()),
            ("CLAUDE_AGENT_SDK_VERSION".into(), "0.2.0".into()),
            ("CLAUDE_CODE_EMIT_SESSION_STATE_EVENTS".into(), "1".into()),
            ("CLAUDE_CODE_SDK_READS_SESSION_STATE".into(), "1".into()),
        ]
    }
}

/// 查找 `claude` 可执行文件:优先用户配置的路径,其次环境变量与 `PATH`,最后是常见安装位置。
pub fn find_claude(custom: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = custom.filter(|p| !p.trim().is_empty()) {
        let p = expand_home(p.trim());
        if is_executable(&p) {
            return Some(p);
        }
    }
    if let Ok(p) = std::env::var("UCODE_CLAUDE_PATH") {
        let p = PathBuf::from(p);
        if is_executable(&p) {
            return Some(p);
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join("claude");
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
    }
    let home = std::env::var("HOME").unwrap_or_default();
    [
        format!("{home}/.local/bin/claude"),
        format!("{home}/.claude/local/claude"),
        "/opt/homebrew/bin/claude".into(),
        "/usr/local/bin/claude".into(),
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|p| is_executable(p))
}

fn expand_home(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(rest),
        None => PathBuf::from(p),
    }
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 新会话带_session_id_恢复带_resume() {
        let new = ClaudeOptions::from_config(&BackendConfig { cwd: "/tmp".into(), ..Default::default() });
        let args = new.args();
        assert!(args.contains(&"--session-id".to_string()));
        assert!(!args.contains(&"--resume".to_string()));

        let resumed = ClaudeOptions::from_config(&BackendConfig {
            cwd: "/tmp".into(),
            resume: Some("abc".into()),
            ..Default::default()
        });
        let args = resumed.args();
        assert!(args.windows(2).any(|w| w == ["--resume", "abc"]));
        assert!(!args.contains(&"--session-id".to_string()));
        assert_eq!(resumed.effective_session_id(), "abc");
    }

    #[test]
    fn 模型模式强度参数() {
        let o = ClaudeOptions {
            model: Some("opus".into()),
            mode: Some(PermissionMode::Plan),
            effort: Some(Effort::High),
            allowed_tools: vec!["Read".into(), "Bash(ls:*)".into()],
            ..Default::default()
        };
        let a = o.args();
        assert!(a.windows(2).any(|w| w == ["--model", "opus"]));
        assert!(a.windows(2).any(|w| w == ["--permission-mode", "plan"]));
        assert!(a.windows(2).any(|w| w == ["--effort", "high"]));
        assert!(a.windows(2).any(|w| w == ["--allowedTools", "Read,Bash(ls:*)"]));
        // default 别名不传 --model
        let d = ClaudeOptions { model: Some("default".into()), ..Default::default() };
        assert!(!d.args().contains(&"--model".to_string()));
    }

    #[test]
    fn 自定义路径不存在时回退() {
        // 不应 panic;结果取决于本机是否安装
        let _ = find_claude(Some("/definitely/not/here"));
    }
}
