//! 列筛选与排序(类似 Excel 的「自动筛选」):每列按显示文本勾选保留的值,候选值受其他列筛选影响(级联);
//! 首行可作表头,不参与筛选与排序。

use crate::model::Sheet;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};

/// 排序方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    pub column: usize,
    pub ascending: bool,
}

/// 一张表的筛选 / 排序状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterState {
    /// 列 → 允许显示的值(没有该键 = 该列不筛选)
    pub filters: HashMap<usize, HashSet<String>>,
    pub sort: Option<Sort>,
    /// 首行当表头:始终在最前,不筛选、不排序
    pub first_row_header: bool,
}

impl Default for FilterState {
    fn default() -> Self {
        Self { filters: HashMap::new(), sort: None, first_row_header: true }
    }
}

/// 候选值及其出现次数(空串表示「空白」)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnValue {
    pub text: String,
    pub count: usize,
}

impl FilterState {
    /// 是否有生效的筛选或排序。
    pub fn is_active(&self) -> bool {
        !self.filters.is_empty() || self.sort.is_some()
    }

    /// 某列是否在筛选。
    pub fn is_filtered(&self, column: usize) -> bool {
        self.filters.contains_key(&column)
    }

    /// 清除全部筛选(保留排序与表头设置)。
    pub fn clear_filters(&mut self) {
        self.filters.clear();
    }

    /// 切换某列的一个值(从「全部显示」点掉一个 = 取消勾选它);全部勾选等价于不筛选。
    pub fn toggle(&mut self, column: usize, value: &str, available: &[ColumnValue]) {
        let mut allowed: HashSet<String> = self.filters.get(&column).cloned().unwrap_or_else(|| available.iter().map(|v| v.text.clone()).collect());
        if !allowed.remove(value) {
            allowed.insert(value.to_string());
        }
        self.set_allowed(column, allowed, available);
    }

    /// 设置某列允许的值;包含全部候选值时视为不筛选。
    pub fn set_allowed(&mut self, column: usize, allowed: HashSet<String>, available: &[ColumnValue]) {
        if available.iter().all(|v| allowed.contains(&v.text)) {
            self.filters.remove(&column);
        } else {
            self.filters.insert(column, allowed);
        }
    }

    fn passes(&self, sheet: &Sheet, row: usize, skip: Option<usize>) -> bool {
        self.filters.iter().all(|(c, allowed)| Some(*c) == skip || allowed.contains(sheet.text(row, *c)))
    }

    /// 应用筛选与排序,得到要显示的原始行号(表头行在最前)。
    pub fn visible_rows(&self, sheet: &Sheet) -> Vec<usize> {
        let total = sheet.rows.len();
        let head = usize::from(self.first_row_header && total > 0);
        let mut body: Vec<usize> = (head..total).filter(|r| self.passes(sheet, *r, None)).collect();
        if let Some(s) = self.sort {
            // 稳定排序:相等保持原行序;空白始终排最后(与 Excel 一致)
            body.sort_by(|a, b| {
                let (x, y) = (sheet.text(*a, s.column), sheet.text(*b, s.column));
                match (x.is_empty(), y.is_empty()) {
                    (true, true) => Ordering::Equal,
                    (true, false) => Ordering::Greater,
                    (false, true) => Ordering::Less,
                    _ => {
                        let o = compare(x, y);
                        if s.ascending { o } else { o.reverse() }
                    }
                }
            });
        }
        (0..head).chain(body).collect()
    }

    /// 某列的候选值(只统计通过「其他列」筛选的行),按自然顺序,空白排最后。
    pub fn distinct_values(&self, sheet: &Sheet, column: usize) -> Vec<ColumnValue> {
        let start = usize::from(self.first_row_header);
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for row in start..sheet.rows.len() {
            if self.passes(sheet, row, Some(column)) {
                *counts.entry(sheet.text(row, column)).or_default() += 1;
            }
        }
        let mut list: Vec<ColumnValue> = counts.into_iter().map(|(t, c)| ColumnValue { text: t.to_string(), count: c }).collect();
        list.sort_by(|a, b| match (a.text.is_empty(), b.text.is_empty()) {
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            _ => compare(&a.text, &b.text),
        });
        list
    }
}

/// 比较两个显示文本:都是数字按数值,否则忽略大小写按字典序。
pub fn compare(a: &str, b: &str) -> Ordering {
    match (number(a), number(b)) {
        (Some(x), Some(y)) if x != y => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        _ => a.to_lowercase().cmp(&b.to_lowercase()),
    }
}

/// 显示文本里的数字(忽略千分位、货币符号、百分号)。
fn number(t: &str) -> Option<f64> {
    let c: String = t.chars().filter(|c| !",%¥$€£ ".contains(*c)).collect();
    if c.is_empty() { None } else { c.parse().ok() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Cell;

    fn sheet() -> Sheet {
        let row = |a: &str, b: &str| vec![Cell::text(a), Cell::text(b)];
        Sheet::new("s", vec![row("名称", "数量"), row("苹果", "10"), row("梨", "9"), row("苹果", "2"), row("桃", "")], false)
    }

    #[test]
    fn 表头固定_按数值排序_空白最后() {
        let s = sheet();
        let st = FilterState { sort: Some(Sort { column: 1, ascending: true }), ..Default::default() };
        assert_eq!(st.visible_rows(&s), vec![0, 3, 2, 1, 4]);
        let st = FilterState { sort: Some(Sort { column: 1, ascending: false }), ..Default::default() };
        assert_eq!(st.visible_rows(&s), vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn 筛选与候选值级联() {
        let s = sheet();
        let mut st = FilterState::default();
        let avail = st.distinct_values(&s, 0);
        assert_eq!(avail.iter().find(|v| v.text == "苹果").unwrap().count, 2);
        st.toggle(0, "苹果", &avail);
        assert_eq!(st.visible_rows(&s), vec![0, 2, 4]);
        // 数量列的候选值只统计通过名称筛选的行
        let q: Vec<_> = st.distinct_values(&s, 1).into_iter().map(|v| v.text).collect();
        assert_eq!(q, vec!["9", ""]);
        // 再勾回来 = 不筛选
        st.toggle(0, "苹果", &avail);
        assert!(!st.is_filtered(0));
    }

    #[test]
    fn 不当表头时首行参与() {
        let s = sheet();
        let st = FilterState { first_row_header: false, sort: Some(Sort { column: 0, ascending: true }), ..Default::default() };
        assert_eq!(st.visible_rows(&s).len(), 5);
    }

    #[test]
    fn 数字与文字比较() {
        assert_eq!(compare("2", "10"), Ordering::Less);
        assert_eq!(compare("1,000", "999"), Ordering::Greater);
        assert_eq!(compare("a", "B"), Ordering::Less);
    }
}
