//! LoomCode 的真实后端:直连本机 `claude` 二进制的 stream-json 控制协议(不经过 ACP,也不需要 Node)。
//!
//! 模块划分:
//! - `options`:启动参数、环境变量与 `claude` 可执行文件查找;
//! - `translate`:有状态的消息翻译器,把 claude 输出的每一行翻译成 `AgentEvent`;
//! - `protocol`:发往 claude 标准输入的消息构造(用户消息、控制请求、权限应答);
//! - `backend`:单 Actor 线程驱动子进程,实现 `AgentBackend`;
//! - `lazy`:惰性包装,收到第一条命令才真正启动子进程(浏览历史会话不占进程);
//! - `history`:读取 `~/.claude/projects` 下的历史会话并回放。

pub mod backend;
pub mod history;
pub mod lazy;
pub mod options;
pub mod usage;
pub mod protocol;
pub mod translate;

pub use backend::ClaudeBackend;
pub use lazy::LazyBackend;
pub use options::{ClaudeOptions, find_claude};
