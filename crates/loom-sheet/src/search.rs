//! 表格内容搜索:在给定的可见行里按行优先顺序找出包含关键词的单元格(忽略大小写)。

use crate::model::Sheet;

/// 最多返回的匹配数(再多没有浏览意义,也避免巨表占内存)。
pub const MAX_MATCHES: usize = 20_000;

/// 一处匹配(原始行号、列号,都从 0 起)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Match {
    pub row: usize,
    pub col: usize,
}

/// 在 `rows`(显示顺序的原始行号)里查找包含 `query` 的单元格;关键词首尾空白忽略,为空返回空。
pub fn find(sheet: &Sheet, rows: &[usize], query: &str) -> Vec<Match> {
    let key = query.trim().to_lowercase();
    if key.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for &row in rows {
        let Some(cells) = sheet.rows.get(row) else { continue };
        for (col, cell) in cells.iter().enumerate() {
            if !cell.text.is_empty() && cell.text.to_lowercase().contains(&key) {
                out.push(Match { row, col });
                if out.len() >= MAX_MATCHES {
                    return out;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Cell;

    #[test]
    fn 按行优先顺序且忽略大小写() {
        let s = Sheet::new("s", vec![vec![Cell::text("Apple"), Cell::text("b")], vec![Cell::text("x"), Cell::text("APPLE pie")]], false);
        assert_eq!(find(&s, &[0, 1], " apple "), vec![Match { row: 0, col: 0 }, Match { row: 1, col: 1 }]);
        // 只搜可见行
        assert_eq!(find(&s, &[1], "apple").len(), 1);
        assert!(find(&s, &[0, 1], "  ").is_empty());
    }
}
