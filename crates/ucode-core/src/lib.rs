//! UCode 领域模型:与界面、与具体后端(真实 Claude / 演示)无关的纯数据与状态机。
//!
//! 设计要点:
//! - `AgentBackend` 是唯一的后端抽象,真实的 `claude` 与演示脚本都实现它;
//! - 后端只产出 `AgentEvent`,界面只发送 `AgentCommand`;
//! - `SessionState::apply` 是纯函数式的状态归约,可以脱离界面做单元测试。

pub mod backend;
pub mod event;
pub mod modes;
pub mod session;
pub mod timeline;
pub mod tool;
pub mod workspace;

pub use backend::*;
pub use event::*;
pub use modes::*;
pub use session::*;
pub use timeline::*;
pub use tool::*;
pub use workspace::*;
