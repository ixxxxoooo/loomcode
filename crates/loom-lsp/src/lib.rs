//! 语言服务器(LSP)客户端内核:stdio JSON-RPC 传输、服务器注册表、按项目复用的服务器中心、URI 与 UTF-16 位置换算。
//! 与界面无关(不依赖 gpui),由 `loom-app` 把它接到编辑器的悬停 / 跳转 / 补全 / 诊断接口上。
//!
//! 设计取舍:
//! - 只用 `serde_json::Value`,不绑定 `lsp-types`(界面层按需反序列化),避免版本耦合。
//! - 请求返回 `async_channel::Receiver`,界面层在 gpui 的异步任务里 `await`,不占线程池。
//! - 服务器按「项目根 + 服务器 id」复用,同一个项目里打开多个文件只启动一个进程。
//!
//! @author ixxxxoooo

pub mod client;
pub mod framing;
pub mod hub;
pub mod position;
pub mod servers;
pub mod uri;

pub use client::Client;
pub use hub::{ServerState, ServerStatus};
pub use servers::{ServerSpec, spec_for_path};
