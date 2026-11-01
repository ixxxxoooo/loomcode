//! Excel / OpenDocument 工作簿读取(xlsx / xlsm / xls / ods),基于 `calamine`。
//! 工作表按需读取:先列出工作表名,再只解析被选中的那一张。

use crate::model::{Cell, CellKind, Sheet};
use crate::{MAX_COLUMNS, MAX_ROWS};
use calamine::{Data, Reader, open_workbook_auto};
use std::path::Path;

/// 列出工作簿里的工作表名(按文件里的顺序)。
///
/// 参数:`path` 文件路径。返回:工作表名;文件打不开或已加密 / 损坏时返回中文错误说明。
pub fn sheet_names(path: &Path) -> Result<Vec<String>, String> {
    let wb = open_workbook_auto(path).map_err(|e| format!("无法打开工作簿:{e}"))?;
    Ok(wb.sheet_names())
}

/// 读取第 `index` 张工作表(行列坐标与 Excel 一致:已用区域前面的空行 / 空列也补上)。
///
/// 参数:`path` 文件路径;`index` 工作表序号(0 起)。返回:工作表;越界或读取失败时返回错误说明。
pub fn read_sheet(path: &Path, index: usize) -> Result<Sheet, String> {
    let mut wb = open_workbook_auto(path).map_err(|e| format!("无法打开工作簿:{e}"))?;
    let name = wb.sheet_names().get(index).cloned().ok_or_else(|| "工作表序号越界".to_string())?;
    let range = wb.worksheet_range(&name).map_err(|e| format!("读取工作表失败:{e}"))?;
    let (r0, c0) = range.start().map_or((0, 0), |(r, c)| (r as usize, c as usize));
    let mut truncated = false;
    let mut rows: Vec<Vec<Cell>> = Vec::new();
    for _ in 0..r0.min(MAX_ROWS) {
        rows.push(Vec::new());
    }
    for row in range.rows() {
        if rows.len() >= MAX_ROWS {
            truncated = true;
            break;
        }
        let mut cells: Vec<Cell> = vec![Cell::default(); c0.min(MAX_COLUMNS)];
        for d in row.iter().take(MAX_COLUMNS.saturating_sub(cells.len())) {
            cells.push(convert(d));
        }
        if c0 + row.len() > MAX_COLUMNS {
            truncated = true;
        }
        while cells.last().is_some_and(|c| c.text.is_empty()) {
            cells.pop();
        }
        rows.push(cells);
    }
    Ok(Sheet::new(name, rows, truncated))
}

/// 一个 calamine 值 → 单元格(整数值的浮点不带小数点,日期按「年-月-日」显示)。
fn convert(d: &Data) -> Cell {
    match d {
        Data::Empty => Cell::default(),
        Data::String(s) => Cell::text(s.clone()),
        Data::Int(i) => Cell { text: i.to_string(), kind: CellKind::Number },
        Data::Float(f) => Cell { text: float_text(*f), kind: CellKind::Number },
        Data::Bool(b) => Cell { text: if *b { "TRUE" } else { "FALSE" }.into(), kind: CellKind::Boolean },
        Data::DateTime(dt) => match dt.as_datetime() {
            Some(v) => {
                let full = v.to_string();
                Cell { text: if full.ends_with("00:00:00") { v.date().to_string() } else { full }, kind: CellKind::Text }
            }
            None => Cell { text: float_text(dt.as_f64()), kind: CellKind::Number },
        },
        Data::DateTimeIso(s) | Data::DurationIso(s) => Cell::text(s.clone()),
        Data::Error(e) => Cell { text: e.to_string(), kind: CellKind::Error },
    }
}

/// 浮点显示:整数值去掉小数点,其余取最短的精确表示。
fn float_text(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 { format!("{}", f as i64) } else { format!("{f}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 浮点显示() {
        assert_eq!(float_text(3.0), "3");
        assert_eq!(float_text(3.25), "3.25");
        assert_eq!(float_text(-0.5), "-0.5");
    }

    #[test]
    fn 转换各类值() {
        assert_eq!(convert(&Data::Int(7)).kind, CellKind::Number);
        assert_eq!(convert(&Data::Bool(true)).text, "TRUE");
        assert_eq!(convert(&Data::Empty).text, "");
        assert_eq!(convert(&Data::String("x".into())).kind, CellKind::Text);
    }

    #[test]
    fn 打不开的文件给出中文错误() {
        let e = sheet_names(Path::new("/nonexistent/none.xlsx")).unwrap_err();
        assert!(e.starts_with("无法打开工作簿"));
    }
}
