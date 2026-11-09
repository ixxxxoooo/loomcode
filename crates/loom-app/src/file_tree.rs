//! 「文件」面板(资源管理器):项目文件树。单击文件在编辑器里开预览标签、双击固定,支持拖放(`file_dnd.rs`)。

use crate::git_status::{GitMark, GitStatus};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// 文件树状态。
pub struct FileTree {
    /// 主目录(多根工作区的第一个文件夹;新建条目、拖放到空白处的默认位置)
    root: PathBuf,
    /// 多根工作区的其余文件夹。不为空时进入多根模式:每个文件夹作为一个可折叠的顶层节点(对标 VS Code)
    extra: Vec<PathBuf>,
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
            if p.is_dir() && self.roots().any(|r| p.starts_with(r)) && self.expanded.insert(p.clone()) {
                self.children.insert(p.clone(), read_dir_sorted(&p));
            }
        }
    }

    /// 项目根目录(多根时是主目录)。
    pub fn root_path(&self) -> &Path {
        &self.root
    }

    /// 全部根目录(主目录在前)。
    pub fn roots(&self) -> impl Iterator<Item = &PathBuf> {
        std::iter::once(&self.root).chain(&self.extra)
    }

    /// 是否多根模式(工作区有两个及以上文件夹)。
    pub fn is_multi(&self) -> bool {
        !self.extra.is_empty()
    }

    /// 是否某个根目录本身(多根模式下的顶层节点)。
    pub fn is_root(&self, p: &Path) -> bool {
        self.roots().any(|r| r == p)
    }

    /// 路径所在的根目录(嵌套时取最深的;都不包含时退回主目录)。
    pub fn root_of(&self, p: &Path) -> PathBuf {
        self.roots().filter(|r| p.starts_with(r)).max_by_key(|r| r.as_os_str().len()).unwrap_or(&self.root).clone()
    }

    /// 写入后台读到的 Git 状态。
    pub fn set_git(&mut self, git: GitStatus) {
        self.git = git;
    }

    /// 新建条目时的默认父目录:选中的目录,或选中文件所在目录,否则项目根。
    pub fn target_dir(&self) -> PathBuf {
        match &self.selected {
            Some(p) if p.is_dir() => p.clone(),
            Some(p) => p.parent().map(Path::to_path_buf).unwrap_or_else(|| self.root_of(p)),
            None => self.root.clone(),
        }
    }

    /// 全部折叠。
    pub fn collapse_all(&mut self) {
        self.expanded.clear();
        self.reload();
    }

    /// 根目录本身是否展开:单根时恒展开(不显示根节点),多根时看展开集合。
    fn root_open(&self, r: &Path) -> bool {
        !self.is_multi() || self.expanded.contains(r)
    }

    /// 展开到某个路径的所有上级目录(不选中)。
    pub fn expand_to(&mut self, path: &Path) {
        let root = self.root_of(path);
        let mut cur = path.parent();
        while let Some(dir) = cur {
            if !dir.starts_with(&root) {
                break;
            }
            // 单根时根目录不在展开集合里(它总是展开的);多根时根节点也要展开
            if dir == root && !self.is_multi() {
                break;
            }
            if self.expanded.insert(dir.to_path_buf()) {
                self.children.insert(dir.to_path_buf(), read_dir_sorted(dir));
            }
            if dir == root {
                break;
            }
            cur = dir.parent();
        }
    }

    pub fn new(root: String) -> Self {
        let mut t = Self {
            root: PathBuf::from(root),
            extra: Vec::new(),
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
        self.set_roots(&[root]);
    }

    /// 切换到一组根目录(第一个是主目录);没变就什么都不做。多根时各文件夹默认展开。
    pub fn set_roots(&mut self, roots: &[String]) {
        let Some((first, rest)) = roots.split_first() else { return };
        let (root, extra) = (PathBuf::from(first), rest.iter().map(PathBuf::from).collect::<Vec<_>>());
        if root == self.root && extra == self.extra {
            return;
        }
        self.root = root;
        self.extra = extra;
        self.expanded.clear();
        self.selected = None;
        if self.is_multi() {
            let all: Vec<PathBuf> = self.roots().cloned().collect();
            self.expanded.extend(all);
        }
        self.reload();
    }

    /// 重新读取根目录与所有已展开目录。
    pub fn reload(&mut self) {
        self.children.clear();
        for r in self.roots().cloned().collect::<Vec<_>>() {
            self.children.insert(r.clone(), read_dir_sorted(&r));
        }
        for dir in self.expanded.clone() {
            self.children.insert(dir.clone(), read_dir_sorted(&dir));
        }
    }

    /// 增量刷新:只重读 `dirs` 里属于「根或已展开」的目录;已不存在的展开目录顺手清掉。
    /// 比 `reload` 轻得多,文件监视的批次用它。
    pub fn reload_dirs(&mut self, dirs: &std::collections::HashSet<PathBuf>) {
        self.expanded.retain(|d| d.is_dir());
        let (roots, expanded): (Vec<PathBuf>, _) = (self.roots().cloned().collect(), self.expanded.clone());
        self.children.retain(|d, _| roots.contains(d) || expanded.contains(d));
        for d in dirs {
            if roots.contains(d) || self.expanded.contains(d) {
                self.children.insert(d.clone(), read_dir_sorted(d));
            }
        }
    }

    fn toggle_dir(&mut self, dir: &Path) {
        // 单根时根目录不显示,也不能收起
        if dir == self.root && !self.is_multi() {
            return;
        }
        if !self.expanded.remove(dir) {
            self.expanded.insert(dir.to_path_buf());
            self.children.insert(dir.to_path_buf(), read_dir_sorted(dir));
        }
    }

    /// 在树里选中并展开到某个文件。
    pub fn reveal(&mut self, path: &Path) {
        self.expand_to(path);
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
        if !self.is_multi() {
            go(self, &self.root, 0, &mut out);
            return out;
        }
        // 多根:每个文件夹一个顶层节点,内容缩进一级
        for r in self.roots() {
            let name = r.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| r.display().to_string());
            out.push((0, Node { name, path: r.clone(), is_dir: true }));
            if self.root_open(r) {
                go(self, r, 1, &mut out);
            }
        }
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
mod multi_root_tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 多根时每个文件夹一个可折叠的顶层节点() {
        let base = std::env::temp_dir().join(format!("loom-tree-multi-{}", std::process::id()));
        let (a, b) = (base.join("a"), base.join("b"));
        std::fs::create_dir_all(a.join("src")).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join("src/x.rs"), b"1").unwrap();
        std::fs::write(b.join("y.txt"), b"1").unwrap();
        let mut t = FileTree::new(a.to_string_lossy().to_string());
        // 单根:不显示根节点
        assert_eq!(t.rows().iter().map(|(d, n)| (*d, n.name.clone())).collect::<Vec<_>>(), [(0, "src".to_string())]);
        t.set_roots(&[a.to_string_lossy().to_string(), b.to_string_lossy().to_string()]);
        assert!(t.is_multi());
        let rows = |t: &FileTree| t.rows().iter().map(|(d, n)| (*d, n.name.clone())).collect::<Vec<_>>();
        assert_eq!(rows(&t), [(0, "a".into()), (1, "src".into()), (0, "b".into()), (1, "y.txt".into())]);
        // 折叠第二个文件夹
        t.toggle(&b);
        assert_eq!(rows(&t), [(0, "a".into()), (1, "src".into()), (0, "b".into())]);
        // 定位到 b 里的文件会把 b 重新展开;所在根按路径判断
        t.reveal(&b.join("y.txt"));
        assert!(t.is_open(&b));
        assert_eq!(t.root_of(&b.join("y.txt")), b);
        assert_eq!(t.target_dir(), b);
        // 定位到 a/src/x.rs 展开 src
        t.reveal(&a.join("src/x.rs"));
        assert!(t.is_open(&a.join("src")));
        let _ = std::fs::remove_dir_all(&base);
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
