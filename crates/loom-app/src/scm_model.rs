//! 源代码管理视图的纯逻辑(与界面无关):把 `git status` 拆成「合并更改 / 暂存的更改 / 更改」三组,
//! 并把一组文件整理成列表或目录树的显示行。对标 VS Code 的 Source Control 视图。
//!
//! @author ixxxxoooo

use loom_core::git::StatusEntry;
use std::collections::{BTreeMap, HashSet};

/// 分组(顺序即显示顺序)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Group {
    /// 合并冲突
    Conflicts,
    /// 已暂存,下一次提交会包含
    Staged,
    /// 工作区里还没暂存的改动(含未跟踪)
    Changes,
}

impl Group {
    /// 组标题。
    pub fn title(self) -> &'static str {
        match self {
            Group::Conflicts => "合并更改",
            Group::Staged => "暂存的更改",
            Group::Changes => "更改",
        }
    }

    /// 元素 id 里用的短名。
    pub fn key(self) -> &'static str {
        match self {
            Group::Conflicts => "mg",
            Group::Staged => "sg",
            Group::Changes => "cg",
        }
    }
}

/// 一组里的一个文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub group: Group,
    pub entry: StatusEntry,
    /// 这一组里显示的状态字母:`M` `A` `D` `R` `C` `U`(未跟踪)`!`(冲突)
    pub kind: char,
}

impl Item {
    /// 路径。
    pub fn path(&self) -> &str {
        &self.entry.path
    }

    /// 在界面里唯一标识这一行(同一文件可同时出现在两组)。
    pub fn key(&self) -> String {
        format!("{}:{}", self.group.key(), self.entry.path)
    }
}

/// 三组文件。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Groups {
    pub conflicts: Vec<Item>,
    pub staged: Vec<Item>,
    pub changes: Vec<Item>,
}

impl Groups {
    /// 按组取列表。
    pub fn of(&self, g: Group) -> &[Item] {
        match g {
            Group::Conflicts => &self.conflicts,
            Group::Staged => &self.staged,
            Group::Changes => &self.changes,
        }
    }

    /// 有改动的不同文件数(活动栏角标用)。
    pub fn distinct(&self) -> usize {
        let mut set: HashSet<&str> = HashSet::new();
        for i in self.conflicts.iter().chain(&self.staged).chain(&self.changes) {
            set.insert(i.path());
        }
        set.len()
    }
}

/// 把 `git status` 的条目拆成三组:冲突单列,已暂存与未暂存的改动分开列(同一文件两边都有改动会出现两次)。
pub fn split(entries: &[StatusEntry]) -> Groups {
    let mut g = Groups::default();
    for e in entries {
        if e.conflict() {
            g.conflicts.push(Item { group: Group::Conflicts, entry: e.clone(), kind: '!' });
            continue;
        }
        if let Some(k) = e.staged_kind() {
            g.staged.push(Item { group: Group::Staged, entry: e.clone(), kind: k });
        }
        if let Some(k) = e.unstaged_kind() {
            g.changes.push(Item { group: Group::Changes, entry: e.clone(), kind: k });
        }
    }
    let by_path = |a: &Item, b: &Item| a.entry.path.to_lowercase().cmp(&b.entry.path.to_lowercase());
    g.conflicts.sort_by(by_path);
    g.staged.sort_by(by_path);
    g.changes.sort_by(by_path);
    g
}

/// 状态字母的中文说明(悬停提示)。
pub fn kind_title(kind: char) -> &'static str {
    match kind {
        'M' => "已修改",
        'A' => "已添加",
        'D' => "已删除",
        'R' => "已重命名",
        'C' => "已复制",
        'U' => "未跟踪",
        '!' => "冲突",
        _ => "已更改",
    }
}

/// 列表 / 目录树里的一行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// 目录(仅树模式):`path` 是相对仓库根的目录路径,`name` 是显示名(紧凑目录会带 `/`)
    Dir { path: String, name: String, depth: usize, open: bool, files: usize },
    /// 文件:`ix` 是它在该组 `Vec<Item>` 里的下标
    File { ix: usize, depth: usize },
}

/// 目录树节点(构建用)。
#[derive(Default)]
struct Node {
    dirs: BTreeMap<String, Node>,
    files: Vec<usize>,
}

impl Node {
    fn count(&self) -> usize {
        self.files.len() + self.dirs.values().map(Node::count).sum::<usize>()
    }
}

/// 把一组文件整理成显示行。
///
/// 参数:`items` 该组的文件;`tree` 为真按目录树(目录在前、紧凑单子目录链,与 VS Code 默认一致),否则平铺;
/// `group` 用来拼折叠键;`collapsed` 已折叠的目录键(`<组>:<目录路径>`)。
pub fn rows(items: &[Item], tree: bool, group: Group, collapsed: &HashSet<String>) -> Vec<Row> {
    if !tree {
        return (0..items.len()).map(|ix| Row::File { ix, depth: 0 }).collect();
    }
    let mut root = Node::default();
    for (ix, it) in items.iter().enumerate() {
        let mut node = &mut root;
        let parts: Vec<&str> = it.path().split('/').collect();
        for d in &parts[..parts.len() - 1] {
            node = node.dirs.entry((*d).to_string()).or_default();
        }
        node.files.push(ix);
    }
    let mut out = Vec::new();
    walk(&root, "", 0, items, group, collapsed, &mut out);
    out
}

fn walk(node: &Node, prefix: &str, depth: usize, items: &[Item], group: Group, collapsed: &HashSet<String>, out: &mut Vec<Row>) {
    for (name, child) in &node.dirs {
        // 紧凑目录:只有一个子目录且没有文件时,合并成 `a/b/c`
        let (mut path, mut label, mut cur) = (join(prefix, name), name.clone(), child);
        while cur.files.is_empty() && cur.dirs.len() == 1 {
            let (n, c) = cur.dirs.iter().next().expect("长度已判断为 1");
            path = join(&path, n);
            label = format!("{label}/{n}");
            cur = c;
        }
        let open = !collapsed.contains(&format!("{}:{path}", group.key()));
        out.push(Row::Dir { path: path.clone(), name: label, depth, open, files: cur.count() });
        if open {
            walk(cur, &path, depth + 1, items, group, collapsed, out);
        }
    }
    let mut files = node.files.clone();
    files.sort_by_key(|&ix| items[ix].path().rsplit('/').next().unwrap_or("").to_lowercase());
    out.extend(files.into_iter().map(|ix| Row::File { ix, depth }));
}

fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() { name.to_string() } else { format!("{prefix}/{name}") }
}

/// 一个目录下(含子目录)属于该组的全部文件路径(目录行上的「暂存 / 放弃」按钮用)。
pub fn files_under(items: &[Item], dir: &str) -> Vec<StatusEntry> {
    let prefix = format!("{dir}/");
    items.iter().filter(|i| i.path().starts_with(&prefix)).map(|i| i.entry.clone()).collect()
}

/// 主按钮的文案与说明。
///
/// 参数:`staged` 暂存文件数;`changes` 未暂存文件数;`amend` 是否「修改上次提交」;`branch` 分支名。
/// 返回:(按钮文字, 输入框占位符)。
pub fn commit_labels(staged: usize, changes: usize, amend: bool, branch: Option<&str>) -> (String, String) {
    let on = branch.map(|b| format!("在「{b}」")).unwrap_or_default();
    let label = if amend {
        "修改上次提交".to_string()
    } else if staged == 0 && changes > 0 {
        "提交全部更改".to_string()
    } else {
        "提交".to_string()
    };
    (label, format!("消息(⌘Enter {on}提交)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(path: &str, index: char, worktree: char) -> StatusEntry {
        StatusEntry { path: path.into(), index, worktree, orig: None }
    }

    #[test]
    fn 同一文件两边都有改动会出现在两组() {
        let g = split(&[e("a.rs", 'M', 'M'), e("b.rs", ' ', 'M'), e("c.rs", 'A', ' '), e("d.rs", '?', '?'), e("x.rs", 'U', 'U')]);
        assert_eq!(g.staged.iter().map(|i| i.path()).collect::<Vec<_>>(), ["a.rs", "c.rs"]);
        assert_eq!(g.changes.iter().map(|i| i.path()).collect::<Vec<_>>(), ["a.rs", "b.rs", "d.rs"]);
        assert_eq!(g.conflicts.len(), 1);
        assert_eq!(g.changes[2].kind, 'U');
        assert_eq!(g.staged[1].kind, 'A');
        assert_eq!(g.distinct(), 5, "a.rs 只算一个文件");
    }

    #[test]
    fn 按路径不分大小写排序() {
        let g = split(&[e("b.rs", ' ', 'M'), e("A.rs", ' ', 'M'), e("a/z.rs", ' ', 'M')]);
        assert_eq!(g.changes.iter().map(|i| i.path()).collect::<Vec<_>>(), ["A.rs", "a/z.rs", "b.rs"]);
    }

    #[test]
    fn 平铺模式每个文件一行() {
        let g = split(&[e("src/a.rs", ' ', 'M'), e("b.rs", ' ', 'M')]);
        let r = rows(&g.changes, false, Group::Changes, &HashSet::new());
        assert_eq!(r, vec![Row::File { ix: 0, depth: 0 }, Row::File { ix: 1, depth: 0 }]);
    }

    #[test]
    fn 树模式目录在前并紧凑单子目录链() {
        let g = split(&[e("z.rs", ' ', 'M'), e("src/ui/a.rs", ' ', 'M'), e("src/ui/b.rs", ' ', 'M'), e("docs/deep/er/x.md", ' ', 'M')]);
        let r = rows(&g.changes, true, Group::Changes, &HashSet::new());
        // docs/deep/er 紧凑成一行;src/ui 紧凑成一行;根下文件 z.rs 在最后
        assert_eq!(r[0], Row::Dir { path: "docs/deep/er".into(), name: "docs/deep/er".into(), depth: 0, open: true, files: 1 });
        assert!(matches!(r[1], Row::File { depth: 1, .. }));
        assert_eq!(r[2], Row::Dir { path: "src/ui".into(), name: "src/ui".into(), depth: 0, open: true, files: 2 });
        assert!(matches!(r.last(), Some(Row::File { depth: 0, .. })));
        assert_eq!(r.len(), 1 + 1 + 1 + 2 + 1);
    }

    #[test]
    fn 折叠的目录不展开子行() {
        let g = split(&[e("src/a.rs", ' ', 'M'), e("src/b.rs", ' ', 'M')]);
        let mut c = HashSet::new();
        c.insert("cg:src".to_string());
        let r = rows(&g.changes, true, Group::Changes, &c);
        assert_eq!(r, vec![Row::Dir { path: "src".into(), name: "src".into(), depth: 0, open: false, files: 2 }]);
    }

    #[test]
    fn 目录下的文件() {
        let g = split(&[e("src/a.rs", ' ', 'M'), e("src/x/b.rs", ' ', 'M'), e("srcx/c.rs", ' ', 'M')]);
        let f = files_under(&g.changes, "src");
        assert_eq!(f.iter().map(|x| x.path.as_str()).collect::<Vec<_>>(), ["src/a.rs", "src/x/b.rs"], "srcx 不能被误算进 src");
    }

    #[test]
    fn 主按钮文案() {
        assert_eq!(commit_labels(2, 1, false, Some("main")).0, "提交");
        assert_eq!(commit_labels(0, 3, false, Some("main")).0, "提交全部更改");
        assert_eq!(commit_labels(0, 0, true, None).0, "修改上次提交");
        assert!(commit_labels(1, 0, false, Some("dev")).1.contains("「dev」"));
    }
}
