//! 文件树的 Git 状态:读取 `git status`,给文件与其上级目录标记改动类型。
//!
//! @author ixxxxoooo

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 单个路径的改动类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitMark {
    /// 已修改
    Modified,
    /// 新增或未跟踪
    Added,
    /// 已删除
    Deleted,
}

/// 路径到改动类型的映射(目录取其下最「重」的状态)。
#[derive(Default)]
pub struct GitStatus {
    marks: HashMap<PathBuf, GitMark>,
}

impl GitStatus {
    /// 读取 `root` 下所有仓库的状态:`root` 所在的仓库,加上子目录里发现的仓库(与源代码管理同一套发现规则,
    /// `git::find_repos` 深度 2),所以项目根本身不是仓库、仓库在子目录里时也有标记。不是仓库或 git 不可用时返回空。
    ///
    /// 参数:`root` 项目根目录。
    pub fn read(root: &Path) -> Self {
        let _span = loom_diag::span("git status", 300);
        let mut all = Self::default();
        for repo in loom_core::git::find_repos(&root.to_string_lossy(), 2) {
            all.merge(Self::read_repo(Path::new(&repo), root));
        }
        all
    }

    /// 读取一个仓库的状态,只保留 `root` 之下的路径。
    fn read_repo(repo: &Path, root: &Path) -> Self {
        let out = Command::new("git").arg("--no-optional-locks").arg("-C").arg(repo).args(["status", "--porcelain=v1", "-z", "-uall"]).output().ok().filter(|o| o.status.success());
        let Some(out) = out else { return Self::default() };
        Self::parse(&String::from_utf8_lossy(&out.stdout), repo, root)
    }

    /// 合并另一份标记;同一路径取更「重」的(修改盖过新增,与 `parse` 里上级目录的规则一致)。
    fn merge(&mut self, other: Self) {
        for (p, m) in other.marks {
            let slot = self.marks.entry(p).or_insert(m);
            if *slot == GitMark::Added && m != GitMark::Added {
                *slot = m;
            }
        }
    }

    /// 解析 `porcelain=v1 -z` 输出;`top` 为仓库根,只保留位于 `root` 之下的路径。
    pub fn parse(text: &str, top: &Path, root: &Path) -> Self {
        let mut marks: HashMap<PathBuf, GitMark> = HashMap::new();
        let mut parts = text.split('\0');
        while let Some(entry) = parts.next() {
            if entry.len() < 4 {
                continue;
            }
            let (xy, rel) = (&entry[..2], &entry[3..]);
            // 重命名后面还跟着一段原路径
            if xy.contains('R') || xy.contains('C') {
                parts.next();
            }
            let mark = if xy.contains('D') {
                GitMark::Deleted
            } else if xy.contains('A') || xy == "??" {
                GitMark::Added
            } else {
                GitMark::Modified
            };
            let full = top.join(rel);
            if !full.starts_with(root) {
                continue;
            }
            // 自身与到项目根之间的所有上级目录
            let mut cur: Option<&Path> = Some(&full);
            while let Some(p) = cur.filter(|p| *p != root && p.starts_with(root)) {
                let slot = marks.entry(p.to_path_buf()).or_insert(mark);
                if *slot == GitMark::Added && mark == GitMark::Modified {
                    *slot = GitMark::Modified;
                }
                cur = p.parent();
            }
        }
        Self { marks }
    }

    /// 依次读取多个根目录(多根工作区)并合并。
    pub fn read_all(roots: &[PathBuf]) -> Self {
        let mut all = Self::default();
        for r in roots {
            all.merge(Self::read(r));
        }
        all
    }

    /// 某路径的改动类型。
    pub fn get(&self, path: &Path) -> Option<GitMark> {
        self.marks.get(path).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 解析状态并标记上级目录() {
        let top = Path::new("/r");
        let s = GitStatus::parse(" M src/a.rs\0?? src/b.rs\0D  old.txt\0R  new.rs\0orig.rs\0", top, top);
        assert_eq!(s.get(Path::new("/r/src/a.rs")), Some(GitMark::Modified));
        assert_eq!(s.get(Path::new("/r/src/b.rs")), Some(GitMark::Added));
        assert_eq!(s.get(Path::new("/r/src")), Some(GitMark::Modified));
        assert_eq!(s.get(Path::new("/r/old.txt")), Some(GitMark::Deleted));
        assert_eq!(s.get(Path::new("/r/new.rs")), Some(GitMark::Modified));
        assert_eq!(s.get(Path::new("/r/orig.rs")), None);
    }

    #[test]
    fn 子目录里的仓库也标记到项目根下的上级目录() {
        // 项目根 /p 本身不是仓库,仓库在 /p/fe 与 /p/be
        let root = Path::new("/p");
        let mut s = GitStatus::parse(" M src/a.ts\0", Path::new("/p/fe"), root);
        s.merge(GitStatus::parse("?? new.go\0", Path::new("/p/be"), root));
        assert_eq!(s.get(Path::new("/p/fe/src/a.ts")), Some(GitMark::Modified));
        assert_eq!(s.get(Path::new("/p/fe")), Some(GitMark::Modified));
        assert_eq!(s.get(Path::new("/p/be/new.go")), Some(GitMark::Added));
        assert_eq!(s.get(Path::new("/p/be")), Some(GitMark::Added));
        assert_eq!(s.get(root), None);
    }

    #[test]
    fn 子目录项目只保留其下路径() {
        let s = GitStatus::parse(" M a/x.rs\0 M b/y.rs\0", Path::new("/r"), Path::new("/r/a"));
        assert_eq!(s.get(Path::new("/r/a/x.rs")), Some(GitMark::Modified));
        assert_eq!(s.get(Path::new("/r/b/y.rs")), None);
    }
}
