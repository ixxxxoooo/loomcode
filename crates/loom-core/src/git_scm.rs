//! 源代码管理(对标 VS Code 的 Source Control)用到的 Git 操作:暂存、取消暂存、放弃、提交、
//! 拉取 / 推送 / 同步,以及读取分支与上游的领先落后。
//!
//! 全部走 `git` 命令行,会起子进程,调用方应放到后台线程里执行。
//! 路径一律是相对仓库根目录的路径,`dir` 传仓库根目录(`git::repo_root` 的结果)。
//!
//! @author ixxxxoooo

use crate::git::{StatusEntry, git};

/// 当前分支与上游的关系(状态栏、「同步更改」按钮用)。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncInfo {
    /// 当前分支名;游离 HEAD 时为 `None`
    pub branch: Option<String>,
    /// 上游分支(如 `origin/main`);没有设置上游时为 `None`(界面显示「发布分支」)
    pub upstream: Option<String>,
    /// 本地领先上游的提交数(待推送)
    pub ahead: u32,
    /// 本地落后上游的提交数(待拉取)
    pub behind: u32,
    /// 是否有任何一个远程(没有远程时不显示同步 / 发布)
    pub has_remote: bool,
    /// 仓库里是否已经有提交(空仓库时「取消暂存」要换一种做法)
    pub has_commits: bool,
}

/// 解析 `git rev-list --left-right --count @{u}...HEAD` 的输出 `落后\t领先`。
pub fn parse_ahead_behind(out: &str) -> (u32, u32) {
    let mut it = out.split_whitespace().filter_map(|x| x.parse::<u32>().ok());
    let behind = it.next().unwrap_or(0);
    let ahead = it.next().unwrap_or(0);
    (ahead, behind)
}

/// 读取分支、上游与领先落后。不是仓库时返回默认值。
pub fn sync_info(dir: &str) -> SyncInfo {
    let branch = git(dir, &["symbolic-ref", "--short", "-q", "HEAD"]).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let has_commits = git(dir, &["rev-parse", "--verify", "-q", "HEAD"]).is_ok();
    let has_remote = git(dir, &["remote"]).is_ok_and(|s| !s.trim().is_empty());
    let upstream = git(dir, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let (ahead, behind) = match &upstream {
        Some(_) => git(dir, &["rev-list", "--left-right", "--count", "@{u}...HEAD"]).map(|o| parse_ahead_behind(&o)).unwrap_or((0, 0)),
        None => (0, 0),
    };
    SyncInfo { branch, upstream, ahead, behind, has_remote, has_commits }
}

/// 暂存指定文件(含删除、新增、重命名)。
pub fn stage(dir: &str, paths: &[String]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args = vec!["add", "-A", "--"];
    args.extend(paths.iter().map(String::as_str));
    git(dir, &args).map(|_| ())
}

/// 暂存全部改动(含未跟踪)。
pub fn stage_all(dir: &str) -> Result<(), String> {
    git(dir, &["add", "-A"]).map(|_| ())
}

/// 取消暂存指定文件(改动保留在工作区)。空仓库(还没有提交)时从索引里移除。
pub fn unstage(dir: &str, paths: &[String], has_commits: bool) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args: Vec<&str> = if has_commits { vec!["restore", "--staged", "--"] } else { vec!["rm", "--cached", "-r", "-q", "--"] };
    args.extend(paths.iter().map(String::as_str));
    git(dir, &args).map(|_| ())
}

/// 取消暂存全部。
pub fn unstage_all(dir: &str, has_commits: bool) -> Result<(), String> {
    if has_commits { git(dir, &["restore", "--staged", "."]).map(|_| ()) } else { git(dir, &["rm", "--cached", "-r", "-q", "."]).map(|_| ()) }
}

/// 放弃工作区的改动:已跟踪文件恢复成暂存区(或 HEAD)里的样子,未跟踪文件直接删除。**不可恢复。**
///
/// 参数:`entries` 要放弃的条目(用它们的状态区分已跟踪与未跟踪)。
pub fn discard(dir: &str, entries: &[StatusEntry]) -> Result<(), String> {
    let tracked: Vec<&str> = entries.iter().filter(|e| !e.untracked()).map(|e| e.path.as_str()).collect();
    let untracked: Vec<&str> = entries.iter().filter(|e| e.untracked()).map(|e| e.path.as_str()).collect();
    if !tracked.is_empty() {
        let mut args = vec!["restore", "--worktree", "--"];
        args.extend(tracked);
        git(dir, &args)?;
    }
    if !untracked.is_empty() {
        let mut args = vec!["clean", "-f", "-q", "--"];
        args.extend(untracked);
        git(dir, &args)?;
    }
    Ok(())
}

/// 在目录里初始化仓库(`git init`,默认分支名跟随用户的 git 配置)。
pub fn init(dir: &str) -> Result<(), String> {
    git(dir, &["init", "-q"]).map(|_| ())
}

/// 提交。`amend` 为真时修改上一次提交(消息为空则沿用原消息)。
pub fn commit(dir: &str, message: &str, amend: bool) -> Result<(), String> {
    let msg = message.trim();
    if msg.is_empty() && !amend {
        return Err("请先填写提交消息".into());
    }
    let mut args = vec!["commit", "-q"];
    if amend {
        args.push("--amend");
    }
    if msg.is_empty() {
        args.push("--no-edit");
    } else {
        args.extend(["-m", msg]);
    }
    git(dir, &args).map(|_| ())
}

/// 上一次提交的完整消息(「修改上次提交」时回填输入框)。
pub fn last_commit_message(dir: &str) -> Option<String> {
    git(dir, &["log", "-1", "--format=%B"]).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// 取回远程更新但不合并。
pub fn fetch(dir: &str) -> Result<(), String> {
    git(dir, &["fetch", "--all", "--prune", "-q"]).map(|_| ())
}

/// 拉取并合并上游更新。
pub fn pull(dir: &str) -> Result<(), String> {
    git(dir, &["pull", "-q"]).map(|_| ())
}

/// 推送当前分支;没有上游时发布到第一个远程并设为上游(对应 VS Code 的「发布分支」)。
pub fn push(dir: &str, info: &SyncInfo) -> Result<(), String> {
    if info.upstream.is_some() {
        return git(dir, &["push", "-q"]).map(|_| ());
    }
    let branch = info.branch.as_deref().ok_or("游离 HEAD 无法推送,请先切换到分支")?;
    let remote = git(dir, &["remote"])?.lines().next().map(str::trim).filter(|s| !s.is_empty()).ok_or("没有配置远程仓库")?.to_string();
    git(dir, &["push", "-q", "--set-upstream", &remote, branch]).map(|_| ())
}

/// 同步:先拉取再推送(VS Code 的「同步更改」)。
pub fn sync(dir: &str, info: &SyncInfo) -> Result<(), String> {
    if info.upstream.is_some() {
        pull(dir)?;
    }
    push(dir, info)
}

/// 某文件在暂存区里的内容;暂存区没有(新增未暂存、二进制)时返回 `None`。
pub fn file_at_index(dir: &str, rel: &str) -> Option<String> {
    git(dir, &["show", &format!(":{rel}")]).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{file_at_head, status};
    use std::process::Command;

    #[test]
    fn 解析领先落后() {
        assert_eq!(parse_ahead_behind("2\t5\n"), (5, 2));
        assert_eq!(parse_ahead_behind("0\t0"), (0, 0));
        assert_eq!(parse_ahead_behind(""), (0, 0));
    }

    fn init_repo(tag: &str) -> (std::path::PathBuf, String) {
        let base = std::env::temp_dir().join(format!("loom-scm-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let dir = base.to_string_lossy().to_string();
        run(&dir, &["init", "-q", "-b", "main"]);
        (base, dir)
    }

    fn run(dir: &str, args: &[&str]) {
        let ok = Command::new("git").arg("-C").arg(dir).args(args)
            .env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t")
            .output().unwrap().status.success();
        assert!(ok, "git {args:?} 失败");
    }

    fn entry<'a>(list: &'a [StatusEntry], p: &str) -> &'a StatusEntry {
        list.iter().find(|e| e.path == p).unwrap_or_else(|| panic!("状态里没有 {p}:{list:?}"))
    }

    /// 暂存 → 取消暂存 → 放弃 → 提交 → 修改上次提交的完整流程(真实仓库)。
    #[test]
    fn 暂存取消暂存放弃与提交() {
        let (base, dir) = init_repo("flow");
        // 空仓库:还没有提交时取消暂存要走 rm --cached
        std::fs::write(base.join("a.txt"), "1\n").unwrap();
        assert!(!sync_info(&dir).has_commits);
        stage(&dir, &["a.txt".into()]).unwrap();
        assert_eq!(entry(&status(&dir).unwrap(), "a.txt").staged_kind(), Some('A'));
        unstage(&dir, &["a.txt".into()], false).unwrap();
        assert!(entry(&status(&dir).unwrap(), "a.txt").untracked());

        // 空消息不能提交
        stage_all(&dir).unwrap();
        assert!(commit(&dir, "  ", false).is_err());
        commit(&dir, "首次提交", false).unwrap();
        assert!(sync_info(&dir).has_commits);
        assert_eq!(sync_info(&dir).branch.as_deref(), Some("main"));
        assert!(status(&dir).unwrap().is_empty());

        // 修改 + 新增 + 删除
        std::fs::write(base.join("a.txt"), "2\n").unwrap();
        std::fs::write(base.join("b.txt"), "new\n").unwrap();
        let s = status(&dir).unwrap();
        assert_eq!(entry(&s, "a.txt").unstaged_kind(), Some('M'));
        assert_eq!(entry(&s, "b.txt").unstaged_kind(), Some('U'));
        assert_eq!(entry(&s, "a.txt").staged_kind(), None);

        // 暂存 a 后,再改一次:同一文件同时出现在「暂存」与「更改」
        stage(&dir, &["a.txt".into()]).unwrap();
        std::fs::write(base.join("a.txt"), "3\n").unwrap();
        let s = status(&dir).unwrap();
        let a = entry(&s, "a.txt");
        assert_eq!((a.staged_kind(), a.unstaged_kind()), (Some('M'), Some('M')));
        assert_eq!(file_at_index(&dir, "a.txt").as_deref(), Some("2\n"));
        assert_eq!(file_at_head(&dir, "a.txt").as_deref(), Some("1\n"));

        // 放弃工作区改动:a 回到暂存区的 2,未跟踪的 b 被删除
        discard(&dir, &[a.clone(), entry(&s, "b.txt").clone()]).unwrap();
        assert_eq!(std::fs::read_to_string(base.join("a.txt")).unwrap(), "2\n");
        assert!(!base.join("b.txt").exists());
        let s = status(&dir).unwrap();
        assert_eq!((entry(&s, "a.txt").staged_kind(), entry(&s, "a.txt").unstaged_kind()), (Some('M'), None));

        // 取消暂存全部,再暂存全部
        unstage_all(&dir, true).unwrap();
        assert_eq!(entry(&status(&dir).unwrap(), "a.txt").unstaged_kind(), Some('M'));
        stage_all(&dir).unwrap();

        // 提交与修改上次提交
        commit(&dir, "第二次\n\n正文", false).unwrap();
        assert!(last_commit_message(&dir).unwrap().starts_with("第二次"));
        commit(&dir, "改个标题", true).unwrap();
        assert_eq!(last_commit_message(&dir).as_deref(), Some("改个标题"));
        commit(&dir, "", true).unwrap();
        assert_eq!(last_commit_message(&dir).as_deref(), Some("改个标题"), "空消息的 amend 沿用原消息");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn 删除与重命名的暂存状态() {
        let (base, dir) = init_repo("rename");
        std::fs::write(base.join("old.txt"), "content content content\n").unwrap();
        std::fs::write(base.join("gone.txt"), "x\n").unwrap();
        stage_all(&dir).unwrap();
        commit(&dir, "init", false).unwrap();
        std::fs::rename(base.join("old.txt"), base.join("new.txt")).unwrap();
        std::fs::remove_file(base.join("gone.txt")).unwrap();
        stage_all(&dir).unwrap();
        let s = status(&dir).unwrap();
        assert_eq!(entry(&s, "gone.txt").staged_kind(), Some('D'));
        let r = entry(&s, "new.txt");
        assert_eq!((r.staged_kind(), r.orig.as_deref()), (Some('R'), Some("old.txt")));
        let _ = std::fs::remove_dir_all(base);
    }

    /// 推送 / 拉取 / 同步:用本地裸仓库当远程。
    #[test]
    fn 推送拉取与领先落后() {
        let (base, dir) = init_repo("sync");
        let remote = base.join("remote.git");
        std::fs::create_dir_all(&remote).unwrap();
        run(&remote.to_string_lossy(), &["init", "-q", "--bare", "-b", "main"]);
        std::fs::write(base.join("a.txt"), "1\n").unwrap();
        stage_all(&dir).unwrap();
        commit(&dir, "init", false).unwrap();
        assert!(!sync_info(&dir).has_remote);
        assert!(push(&dir, &sync_info(&dir)).is_err(), "没有远程要报错");

        run(&dir, &["remote", "add", "origin", &remote.to_string_lossy()]);
        let info = sync_info(&dir);
        assert!(info.has_remote && info.upstream.is_none());
        push(&dir, &info).unwrap();
        let info = sync_info(&dir);
        assert_eq!((info.upstream.as_deref(), info.ahead, info.behind), (Some("origin/main"), 0, 0));

        std::fs::write(base.join("a.txt"), "2\n").unwrap();
        stage_all(&dir).unwrap();
        commit(&dir, "again", false).unwrap();
        let info = sync_info(&dir);
        assert_eq!((info.ahead, info.behind), (1, 0));
        sync(&dir, &info).unwrap();
        assert_eq!(sync_info(&dir).ahead, 0);

        // 远程多出一个提交:本地落后 1
        let other = base.join("other");
        run(&base.to_string_lossy(), &["clone", "-q", &remote.to_string_lossy(), "other"]);
        let other_dir = other.to_string_lossy().to_string();
        std::fs::write(other.join("c.txt"), "c\n").unwrap();
        stage_all(&other_dir).unwrap();
        commit(&other_dir, "remote change", false).unwrap();
        run(&other_dir, &["push", "-q"]);
        fetch(&dir).unwrap();
        assert_eq!(sync_info(&dir).behind, 1);
        pull(&dir).unwrap();
        assert!(base.join("c.txt").exists());
        let _ = std::fs::remove_dir_all(base);
    }
}
