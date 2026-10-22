//! 惰性后端:界面侧立刻拿到通信句柄,但直到收到第一条命令才真正启动 `claude` 子进程。
//!
//! 浏览历史会话时只需要回放磁盘上的记录,没有必要为每个会话常驻一个子进程。

use crate::backend::ClaudeBackend;
use ucode_core::*;

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
            .name("ucode-claude-lazy".into())
            .spawn(move || {
                // 等第一条命令;界面丢弃会话(通道关闭)时直接退出,从未启动过子进程
                let Ok(first) = cmd_rx.recv_blocking() else { return };
                if matches!(first, AgentCommand::Shutdown) {
                    return;
                }
                let real = match inner.start(config) {
                    Ok(h) => h,
                    Err(e) => {
                        let _ = ev_tx.send_blocking(AgentEvent::Notice { level: NoticeLevel::Error, text: e.to_string() });
                        let _ = ev_tx.send_blocking(AgentEvent::TurnEnded(StopReason::Error));
                        return;
                    }
                };
                // 真实后端 → 界面:事件桥
                let events = real.events.clone();
                let tx = ev_tx.clone();
                let _ = std::thread::Builder::new().name("ucode-claude-lazy-events".into()).spawn(move || {
                    while let Ok(ev) = events.recv_blocking() {
                        if tx.send_blocking(ev).is_err() {
                            break;
                        }
                    }
                });
                // 界面 → 真实后端:先转第一条,再持续转发;界面侧关闭时通知后端退出
                real.send(first);
                while let Ok(cmd) = cmd_rx.recv_blocking() {
                    real.send(cmd);
                }
                real.send(AgentCommand::Shutdown);
            })
            .map_err(|e| BackendError::Spawn(e.to_string()))?;
        Ok(handle)
    }
}
