//! 项目内搜索 / 替换内核(与界面无关,对标 VS Code 的「搜索」):
//! 区分大小写 / 全词 / 正则三个开关、包含与排除文件(逗号分隔的 glob)、遵守 `.gitignore`,
//! 并行遍历、可随时取消、结果上限 2 万处;替换支持 `$1` 捕获组与保留大小写,原子写盘。
//!
//! 限制:按行匹配(正则不跨行);非 UTF-8 文件与二进制文件跳过;超过 2 MiB 的文件跳过。
//!
//! @author ixxxxoooo

mod engine;
mod preview;
mod query;
mod replace;

pub use engine::{FileMatches, Match, SearchOutcome, MAX_FILE_BYTES, MAX_RESULTS, search};
pub use preview::Preview;
pub use query::SearchQuery;
pub use replace::{MatchKey, apply_case, replace_in_file};
