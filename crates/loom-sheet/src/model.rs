//! 表格数据模型:单元格与工作表(纯值类型)。

/// 单元格类别(决定对齐)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CellKind {
    #[default]
    Text,
    Number,
    Boolean,
    Error,
}

/// 一个单元格。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Cell {
    /// 显示文本
    pub text: String,
    pub kind: CellKind,
}

impl Cell {
    /// 文本单元格。
    pub fn text(s: impl Into<String>) -> Self {
        Self { text: s.into(), kind: CellKind::Text }
    }
}

/// 一张工作表(行列矩阵,行只到最后一个非空单元格)。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sheet {
    pub name: String,
    pub rows: Vec<Vec<Cell>>,
    pub column_count: usize,
    /// 因行 / 列上限被截断
    pub truncated: bool,
}

impl Sheet {
    /// 由行构造,自动统计列数。
    pub fn new(name: impl Into<String>, rows: Vec<Vec<Cell>>, truncated: bool) -> Self {
        let column_count = rows.iter().map(Vec::len).max().unwrap_or(0);
        Self { name: name.into(), rows, column_count, truncated }
    }

    /// 取单元格文本(越界为空串)。
    pub fn text(&self, row: usize, col: usize) -> &str {
        self.rows.get(row).and_then(|r| r.get(col)).map_or("", |c| c.text.as_str())
    }

    /// 取单元格(越界为 `None`)。
    pub fn cell(&self, row: usize, col: usize) -> Option<&Cell> {
        self.rows.get(row).and_then(|r| r.get(col))
    }
}

/// 列索引 → 列字母(0 → A,26 → AA)。
pub fn column_letters(index: usize) -> String {
    let mut n = index + 1;
    let mut out = Vec::new();
    while n > 0 {
        let rem = (n - 1) % 26;
        out.push(b'A' + rem as u8);
        n = (n - 1) / 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// 解析单元格引用 `B12` → (列, 行),都从 0 起;格式不合法为 `None`。
pub fn parse_cell_ref(s: &str) -> Option<(usize, usize)> {
    let (mut col, mut row, mut digits) = (0usize, 0usize, 0usize);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' if digits == 0 => col = col * 26 + (b - b'A' + 1) as usize,
            b'a'..=b'z' if digits == 0 => col = col * 26 + (b - b'a' + 1) as usize,
            b'0'..=b'9' => {
                row = row * 10 + (b - b'0') as usize;
                digits += 1;
            }
            _ => return None,
        }
    }
    (col > 0 && digits > 0 && row > 0).then(|| (col - 1, row - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 列字母双向() {
        assert_eq!(column_letters(0), "A");
        assert_eq!(column_letters(25), "Z");
        assert_eq!(column_letters(26), "AA");
        assert_eq!(column_letters(701), "ZZ");
        assert_eq!(column_letters(702), "AAA");
        assert_eq!(parse_cell_ref("B12"), Some((1, 11)));
        assert_eq!(parse_cell_ref("aa1"), Some((26, 0)));
        assert_eq!(parse_cell_ref("12"), None);
        assert_eq!(parse_cell_ref("A0"), None);
    }

    #[test]
    fn 列数取最长行() {
        let s = Sheet::new("s", vec![vec![Cell::text("a")], vec![Cell::text("a"), Cell::text("b"), Cell::text("c")]], false);
        assert_eq!(s.column_count, 3);
        assert_eq!(s.text(0, 2), "");
        assert_eq!(s.text(1, 2), "c");
    }
}
