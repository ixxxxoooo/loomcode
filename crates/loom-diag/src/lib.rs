//! 诊断底座:文件日志、面包屑、慢操作计时、崩溃记录、运行标记、系统崩溃报告导入。
//! 位于依赖树最底层,`loom-claude`、`loom-term` 等都可以直接记日志,不依赖界面。
//!
//! @author ixxxxoooo

pub mod crash;
pub mod crumbs;
pub mod ips;
pub mod logger;
pub mod perf;
pub mod records;
pub mod sentinel;
pub mod stats;

pub use crash::RunInfo;
pub use crumbs::crumb;
pub use logger::{Dirs, Level, Logger, dirs, enabled, init_home, level, log, log_nonblocking, log_path, log_str, logger, redact_home, run_id, set_level, tail, timestamp};
pub use perf::{Span, span};
pub use records::CrashRecord;
