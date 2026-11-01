//! 表格文件的读取与浏览逻辑(与界面无关):CSV / TSV 解析、xlsx / xls / ods 读取、
//! 列筛选与排序、内容搜索。对标 OhMyAgent 的 `SpreadsheetKit`。
//!
//! @author ixxxxoooo

pub mod csv;
pub mod filter;
pub mod model;
pub mod search;
pub mod xlsx;

pub use filter::{ColumnValue, FilterState, Sort};
pub use model::{Cell, CellKind, Sheet, column_letters, parse_cell_ref};
pub use search::{Match, find};

use std::path::Path;

/// 行数上限(超出截断并标记 `truncated`)。
pub const MAX_ROWS: usize = 100_000;
/// 列数上限。
pub const MAX_COLUMNS: usize = 512;

/// 文件类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// 逗号 / 制表符等分隔的文本
    Delimited,
    /// Excel / OpenDocument 工作簿
    Workbook,
}

/// 按扩展名判断是不是表格文件;不是返回 `None`。
pub fn kind_of(path: &Path) -> Option<Kind> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "csv" | "tsv" => Some(Kind::Delimited),
        "xlsx" | "xlsm" | "xls" | "ods" => Some(Kind::Workbook),
        _ => None,
    }
}
