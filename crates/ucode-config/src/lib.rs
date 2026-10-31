//! UCode 配置:自身偏好(`~/.ucode/settings.json`)与 Claude Code 配置的读写。
//!
//! 原则:不读取、不输出任何密钥;环境变量只展示名称,值一律遮盖。

pub mod claude;
pub mod settings;
pub mod ui_state;

pub use claude::{ClaudeConfig, HookEntry, McpServer, PermissionRules};
pub use settings::{AppSettings, ThemeChoice};
pub use ui_state::UiState;
