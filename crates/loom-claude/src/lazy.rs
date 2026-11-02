//! 惰性后端:界面侧立刻拿到通信句柄,但直到收到第一条命令才真正启动 `claude` 子进程。
//!
//! 浏览历史会话时只需要回放磁盘上的记录,没有必要为每个会话常驻一个子进程。
//!
//! 进程退出(崩溃、握手超时、被杀)后句柄依然有效:下一条提示会重新启动 `claude`,
//! 会话已落盘时用 `--resume` 接着聊;否则旧实现会把之后的命令悄悄丢掉,界面永远停在「处理中」。
//!
//! @author ixxxxoooo

use crate::backend::ClaudeBackend;
use loom_core::*;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// 包装 [`ClaudeBackend`],延迟到第一条命令才启动。
#[derive(Debug, Clone, Default)]
pub struct LazyBackend {
    pub inner: ClaudeBackend,
}

impl LazyBackend {
    pub fn new(inner: ClaudeBackend) -> Self {
        Self { inner }
    }
}

impl AgentBackend for LazyBackend {
    fn name(&self) -> &'static str {
        "claude(惰性)"
    }

    fn start(&self, config: BackendConfig) -> Result<BackendHandle, BackendError> {
        let (handle, cmd_rx, ev_tx) = channel_pair();
        let inner = self.inner.clone();
        std::thread::Builder::new()
            .name("loom-claude-lazy".into())
            .spawn(move || run(inner, config, cmd_rx, ev_tx))
            .map_err(|e| BackendError::Spawn(e.to_string()))?;
        Ok(handle)
    }
}

/// 把运行中切换的模型 / 模式 / 强度记进启动配置,重启后沿用。
///
/// 参数 `config`:下次启动用的配置;`cmd`:界面发来的命令。
fn remember(config: &mut BackendConfig, cmd: &AgentCommand) {
    match cmd {
        AgentCommand::SetModel(m) => config.model = Some(m.clone()),
        AgentCommand::SetMode(m) => config.mode = Some(*m),
        AgentCommand::SetEffort(e) => config.effort = Some(*e),
        AgentCommand::SetEnv(env) => config.extra_env = env.clone(),
        _ => {}
    }
}

/// 进程退出后,下次启动用的配置。
///
/// 参数 `config`:上次启动的配置;`session_id`:上次会话的有效 id;`persisted`:该会话是否已写入磁盘。
/// 返回:已落盘就恢复它(分叉已经发生过,不再分叉);没落盘就沿用原配置重新开始。
pub fn restart_config(config: &BackendConfig, session_id: Option<&str>, persisted: bool) -> BackendConfig {
    let mut next = config.clone();
    if let (Some(id), true) = (session_id.filter(|s| !s.is_empty()), persisted) {
        next.resume = Some(id.to_string());
        next.session_id = None;
        next.fork_session = false;
    }
    next
}

/// 会话记录是否已写入 `~/.claude/projects/<编码后的 cwd>/<id>.jsonl`。
///
/// claude 可能按解开符号链接后的路径记录(`/var` → `/private/var`),两种写法都查。
fn session_on_disk(projects: &Path, cwd: &str, id: &str) -> bool {
    let file = format!("{id}.jsonl");
    let real = std::fs::canonicalize(cwd).ok().map(|p| p.to_string_lossy().to_string());
    std::iter::once(cwd.to_string()).chain(real).any(|c| projects.join(crate::history::encode_project_dir(&c)).join(&file).is_file())
}

/// 惰性后端的主循环:等命令 → 启动 → 转发;进程退出后等下一条提示再启动。
fn run(inner: ClaudeBackend, mut config: BackendConfig, cmd_rx: async_channel::Receiver<AgentCommand>, ev_tx: async_channel::Sender<AgentEvent>) {
    // 等第一条命令;界面丢弃会话(通道关闭)时直接退出,从未启动过子进程
    let Ok(mut first) = cmd_rx.recv_blocking() else { return };
    let mut started_once = false;
    loop {
        // 换环境变量只记下,不为它启动进程
        while matches!(first, AgentCommand::SetEnv(_)) {
            remember(&mut config, &first);
            match cmd_rx.recv_blocking() {
                Ok(cmd) => first = cmd,
                Err(_) => return,
            }
        }
        if matches!(first, AgentCommand::Shutdown) {
            return;
        }
        remember(&mut config, &first);
        let real = match inner.start(config.clone()) {
            Ok(h) => h,
            Err(e) => {
                let _ = ev_tx.send_blocking(AgentEvent::Notice { level: NoticeLevel::Error, text: e.to_string() });
                let _ = ev_tx.send_blocking(AgentEvent::TurnEnded(StopReason::Error));
                // 启动失败不退出:用户装好 claude 或修好配置后再发一次即可
                match cmd_rx.recv_blocking() {
                    Ok(cmd) => {
                        first = cmd;
                        continue;
                    }
                    Err(_) => return,
                }
            }
        };
        if started_once {
            loom_diag::info!("claude 进程已重新启动 resume={:?}", config.resume);
            let _ = ev_tx.send_blocking(AgentEvent::Notice { level: NoticeLevel::Info, text: "claude 进程已重新启动,继续当前会话".into() });
        }
        started_once = true;

        // 真实后端 → 界面:事件桥;顺带记下会话 id,事件通道关闭即进程已退出
        let alive = Arc::new(AtomicBool::new(true));
        let session_id: Arc<Mutex<Option<String>>> = Arc::default();
        {
            let (events, tx, alive, session_id) = (real.events.clone(), ev_tx.clone(), alive.clone(), session_id.clone());
            let _ = std::thread::Builder::new().name("loom-claude-lazy-events".into()).spawn(move || {
                while let Ok(ev) = events.recv_blocking() {
                    if let AgentEvent::Ready { session_id: id, .. } = &ev
                        && let Ok(mut s) = session_id.lock()
                    {
                        *s = Some(id.clone());
                    }
                    if tx.send_blocking(ev).is_err() {
                        break;
                    }
                }
                alive.store(false, Ordering::SeqCst);
            });
        }

        // 界面 → 真实后端:先转第一条,再持续转发;界面侧关闭时通知后端退出
        real.send(first);
        let next = loop {
            let Ok(cmd) = cmd_rx.recv_blocking() else {
                real.send(AgentCommand::Shutdown);
                return;
            };
            remember(&mut config, &cmd);
            if alive.load(Ordering::SeqCst) {
                // 环境变量只能在启动时注入:结束当前进程,下一条提示用新环境恢复会话
                let cmd = if matches!(cmd, AgentCommand::SetEnv(_)) { AgentCommand::Shutdown } else { cmd };
                real.send(cmd);
                continue;
            }
            // 进程已退出:提示、回退文件与关闭会重新启动,其余命令(查用量、切模型等)只记下不启动
            match cmd {
                AgentCommand::Prompt { .. } | AgentCommand::RewindFiles { .. } | AgentCommand::Shutdown => break cmd,
                _ => continue,
            }
        };
        let id = session_id.lock().ok().and_then(|s| s.clone());
        let persisted = id.as_deref().is_some_and(|id| session_on_disk(&crate::history::projects_dir(), &config.cwd, id));
        config = restart_config(&config, id.as_deref(), persisted);
        first = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 已落盘的会话重启时恢复() {
        let cfg = BackendConfig { cwd: "/w".into(), session_id: Some("new".into()), ..Default::default() };
        let next = restart_config(&cfg, Some("new"), true);
        assert_eq!(next.resume.as_deref(), Some("new"));
        assert!(next.session_id.is_none());
        // 分叉会话:分叉已经发生,重启时直接恢复分叉出来的新会话
        let fork = BackendConfig { resume: Some("src".into()), session_id: Some("forked".into()), fork_session: true, ..Default::default() };
        let next = restart_config(&fork, Some("forked"), true);
        assert_eq!((next.resume.as_deref(), next.fork_session, next.session_id.as_deref()), (Some("forked"), false, None));
    }

    #[test]
    fn 未落盘的会话沿用原配置() {
        let cfg = BackendConfig { cwd: "/w".into(), session_id: Some("new".into()), ..Default::default() };
        let next = restart_config(&cfg, Some("new"), false);
        assert!(next.resume.is_none());
        assert_eq!(next.session_id.as_deref(), Some("new"));
        assert!(restart_config(&cfg, None, true).resume.is_none());
    }

    #[test]
    fn 运行中的切换在重启后沿用() {
        let mut cfg = BackendConfig::default();
        remember(&mut cfg, &AgentCommand::SetModel("opus".into()));
        remember(&mut cfg, &AgentCommand::SetMode(PermissionMode::Plan));
        remember(&mut cfg, &AgentCommand::SetEffort(Effort::High));
        assert_eq!(cfg.model.as_deref(), Some("opus"));
        assert_eq!(cfg.mode, Some(PermissionMode::Plan));
        assert_eq!(cfg.effort, Some(Effort::High));
        remember(&mut cfg, &AgentCommand::SetEnv(vec![("ANTHROPIC_MODEL".into(), "deepseek-chat".into())]));
        assert_eq!(cfg.extra_env, vec![("ANTHROPIC_MODEL".to_string(), "deepseek-chat".to_string())]);
    }

    #[test]
    fn 会话文件检测() {
        let dir = std::env::temp_dir().join(format!("loom-lazy-{}", std::process::id()));
        let proj = dir.join(crate::history::encode_project_dir("/w/p"));
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(proj.join("abc.jsonl"), "{}\n").unwrap();
        assert!(session_on_disk(&dir, "/w/p", "abc"));
        assert!(!session_on_disk(&dir, "/w/p", "zzz"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
