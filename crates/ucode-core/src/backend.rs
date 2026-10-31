//! 后端抽象。

use crate::event::{AgentCommand, AgentEvent};
use async_channel::{Receiver, Sender};

/// 一个已启动后端的通信句柄:界面往 `commands` 发命令,从 `events` 收事件。
#[derive(Clone)]
pub struct BackendHandle {
    pub commands: Sender<AgentCommand>,
    pub events: Receiver<AgentEvent>,
}

impl BackendHandle {
    /// 发送命令;后端已退出时静默忽略(界面不需要处理这类竞态)。
    pub fn send(&self, cmd: AgentCommand) {
        let _ = self.commands.try_send(cmd);
    }
}

/// 后端启动参数。
#[derive(Debug, Clone, Default)]
pub struct BackendConfig {
    /// 工作目录
    pub cwd: String,
    /// 要恢复的会话 id
    pub resume: Option<String>,
    /// 新会话预分配的 id(与 `resume` 互斥;为空则由后端自行生成)
    pub session_id: Option<String>,
    pub model: Option<String>,
    pub mode: Option<crate::modes::PermissionMode>,
    pub effort: Option<crate::modes::Effort>,
    /// 额外授权访问的目录(多根工作区的其余文件夹,对应 `--add-dir`)
    pub add_dirs: Vec<String>,
    /// 追加到系统提示末尾的内容(`--append-system-prompt`)
    pub append_system_prompt: Option<String>,
}

/// 后端工厂:真实 Claude 与演示后端各自实现。
pub trait AgentBackend: Send + Sync {
    /// 后端名称(日志与诊断页使用)
    fn name(&self) -> &'static str;
    /// 启动一个会话,返回通信句柄。
    fn start(&self, config: BackendConfig) -> Result<BackendHandle, BackendError>;
}

/// 后端错误。
#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("找不到 claude 可执行文件,请先安装 Claude Code")]
    ClaudeNotFound,
    #[error("启动子进程失败:{0}")]
    Spawn(String),
    #[error("协议错误:{0}")]
    Protocol(String),
}

/// 创建一对成对的通道,供后端实现使用。返回 (界面侧句柄, 后端侧的命令接收端与事件发送端)。
pub fn channel_pair() -> (BackendHandle, Receiver<AgentCommand>, Sender<AgentEvent>) {
    let (cmd_tx, cmd_rx) = async_channel::unbounded();
    let (ev_tx, ev_rx) = async_channel::unbounded();
    (BackendHandle { commands: cmd_tx, events: ev_rx }, cmd_rx, ev_tx)
}
