//! 「文件」面板(资源管理器):项目文件树。单击文件在编辑器里开预览标签、双击固定,支持拖放(`file_dnd.rs`)。

use crate::git_status::{GitMark, GitStatus};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// 文件树状态。
pub struct FileTree {
    root: PathBuf,
    expanded: HashSet<PathBuf>,
    children: HashMap<PathBuf, Vec<Node>>,
    selected: Option<PathBuf>,
    /// 文件与目录的 Git 改动标记
    git: GitStatus,
}

pub use crate::file_lang::{Node, language_for, read_dir_sorted};

impl FileTree {
    /// 展开着的目录(绝对路径,排序后,保存记忆用)。
    pub fn expanded_dirs(&self) -> Vec<String> {
        let mut v: Vec<String> = self.expanded.iter().map(|p| p.to_string_lossy().to_string()).collect();
        v.sort();
        v
    }

    /// 按记忆展开目录(只展开仍在项目内且存在的目录)。
    pub fn expand_dirs(&mut self, dirs: &[String]) {
        for d in dirs {
            let p = PathBuf::from(d);
            if p.is_dir() && p.starts_with(&self.root) && self.expanded.insert(p.clone()) {
                self.children.insert(p.clone(), read_dir_sorted(&p));
            }
        }
    }

    /// 项目根目录。
    pub fn root_path(&self) -> &Path {
        &self.root
    }

    /// 写入后台读到的 Git 状态。
    pub fn set_git(&mut self, git: GitStatus) {
        self.git = git;
    }

    /// 新建条目时的默认父目录:选中的目录,或选中文件所在目录,否则项目根。
    pub fn target_dir(&self) -> PathBuf {
        match &self.selected {
            Some(p) if p.is_dir() => p.clone(),
            Some(p) => p.parent().map(Path::to_path_buf).unwrap_or_else(|| self.root.clone()),
            None => self.root.clone(),
        }
    }

    /// 全部折叠。
    pub fn collapse_all(&mut self) {
        self.expanded.clear();
        self.reload();
    }

    /// 展开到某个路径的所有上级目录(不选中)。
    pub fn expand_to(&mut self, path: &Path) {
        let mut cur = path.parent();
        while let Some(dir) = cur {
            if dir == self.root || !dir.starts_with(&self.root) {
                break;
            }
            self.expanded.insert(dir.to_path_buf());
            cur = dir.parent();
        }
    }

    pub fn new(root: String) -> Self {
        let mut t = Self {
            root: PathBuf::from(root),
            expanded: HashSet::new(),
            children: HashMap::new(),
            selected: None,
            git: GitStatus::default(),
        };
        t.reload();
        t
    }

    /// 切换项目根目录。
    pub fn set_root(&mut self, root: String) {
        let root = PathBuf::from(root);
        if root == self.root {
            return;
        }
        self.root = root;
        self.expanded.clear();
        self.selected = None;
        self.reload();
    }

    /// 重新读取根目录与所有已展开目录。
    pub fn reload(&mut self) {
        self.children.clear();
        self.children.insert(self.root.clone(), read_dir_sorted(&self.root));
        for dir in self.expanded.clone() {
            self.children.insert(dir.clone(), read_dir_sorted(&dir));
        }
    }

    /// 增量刷新:只重读 `dirs` 里属于「根或已展开」的目录;已不存在的展开目录顺手清掉。
    /// 比 `reload` 轻得多,文件监视的批次用它。
    pub fn reload_dirs(&mut self, dirs: &std::collections::HashSet<PathBuf>) {
        self.expanded.retain(|d| d.is_dir());
        let (root, expanded) = (self.root.clone(), self.expanded.clone());
        self.children.retain(|d, _| *d == root || expanded.contains(d));
        for d in dirs {
            if *d == self.root || self.expanded.contains(d) {
                self.children.insert(d.clone(), read_dir_sorted(d));
            }
        }
    }

    fn toggle_dir(&mut self, dir: &Path) {
        if !self.expanded.remove(dir) {
            self.expanded.insert(dir.to_path_buf());
            self.children.insert(dir.to_path_buf(), read_dir_sorted(dir));
        }
    }

    /// 在树里选中并展开到某个文件。
    pub fn reveal(&mut self, path: &Path) {
        let mut cur = path.parent();
        while let Some(dir) = cur {
            if dir == self.root || !dir.starts_with(&self.root) {
                break;
            }
            if self.expanded.insert(dir.to_path_buf()) {
                self.children.insert(dir.to_path_buf(), read_dir_sorted(dir));
            }
            cur = dir.parent();
        }
        self.selected = Some(path.to_path_buf());
    }

    /// 递归把可见节点摊平成 (深度, 节点) 列表。
    fn flatten(&self) -> Vec<(usize, Node)> {
        fn go(t: &FileTree, dir: &Path, depth: usize, out: &mut Vec<(usize, Node)>) {
            if let Some(nodes) = t.children.get(dir) {
                for n in nodes {
                    out.push((depth, n.clone()));
                    if n.is_dir && t.expanded.contains(&n.path) {
                        go(t, &n.path, depth + 1, out);
                    }
                }
            }
        }
        let mut out = Vec::new();
        go(self, &self.root, 0, &mut out);
        out
    }

    /// 摊平后的可见行(渲染用)。
    pub(crate) fn rows(&self) -> Vec<(usize, Node)> {
        self.flatten()
    }

    pub(crate) fn is_selected(&self, p: &Path) -> bool {
        self.selected.as_deref() == Some(p)
    }

    pub(crate) fn is_open(&self, p: &Path) -> bool {
        self.expanded.contains(p)
    }

    pub(crate) fn git_mark(&self, p: &Path) -> Option<GitMark> {
        self.git.get(p)
    }

    /// 展开 / 收起一个目录。
    pub(crate) fn toggle(&mut self, dir: &Path) {
        self.toggle_dir(dir);
    }
}

#[cfg(test)]
mod incremental_tests {
    use super::*;
    use std::collections::HashSet;

    #[::core::prelude::v1::test]
    fn 增量重读只刷新指定的展开目录_清掉已删除的展开目录() {
        let root = std::env::temp_dir().join(format!("loom-tree-{}", std::process::id()));
        let (a, b) = (root.join("a"), root.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let mut t = FileTree::new(root.to_string_lossy().to_string());
        t.toggle(&a);
        t.toggle(&b);
        let names = |t: &FileTree, d: &Path| t.children.get(d).map(|v| v.iter().map(|n| n.name.clone()).collect::<Vec<_>>()).unwrap_or_default();
        // a、b 里各新建一个文件,但只通知 a 变了:b 的旧列表保持不变(证明没有整树重读)
        std::fs::write(a.join("x.txt"), b"1").unwrap();
        std::fs::write(b.join("y.txt"), b"1").unwrap();
        t.reload_dirs(&HashSet::from([a.clone()]));
        assert_eq!(names(&t, &a), ["x.txt"]);
        assert!(names(&t, &b).is_empty());
        // 展开的目录被删除:从展开集合与缓存里清掉
        std::fs::remove_dir_all(&b).unwrap();
        t.reload_dirs(&HashSet::from([root.clone()]));
        assert!(!t.is_open(&b) && !t.children.contains_key(&b));
        assert!(names(&t, &root).iter().all(|n| n != "b"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
