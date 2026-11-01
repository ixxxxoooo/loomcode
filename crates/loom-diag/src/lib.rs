//! 诊断底座:文件日志、面包屑、慢操作计时(后续还有崩溃记录、运行标记、性能采样)。
//! 位于依赖树最底层,`loom-claude`、`loom-term` 等都可以直接记日志,不依赖界面。
//!
//! @author ixxxxoooo

pub mod crumbs;
pub mod logger;
pub mod perf;

pub use crumbs::crumb;
pub use logger::{Dirs, Level, Logger, dirs, enabled, init_home, level, log, log_nonblocking, log_path, log_str, logger, redact_home, run_id, set_level, tail, timestamp};
pub use perf::{Span, span};
