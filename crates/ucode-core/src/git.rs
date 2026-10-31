//! Git 分支与 worktree 的封装:解析命令行输出的纯函数 + 薄薄一层命令调用。
//!
//! 全部走 `git` 命令行(不引入 libgit2),调用方应放到后台线程里执行。
//!
//! @author ixxxxoooo

use std::path::{Path, PathBuf};
use std::process::Command;

/// 一个本地分支。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    pub name: String,
    /// 是否是当前检出的分支
    pub current: bool,
}

/// 一个 worktree(含主工作区)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: String,
    /// 检出的分支;游离 HEAD 或 bare 时为 `None`
    pub branch: Option<String>,
    /// 列表里的第一项是主工作区
    pub is_main: bool,
}

/// 解析 `git branch --format=%(HEAD)%(refname:short)` 的输出(每行首字符 `*` 表示当前分支)。
pub fn parse_branches(out: &str) -> Vec<Branch> {
    out.lines()
        .filter_map(|l| {
            let current = l.starts_with('*');
            let name = l.get(1..)?.trim();
            // 游离 HEAD 时 git 会输出 `(HEAD detached at xxx)`,不是真分支
            (!name.is_empty() && !name.starts_with('(')).then(|| Branch { name: name.to_string(), current })
        })
        .collect()
}

/// 解析 `git worktree list --porcelain` 的输出(空行分隔的块)。
pub fn parse_worktrees(out: &str) -> Vec<Worktree> {
    let mut list: Vec<Worktree> = Vec::new();
    for block in out.split("\n\n").filter(|b| !b.trim().is_empty()) {
        let mut path = None;
        let mut branch = None;
        for line in block.lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                path = Some(p.to_string());
            } else if let Some(b) = line.strip_prefix("branch ") {
                branch = Some(b.strip_prefix("refs/heads/").unwrap_or(b).to_string());
            }
        }
        if let Some(path) = path {
            let is_main = list.is_empty();
            list.push(Worktree { path, branch, is_main });
        }
    }
    list
}

/// 校验分支名(比 git 自己的规则保守一点,够用且好解释)。
///
/// 返回:合法返回 `Ok(())`,否则返回给用户看的中文原因。
pub fn validate_branch_name(name: &str) -> Result<(), String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("请填写分支名".into());
    }
    if n != name || n.starts_with('-') || n.starts_with('/') || n.ends_with('/') || n.ends_with('.') || n.ends_with(".lock") || n.contains("..") || n.contains("//") || n.contains("@{") {
        return Err("分支名格式不合法".into());
    }
    if n.chars().any(|c| c.is_whitespace() || c.is_control() || "~^:?*[\\".contains(c)) {
        return Err("分支名不能包含空格或 ~ ^ : ? * [ \\ 等字符".into());
    }
    Ok(())
}

/// 仓库的稳定标识:`<目录名>-<路径哈希>`,用来给不同仓库的 worktree 分目录存放。
pub fn repo_key(repo_root: &str) -> String {
    // FNV-1a:不引入依赖,只求同一路径稳定、不同路径几乎不撞
    let hash = repo_root.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    let name: String = crate::workspace::project_name_from_path(repo_root).chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    format!("{name}-{:08x}", hash as u32)
}

/// worktree 的存放路径:`<root>/<repo_key>/<分支名里的 / 换成 ->`。
pub fn worktree_path(root: &Path, repo_root: &str, branch: &str) -> PathBuf {
    root.join(repo_key(repo_root)).join(branch.replace('/', "-"))
}

/// 执行 git 命令,成功返回 stdout,失败返回 stderr 的首行(给用户看)。
fn git(dir: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output().map_err(|e| format!("无法运行 git:{e}"))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).to_string());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    Err(err.lines().find(|l| !l.trim().is_empty()).unwrap_or("git 命令失败").trim().to_string())
}

/// 仓库根目录;不是仓库返回 `None`。
pub fn repo_root(dir: &str) -> Option<String> {
    git(dir, &["rev-parse", "--show-toplevel"]).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// 本地分支列表。
pub fn branches(dir: &str) -> Result<Vec<Branch>, String> {
    git(dir, &["branch", "--format=%(HEAD)%(refname:short)"]).map(|o| parse_branches(&o))
}

/// 所有 worktree(第一项是主工作区)。
pub fn worktrees(dir: &str) -> Result<Vec<Worktree>, String> {
    git(dir, &["worktree", "list", "--porcelain"]).map(|o| parse_worktrees(&o))
}

/// 切换到已有分支。工作区有冲突改动时 git 会拒绝,错误原样返回。
pub fn switch_branch(dir: &str, name: &str) -> Result<(), String> {
    validate_branch_name(name)?;
    git(dir, &["switch", name]).map(|_| ())
}

/// 基于当前 HEAD 新建分支并切换过去。
pub fn create_branch(dir: &str, name: &str) -> Result<(), String> {
    validate_branch_name(name)?;
    git(dir, &["switch", "-c", name]).map(|_| ())
}

/// 新建 worktree:在 `path` 检出新分支 `branch`(基于 `base`,为空则基于当前 HEAD)。
pub fn add_worktree(dir: &str, path: &Path, branch: &str, base: Option<&str>) -> Result<(), String> {
    validate_branch_name(branch)?;
    let path = path.to_string_lossy();
    let mut args = vec!["worktree", "add", "-b", branch, path.as_ref()];
    if let Some(b) = base.filter(|b| !b.is_empty()) {
        validate_branch_name(b)?;
        args.push(b);
    }
    git(dir, &args).map(|_| ())
}

/// 移除 worktree(有未提交改动时 git 会拒绝)。
pub fn remove_worktree(dir: &str, path: &str) -> Result<(), String> {
    git(dir, &["worktree", "remove", path]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 解析分支列表() {
        // 每行首字符是 %(HEAD) 标记:当前分支 `*`,其余是空格;游离 HEAD 的真实输出是 `*(HEAD detached at ..)`
        let b = parse_branches("*main\n feature/x\n*(HEAD detached at abc)\n\n");
        assert_eq!(b, vec![Branch { name: "main".into(), current: true }, Branch { name: "feature/x".into(), current: false }]);
    }

    #[test]
    fn 解析_worktree_列表() {
        let out = "worktree /repo\nHEAD abc\nbranch refs/heads/main\n\nworktree /wt/a\nHEAD def\nbranch refs/heads/feature/a\n\nworktree /wt/d\nHEAD 123\ndetached\n";
        let w = parse_worktrees(out);
        assert_eq!(w.len(), 3);
        assert!(w[0].is_main && w[0].branch.as_deref() == Some("main"));
        assert_eq!(w[1].branch.as_deref(), Some("feature/a"));
        assert!(!w[1].is_main);
        assert_eq!(w[2].branch, None);
    }

    #[test]
    fn 分支名校验() {
        for ok in ["main", "feature/login", "fix-1_2", "release/1.0"] {
            assert!(validate_branch_name(ok).is_ok(), "{ok}");
        }
        for bad in ["", " ", "a b", "-x", "a..b", "a/", "/a", "a.lock", "a~1", "a:b", "a//b", " lead", "x."] {
            assert!(validate_branch_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn 仓库标识稳定且区分路径() {
        assert_eq!(repo_key("/a/proj"), repo_key("/a/proj"));
        assert_ne!(repo_key("/a/proj"), repo_key("/b/proj"));
        assert!(repo_key("/a/my proj").starts_with("my_proj-"));
        let p = worktree_path(Path::new("/w"), "/a/proj", "feature/x");
        assert!(p.ends_with("feature-x") && p.starts_with("/w"));
    }

    /// 真实仓库端到端:建分支、建 / 列 / 删 worktree、切换分支、错误透传。
    #[test]
    fn 真实仓库的分支与_worktree_流程() {
        let base = std::env::temp_dir().join(format!("ucode-git-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let repo = base.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let dir = repo.to_string_lossy().to_string();
        let run = |args: &[&str]| {
            let ok = Command::new("git").arg("-C").arg(&dir).args(args)
                .env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t")
                .output().unwrap().status.success();
            assert!(ok, "git {args:?} 失败");
        };
        run(&["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("a.txt"), "x").unwrap();
        run(&["add", "."]);
        run(&["commit", "-q", "-m", "init"]);

        assert!(repo_root(&dir).is_some());
        assert_eq!(branches(&dir).unwrap(), vec![Branch { name: "main".into(), current: true }]);

        create_branch(&dir, "dev").unwrap();
        switch_branch(&dir, "main").unwrap();
        assert!(branches(&dir).unwrap().iter().any(|b| b.name == "dev" && !b.current));
        assert!(switch_branch(&dir, "nope").is_err(), "不存在的分支要报错");
        assert!(create_branch(&dir, "dev").is_err(), "重复分支要报错");

        let wt = worktree_path(&base.join("wts"), &dir, "feature/x");
        add_worktree(&dir, &wt, "feature/x", None).unwrap();
        let list = worktrees(&dir).unwrap();
        assert_eq!(list.len(), 2);
        assert!(list[0].is_main);
        assert_eq!(list[1].branch.as_deref(), Some("feature/x"));
        assert!(wt.join("a.txt").exists());
        // 同一分支不能在两个 worktree 里同时检出,错误透传
        assert!(add_worktree(&dir, &base.join("wts/other"), "feature/x", None).is_err());

        // 游离 HEAD:列表里不能出现伪分支
        run(&["checkout", "-q", "--detach"]);
        assert!(branches(&dir).unwrap().iter().all(|b| !b.current && !b.name.contains("HEAD")));
        run(&["checkout", "-q", "main"]);

        remove_worktree(&dir, &wt.to_string_lossy()).unwrap();
        assert_eq!(worktrees(&dir).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(base);
    }
}
