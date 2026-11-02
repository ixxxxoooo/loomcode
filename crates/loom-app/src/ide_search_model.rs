//! 搜索结果的纯模型:把「文件 → 匹配」两级结果拍平成虚拟列表的行,以及忽略(从结果里移除)与汇总。
//!
//! @author ixxxxoooo

use loom_search::FileMatches;
use std::collections::HashSet;
use std::path::PathBuf;

/// 虚拟列表里的一行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// 文件行(文件下标)
    File(usize),
    /// 匹配行(文件下标, 匹配下标)
    Match(usize, usize),
}

/// 拍平:折叠的文件只留文件行。
///
/// 参数:`files` 结果;`collapsed` 已折叠的文件路径。返回:按显示顺序排列的行。
pub fn flatten(files: &[FileMatches], collapsed: &HashSet<PathBuf>) -> Vec<Row> {
    let mut rows = Vec::with_capacity(files.len() + files.iter().map(|f| f.matches.len()).sum::<usize>());
    for (fi, f) in files.iter().enumerate() {
        rows.push(Row::File(fi));
        if !collapsed.contains(&f.path) {
            rows.extend((0..f.matches.len()).map(|mi| Row::Match(fi, mi)));
        }
    }
    rows
}

/// 汇总:(文件数, 匹配数)。
pub fn totals(files: &[FileMatches]) -> (usize, usize) {
    (files.len(), files.iter().map(|f| f.matches.len()).sum())
}

/// 忽略一处匹配;文件里没有匹配了就把文件也移除。
///
/// 参数:`files` 结果;`fi` 文件下标;`mi` 匹配下标。越界时什么都不做。
pub fn dismiss_match(files: &mut Vec<FileMatches>, fi: usize, mi: usize) {
    let Some(f) = files.get_mut(fi) else { return };
    if mi < f.matches.len() {
        f.matches.remove(mi);
    }
    if f.matches.is_empty() {
        files.remove(fi);
    }
}

/// 忽略一个文件的全部匹配。越界时什么都不做。
pub fn dismiss_file(files: &mut Vec<FileMatches>, fi: usize) {
    if fi < files.len() {
        files.remove(fi);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_search::{Match, Preview};

    fn file(name: &str, n: usize) -> FileMatches {
        let m = |i| Match { line: i as u32 + 1, start: 0, end: 1, preview: Preview { text: "x".into(), hl: 0..1 } };
        FileMatches { path: PathBuf::from(name), matches: (0..n).map(m).collect() }
    }

    #[::core::prelude::v1::test]
    fn 拍平时折叠的文件只留文件行() {
        let files = vec![file("a", 2), file("b", 1)];
        assert_eq!(flatten(&files, &HashSet::new()), vec![Row::File(0), Row::Match(0, 0), Row::Match(0, 1), Row::File(1), Row::Match(1, 0)]);
        let collapsed: HashSet<PathBuf> = [PathBuf::from("a")].into_iter().collect();
        assert_eq!(flatten(&files, &collapsed), vec![Row::File(0), Row::File(1), Row::Match(1, 0)]);
        assert_eq!(totals(&files), (2, 3));
    }

    #[::core::prelude::v1::test]
    fn 忽略最后一处匹配时文件一并移除() {
        let mut files = vec![file("a", 2), file("b", 1)];
        dismiss_match(&mut files, 0, 0);
        assert_eq!(files[0].matches.len(), 1);
        dismiss_match(&mut files, 1, 0);
        assert_eq!(files.len(), 1);
        dismiss_match(&mut files, 9, 9);
        dismiss_file(&mut files, 0);
        assert!(files.is_empty());
    }
}
