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

/// `git status` 的一项:一个有改动的文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    /// 相对仓库根目录的路径
    pub path: String,
    /// 暂存区状态(`git status --porcelain` 的第一列),空格表示没有
    pub index: char,
    /// 工作区状态(第二列),空格表示没有
    pub worktree: char,
    /// 重命名 / 复制时的原路径(相对仓库根目录)
    pub orig: Option<String>,
}

impl StatusEntry {
    /// 给用户看的单字母状态:未跟踪 `U`,冲突 `!`,否则优先工作区状态,再看暂存区。
    pub fn label(&self) -> char {
        match (self.index, self.worktree) {
            ('?', '?') => 'U',
            ('U', _) | (_, 'U') | ('A', 'A') | ('D', 'D') => '!',
            (i, ' ') => i,
            (_, w) => w,
        }
    }

    /// 是否是未跟踪的新文件(HEAD 里没有它)。
    pub fn untracked(&self) -> bool {
        self.index == '?' && self.worktree == '?'
    }

    /// 是否是合并冲突(双方修改、双方新增 / 删除等)。
    pub fn conflict(&self) -> bool {
        matches!((self.index, self.worktree), ('U', _) | (_, 'U') | ('A', 'A') | ('D', 'D'))
    }

    /// 暂存区里的改动类型(`M` `A` `D` `R` `C`);没有暂存改动、未跟踪或冲突时为 `None`。
    pub fn staged_kind(&self) -> Option<char> {
        (!self.conflict() && self.index != ' ' && self.index != '?').then_some(self.index)
    }

    /// 工作区里的改动类型(`M` `D` `U`=未跟踪);没有未暂存改动或冲突时为 `None`。
    pub fn unstaged_kind(&self) -> Option<char> {
        if self.conflict() || self.worktree == ' ' {
            return None;
        }
        Some(if self.worktree == '?' { 'U' } else { self.worktree })
    }

    /// 是否已经完全暂存(工作区没有进一步改动)。
    pub fn staged(&self) -> bool {
        self.index != ' ' && self.index != '?' && self.worktree == ' '
    }
}

/// 解析 `git status --porcelain=v1 -z` 的输出:`XY 路径\0`;重命名 / 复制多带一个原路径字段,会被跳过;
/// 未跟踪的嵌套仓库目录条目(`?? sub/`)忽略。
pub fn parse_status(out: &str) -> Vec<StatusEntry> {
    let mut fields = out.split('\0');
    let mut list = Vec::new();
    while let Some(f) = fields.next() {
        let mut cs = f.chars();
        let (Some(index), Some(worktree), Some(' ')) = (cs.next(), cs.next(), cs.next()) else { continue };
        let path = cs.as_str();
        // 未跟踪的嵌套仓库,git 只报一个带斜杠的目录条目(`sub/`):它不是文件、也不该被当文件暂存,
        // 嵌套仓库由仓库发现(`find_repos`)作为独立仓库列出,这里不算改动
        if path.is_empty() || (index == '?' && path.ends_with('/')) {
            continue;
        }
        let orig = if matches!(index, 'R' | 'C') || matches!(worktree, 'R' | 'C') { fields.next().map(str::to_string) } else { None };
        list.push(StatusEntry { path: path.to_string(), index, worktree, orig });
    }
    list
}

/// 一个目录的 Git 状态快照:分支与 worktree 列表,供界面渲染菜单用。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitSnapshot {
    pub branches: Vec<Branch>,
    pub worktrees: Vec<Worktree>,
}

impl GitSnapshot {
    /// 读取快照;`dir` 不在 Git 仓库里返回 `None`。会起 git 子进程,放到后台线程调用。
    pub fn load(dir: &str) -> Option<Self> {
        repo_root(dir)?;
        Some(Self { branches: branches(dir).unwrap_or_default(), worktrees: worktrees(dir).unwrap_or_default() })
    }

    /// 当前检出的分支名(游离 HEAD 时为 `None`)。
    pub fn current(&self) -> Option<&str> {
        self.branches.iter().find(|b| b.current).map(|b| b.name.as_str())
    }
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
///
/// 不允许 git 在终端上询问凭据(`GIT_TERMINAL_PROMPT=0`),拉取 / 推送缺凭据时立刻失败而不是卡住。
pub fn git(dir: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).env("GIT_TERMINAL_PROMPT", "0").output().map_err(|e| format!("无法运行 git:{e}"))?;
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

/// 扫描子目录时跳过的目录名(体积大、不会是用户自己的仓库)。
const SCAN_SKIP: &[&str] = &["node_modules", "target", "dist", "build", "vendor", "Pods", "DerivedData", "__pycache__", "venv"];

/// 找出 `dir` 相关的所有仓库根(对标 VS Code 的 `git.autoRepositoryDetection`):
/// 先是包含 `dir` 的仓库(`dir` 在仓库里时),再是 `dir` 往下 `depth` 层里的仓库(含 `.git` 的目录,
/// 找到就不再往它里面扫;隐藏目录和 `SCAN_SKIP` 里的目录跳过)。结果按路径排序、去重。
/// `dir` 已经在仓库里时也照常往下找:里面嵌套的子仓库(克隆进来的项目、子模块)各自是一个仓库,
/// 外层仓库的状态里不再把它们当成一个个怪条目。
pub fn find_repos(dir: &str, depth: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let enclosing = repo_root(dir);
    if let Some(top) = &enclosing {
        out.push(top.clone());
    }
    let base = std::path::Path::new(dir);
    let mut found: Vec<String> = Vec::new();
    let mut budget = 4000usize;
    scan_repos(base, depth, &mut found, &mut budget);
    found.sort();
    out.extend(found);
    out.dedup();
    out
}

fn scan_repos(dir: &std::path::Path, depth: usize, found: &mut Vec<String>, budget: &mut usize) {
    if depth == 0 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || SCAN_SKIP.contains(&name.as_str()) {
            continue;
        }
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        if p.join(".git").exists() {
            if let Some(s) = p.canonicalize().ok().and_then(|c| c.to_str().map(String::from)) {
                found.push(s);
            }
        } else {
            scan_repos(&p, depth - 1, found, budget);
        }
    }
}

/// 本地分支列表。
pub fn branches(dir: &str) -> Result<Vec<Branch>, String> {
    git(dir, &["branch", "--format=%(HEAD)%(refname:short)"]).map(|o| parse_branches(&o))
}

/// 所有 worktree(第一项是主工作区)。
pub fn worktrees(dir: &str) -> Result<Vec<Worktree>, String> {
    git(dir, &["worktree", "list", "--porcelain"]).map(|o| parse_worktrees(&o))
}

/// 工作区有改动的文件(含未跟踪的单个文件)。路径相对仓库根目录。
pub fn status(dir: &str) -> Result<Vec<StatusEntry>, String> {
    git(dir, &["--no-optional-locks", "status", "--porcelain=v1", "-z", "--untracked-files=all"]).map(|o| parse_status(&o))
}

/// 某个文件在 HEAD 里的内容;HEAD 里没有(新文件、空仓库)或不是文本时返回 `None`。
///
/// 参数:`dir` 仓库内任意目录;`rel` 相对仓库根目录的路径。
pub fn file_at_head(dir: &str, rel: &str) -> Option<String> {
    git(dir, &["show", &format!("HEAD:{rel}")]).ok()
}

/// 解析 `git branch -r --format=%(refname:short)`:去掉 `origin/HEAD` 与已有同名本地分支的远程分支。
pub fn parse_remote_branches(out: &str, local: &[Branch]) -> Vec<String> {
    out.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.contains("->") && !l.ends_with("/HEAD") && l.contains('/'))
        .filter(|l| {
            let short = l.split_once('/').map_or(*l, |(_, b)| b);
            !local.iter().any(|b| b.name == short)
        })
        .map(str::to_string)
        .collect()
}

/// 还没有本地对应分支的远程分支(如 `origin/feature-x`)。
pub fn remote_branches(dir: &str, local: &[Branch]) -> Vec<String> {
    git(dir, &["branch", "-r", "--format=%(refname:short)"]).map(|o| parse_remote_branches(&o, local)).unwrap_or_default()
}

/// 检出远程分支:建立同名本地分支并跟踪它。
pub fn switch_remote_branch(dir: &str, remote_ref: &str) -> Result<(), String> {
    validate_branch_name(remote_ref)?;
    git(dir, &["switch", "--track", remote_ref]).map(|_| ())
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
    fn 扫描子目录里的仓库() {
        let base = std::env::temp_dir().join(format!("loom-find-repos-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        for d in ["a/.git", "grp/b/.git", "grp/b/inner/.git", "node_modules/x/.git", ".hid/c/.git", "plain/sub"] {
            std::fs::create_dir_all(base.join(d)).unwrap();
        }
        let got = find_repos(base.to_str().unwrap(), 2);
        let names: Vec<String> = got.iter().map(|p| p.rsplit('/').next().unwrap().to_string()).collect();
        // a、grp/b;不进 node_modules / 隐藏目录,找到仓库后不再扫它里面的 inner
        assert_eq!(names, vec!["a", "b"], "{got:?}");
        // 深度 1 只能看到 a
        assert_eq!(find_repos(base.to_str().unwrap(), 1).len(), 1);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn 外层已是仓库时仍发现嵌套的子仓库且状态里没有怪条目() {
        let base = std::env::temp_dir().join(format!("loom-nested-repos-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("kb")).unwrap();
        let run = |dir: &std::path::Path, args: &[&str]| {
            let ok = std::process::Command::new("git").arg("-C").arg(dir).args(args).output().unwrap().status.success();
            assert!(ok, "git {args:?}");
        };
        run(&base, &["init", "-q"]);
        run(&base.join("kb"), &["init", "-q"]);
        std::fs::write(base.join("kb/f.txt"), "x").unwrap();
        run(&base.join("kb"), &["add", "f.txt"]);
        run(&base.join("kb"), &["-c", "user.email=a@b", "-c", "user.name=n", "commit", "-qm", "i"]);
        std::fs::write(base.join("top.txt"), "y").unwrap();
        let root = base.to_str().unwrap();
        // 外层状态:嵌套仓库不算改动,只剩 top.txt
        let st = status(root).unwrap();
        assert_eq!(st.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(), vec!["top.txt"], "{st:?}");
        // 仓库发现:外层在前,子仓库随后
        let repos = find_repos(root, 2);
        let names: Vec<&str> = repos.iter().map(|p| p.rsplit('/').next().unwrap()).collect();
        assert_eq!(repos.len(), 2, "{repos:?}");
        assert_eq!(names[1], "kb");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn 状态解析忽略未跟踪的嵌套仓库目录() {
        let e = parse_status("?? kb/\0?? kb-fe/\0?? 新文件.txt\0 M sub\0");
        let paths: Vec<&str> = e.iter().map(|x| x.path.as_str()).collect();
        assert_eq!(paths, vec!["新文件.txt", "sub"], "已跟踪的子模块改动(无斜杠)要保留");
    }

    #[test]
    fn 远程分支去掉_head_与已有本地分支() {
        let local = vec![Branch { name: "main".into(), current: true }];
        let out = "origin\norigin/HEAD\norigin/main\norigin/feature/x\nupstream/dev\n";
        assert_eq!(parse_remote_branches(out, &local), vec!["origin/feature/x".to_string(), "upstream/dev".to_string()]);
    }

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
    fn 解析状态含重命名与特殊路径() {
        let out = "M  a.rs\0 M b b.rs\0?? 中文 文件.md\0R  new.rs\0old.rs\0 D gone.rs\0AM added.rs\0UU conflict.rs\0";
        let e = parse_status(out);
        let paths: Vec<_> = e.iter().map(|x| x.path.as_str()).collect();
        // 重命名的原路径 old.rs 不能当成独立条目
        assert_eq!(paths, ["a.rs", "b b.rs", "中文 文件.md", "new.rs", "gone.rs", "added.rs", "conflict.rs"]);
        let label = |p: &str| e.iter().find(|x| x.path == p).unwrap().label();
        assert_eq!(label("a.rs"), 'M');
        assert_eq!(label("b b.rs"), 'M');
        assert_eq!(label("中文 文件.md"), 'U');
        assert_eq!(label("new.rs"), 'R');
        assert_eq!(label("gone.rs"), 'D');
        assert_eq!(label("added.rs"), 'M', "已暂存的新文件又被改过:显示工作区的 M");
        assert_eq!(label("conflict.rs"), '!');
        assert_eq!(e.iter().find(|x| x.path == "new.rs").unwrap().orig.as_deref(), Some("old.rs"));
        assert!(e[2].untracked() && !e[0].untracked());
        assert!(e[0].staged() && !e[1].staged());
    }

    #[test]
    fn 解析状态忽略空与畸形字段() {
        assert!(parse_status("").is_empty());
        assert!(parse_status("\0\0").is_empty());
        assert!(parse_status("x\0M\0").is_empty());
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
        let base = std::env::temp_dir().join(format!("loom-git-{}", std::process::id()));
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

        // 工作区状态:修改、新增(未跟踪)、删除、含中文文件名
        std::fs::write(repo.join("a.txt"), "changed").unwrap();
        std::fs::write(repo.join("新文件.txt"), "new").unwrap();
        run(&["rm", "-q", "--cached", "a.txt"]);
        let st = status(&dir).unwrap();
        assert!(st.iter().any(|e| e.path == "新文件.txt" && e.untracked()), "状态:{st:?}");
        assert!(st.iter().any(|e| e.path == "a.txt" && e.index == 'D'), "状态:{st:?}");
        // HEAD 里的旧内容;新文件在 HEAD 里没有
        assert_eq!(file_at_head(&dir, "a.txt").as_deref(), Some("x"));
        assert_eq!(file_at_head(&dir, "新文件.txt"), None);
        run(&["reset", "-q", "--hard"]);
        std::fs::remove_file(repo.join("新文件.txt")).unwrap();
        assert!(status(&dir).unwrap().is_empty(), "清理后应干净");

        // 游离 HEAD:列表里不能出现伪分支
        run(&["checkout", "-q", "--detach"]);
        assert!(branches(&dir).unwrap().iter().all(|b| !b.current && !b.name.contains("HEAD")));
        run(&["checkout", "-q", "main"]);

        remove_worktree(&dir, &wt.to_string_lossy()).unwrap();
        assert_eq!(worktrees(&dir).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(base);
    }
}
