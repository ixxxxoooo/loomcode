//! LoomCode 配置:自身偏好(`~/.loomcode/settings.json`)与 Claude Code 配置的读写。
//!
//! 原则:不读取、不输出任何密钥;环境变量只展示名称,值一律遮盖。

pub mod claude;
pub mod diag;
pub mod items;
pub mod legacy;
pub mod plugins;
pub mod providers;
pub mod settings;
pub mod ui_state;

pub use claude::{ClaudeConfig, HookEntry, LayerSettings, McpServer, McpSpec, PermissionRules, Rule, Scope};
pub use settings::{AppSettings, AppearancePrefs, ScrollbarStyle, SpinnerStyle, AutoSave, BrowserPrefs, CursorShape, EditorPrefs, IntelPrefs, ProxyMode, ScmPrefs, TerminalPrefs, ThemeChoice};
pub use ui_state::{IdeMemory, IdeTabMemory, UiState};
