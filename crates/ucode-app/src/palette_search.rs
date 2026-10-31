//! 命令面板的搜索:子序列模糊匹配、按分段筛选排序、收集项目文件。纯函数,不依赖界面。
//!
//! @author ixxxxoooo

use crate::palette::{Group, PaletteItem};
use std::path::Path;

/// 面板顶部的分段筛选。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Segment {
    All,
    Session,
    File,
    Command,
}

impl Segment {
    pub const ALL: [Segment; 4] = [Segment::All, Segment::Session, Segment::File, Segment::Command];

    /// 分段标签。
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "全部",
            Self::Session => "会话",
            Self::File => "文件",
            Self::Command => "命令",
        }
    }

    /// 该分段是否包含某个分组的条目(「全部」包含一切)。
    fn accepts(self, group: Group) -> bool {
        match self {
            Self::All => true,
            Self::Session => group == Group::Session,
            Self::File => group == Group::File,
            Self::Command => group == Group::Command,
        }
    }
}

/// 子序列模糊匹配:返回得分(越高越好),不匹配返回 `None`。
/// 用动态规划在所有对齐方式里取最优:连续命中、词首命中与更靠前的命中得分更高。
pub fn fuzzy_score(query: &str, text: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let q: Vec<char> = query.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    if q.is_empty() {
        return Some(0);
    }
    if q.len() > t.len() {
        return None;
    }
    const NONE: i32 = i32::MIN / 2;
    // 单个字符命中的基础分
    let base = |j: usize| -> i32 {
        let mut s = 10;
        if j == 0 || matches!(t[j - 1], '/' | '-' | '_' | ' ' | '.') {
            s += 6;
        }
        s - (j as i32).min(20) / 4
    };
    // prev[j]:前 i 个查询字符匹配完、且第 i 个落在 t[j] 的最佳得分
    let mut prev: Vec<i32> = (0..t.len()).map(|j| if t[j] == q[0] { base(j) } else { NONE }).collect();
    for &qc in &q[1..] {
        let mut cur = vec![NONE; t.len()];
        // best_before[j] = max(prev[k]) for k < j
        let mut best_before = NONE;
        for j in 0..t.len() {
            if j > 0 {
                best_before = best_before.max(prev[j - 1]);
            }
            if t[j] == qc {
                let mut cand = best_before;
                if j > 0 && prev[j - 1] > NONE {
                    cand = cand.max(prev[j - 1] + 8); // 紧挨上一个命中
                }
                if cand > NONE {
                    cur[j] = cand + base(j);
                }
            }
        }
        prev = cur;
    }
    let best = prev.into_iter().max().filter(|&s| s > NONE)?;
    Some(best - (t.len() as i32 / 8))
}

/// 在条目中按分段与查询筛选并排序,最多返回 `limit` 项。
///
/// 参数:`segment` 分段;`query` 查询词。「全部」且无查询时只列命令与会话,避免淹没在文件里;
/// 其他分段无查询时列出该分段的全部条目。
pub fn filter_items(items: &[PaletteItem], query: &str, limit: usize, segment: Segment) -> Vec<PaletteItem> {
    if query.trim().is_empty() {
        let keep = |i: &&PaletteItem| match segment {
            Segment::All => matches!(i.group, Group::Command | Group::Session),
            other => other.accepts(i.group),
        };
        return items.iter().filter(keep).take(limit).cloned().collect();
    }
    let mut scored: Vec<(i32, usize, &PaletteItem)> = items
        .iter()
        .enumerate()
        .filter(|(_, i)| segment.accepts(i.group))
        .filter_map(|(ix, i)| {
            let a = fuzzy_score(query, &i.title);
            let b = fuzzy_score(query, &i.subtitle).map(|s| s - 12);
            a.into_iter().chain(b).max().map(|s| (s, ix, i))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().take(limit).map(|(_, _, i)| i.clone()).collect()
}

/// 递归收集项目文件(相对路径),跳过隐藏目录与构建产物。
pub fn collect_files(root: &Path, max: usize) -> Vec<String> {
    fn go(root: &Path, dir: &Path, depth: usize, out: &mut Vec<String>, max: usize) {
        if depth > 5 || out.len() >= max {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') && name != ".gitignore" || matches!(name.as_str(), "target" | "node_modules" | "DerivedData" | ".build") {
                continue;
            }
            let path = e.path();
            if path.is_dir() {
                go(root, &path, depth + 1, out, max);
            } else if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_string_lossy().to_string());
                if out.len() >= max {
                    return;
                }
            }
        }
    }
    let mut out = Vec::new();
    go(root, root, 0, &mut out, max);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::PaletteAction;
    use gpui_kit::assets::IconName;

    fn item(title: &str, group: Group) -> PaletteItem {
        PaletteItem { title: title.into(), subtitle: String::new(), group, icon: IconName::Search, action: PaletteAction::NewSession }
    }

    #[::core::prelude::v1::test]
    fn 子序列匹配() {
        assert!(fuzzy_score("rtr", "src/router.rs").is_some());
        assert!(fuzzy_score("xyz", "src/router.rs").is_none());
        assert_eq!(fuzzy_score("", "任何"), Some(0));
    }

    #[::core::prelude::v1::test]
    fn 连续与词首得分更高() {
        let a = fuzzy_score("router", "src/router.rs").unwrap();
        let b = fuzzy_score("router", "r_o_u_t_e_r_extra").unwrap();
        assert!(a > b);
    }

    #[::core::prelude::v1::test]
    fn 无查询只列命令与会话() {
        let items = vec![item("新会话", Group::Command), item("src/a.rs", Group::File), item("某会话", Group::Session)];
        let shown = filter_items(&items, "", 10, Segment::All);
        assert_eq!(shown.len(), 2);
    }

    #[::core::prelude::v1::test]
    fn 筛选并按得分排序() {
        let items = vec![item("设置", Group::Command), item("新会话", Group::Command), item("会话列表", Group::Command)];
        let shown = filter_items(&items, "会话", 10, Segment::All);
        assert_eq!(shown.len(), 2);
        // 前缀命中(词首)优先
        assert_eq!(shown[0].title, "会话列表");
    }

    #[::core::prelude::v1::test]
    fn 分段只显示对应分组() {
        let items = vec![item("新会话", Group::Command), item("src/a.rs", Group::File), item("某会话", Group::Session)];
        // 文件分段无查询时也能列出文件
        let files = filter_items(&items, "", 10, Segment::File);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].title, "src/a.rs");
        // 有查询时同样只在该分段内搜索
        assert!(filter_items(&items, "会话", 10, Segment::File).is_empty());
        assert_eq!(filter_items(&items, "会话", 10, Segment::Session).len(), 1);
    }
}
