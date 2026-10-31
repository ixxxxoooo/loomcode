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
    /// 读取 `root` 所在仓库的状态;不是仓库或 git 不可用时返回空。
    ///
    /// 参数:`root` 项目根目录。
    pub fn read(root: &Path) -> Self {
        let run = |args: &[&str]| Command::new("git").arg("-C").arg(root).args(args).output().ok().filter(|o| o.status.success());
        let Some(top) = run(&["rev-parse", "--show-toplevel"]) else { return Self::default() };
        let top = PathBuf::from(String::from_utf8_lossy(&top.stdout).trim());
        let Some(out) = run(&["status", "--porcelain=v1", "-z", "-uall"]) else { return Self::default() };
        Self::parse(&String::from_utf8_lossy(&out.stdout), &top, root)
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
    fn 子目录项目只保留其下路径() {
        let s = GitStatus::parse(" M a/x.rs\0 M b/y.rs\0", Path::new("/r"), Path::new("/r/a"));
        assert_eq!(s.get(Path::new("/r/a/x.rs")), Some(GitMark::Modified));
        assert_eq!(s.get(Path::new("/r/b/y.rs")), None);
    }
}
