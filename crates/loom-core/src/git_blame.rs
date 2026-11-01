//! 逐行追溯(git blame):解析 `git blame --porcelain`,供 IDE 的行内追溯、侧栏追溯与悬浮卡片使用。
//!
//! 支持传入「尚未保存的编辑器内容」(`--contents -`),这样改动中的行显示为「未提交」而不是错位的旧提交。
//! 会起子进程,请在后台线程调用。
//!
//! @author ixxxxoooo

use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};

/// 未提交行使用的全零哈希。
pub const ZERO_HASH: &str = "0000000000000000000000000000000000000000";

/// 一次提交的追溯信息(同一提交的所有行共享)。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BlameCommit {
    pub hash: String,
    pub author: String,
    pub email: String,
    /// 作者时间(Unix 秒)
    pub time: i64,
    pub summary: String,
}

impl BlameCommit {
    /// 是否是尚未提交的改动。
    pub fn uncommitted(&self) -> bool {
        self.hash == ZERO_HASH
    }

    /// 短哈希(7 位)。
    pub fn short(&self) -> &str {
        &self.hash[..self.hash.len().min(7)]
    }
}

/// 一行的追溯结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameLine {
    /// 当前文件里的行号(从 1 开始)
    pub line: usize,
    /// 这一行最初出现时在源提交里的行号
    pub orig_line: usize,
    pub commit: std::sync::Arc<BlameCommit>,
}

/// 解析 `git blame --porcelain` 的输出,返回按行号排列的结果(行号 1..=n)。
pub fn parse_porcelain(out: &str) -> Vec<BlameLine> {
    let mut commits: HashMap<String, std::sync::Arc<BlameCommit>> = HashMap::new();
    let mut pending: HashMap<String, BlameCommit> = HashMap::new();
    let mut lines: Vec<BlameLine> = Vec::new();
    // 当前头部:(哈希, 原行号, 最终行号)
    let mut cur: Option<(String, usize, usize)> = None;
    for raw in out.lines() {
        if let Some(_text) = raw.strip_prefix('\t') {
            // 内容行:收尾当前头部
            if let Some((hash, orig, fin)) = cur.take() {
                let commit = commits
                    .entry(hash.clone())
                    .or_insert_with(|| std::sync::Arc::new(pending.remove(&hash).unwrap_or(BlameCommit { hash: hash.clone(), ..Default::default() })))
                    .clone();
                lines.push(BlameLine { line: fin, orig_line: orig, commit });
            }
            continue;
        }
        if cur.is_none() {
            // 头部:`<40 位哈希> <原行号> <最终行号> [<组内行数>]`
            let mut it = raw.split(' ');
            let (Some(h), Some(o), Some(f)) = (it.next(), it.next(), it.next()) else { continue };
            if h.len() >= 40 && h.chars().all(|c| c.is_ascii_hexdigit()) {
                cur = Some((h.to_string(), o.parse().unwrap_or(0), f.parse().unwrap_or(0)));
                pending.entry(h.to_string()).or_insert_with(|| BlameCommit { hash: h.to_string(), ..Default::default() });
            }
            continue;
        }
        // 提交元数据(只在该提交第一次出现时给出)
        let Some((hash, _, _)) = &cur else { continue };
        let Some(c) = pending.get_mut(hash) else { continue };
        if let Some(v) = raw.strip_prefix("author ") {
            c.author = v.to_string();
        } else if let Some(v) = raw.strip_prefix("author-mail ") {
            c.email = v.trim_matches(|ch| ch == '<' || ch == '>').to_string();
        } else if let Some(v) = raw.strip_prefix("author-time ") {
            c.time = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = raw.strip_prefix("summary ") {
            c.summary = v.to_string();
        }
    }
    lines.sort_by_key(|l| l.line);
    lines
}

/// 对一个文件做逐行追溯。
///
/// 参数:`dir` 仓库内的任一目录(通常是文件所在目录);`rel` 相对 `dir` 的路径(通常就是文件名);`contents` 为 `Some` 时用它(编辑器里未保存的内容)代替工作区文件。
/// 文件未被跟踪、不在仓库里、或是空文件时返回错误 / 空。
pub fn blame(dir: &str, rel: &str, contents: Option<&str>) -> Result<Vec<BlameLine>, String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(dir).args(["--no-optional-locks", "blame", "--porcelain", "-w"]);
    if contents.is_some() {
        cmd.args(["--contents", "-"]);
    }
    cmd.args(["--", rel]).env("GIT_TERMINAL_PROMPT", "0").stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(if contents.is_some() { Stdio::piped() } else { Stdio::null() });
    let mut child = cmd.spawn().map_err(|e| format!("无法运行 git:{e}"))?;
    if let (Some(text), Some(mut stdin)) = (contents, child.stdin.take()) {
        // 在另一个线程写入,避免内容很大时与 git 的输出互相等待造成死锁
        let data = text.as_bytes().to_vec();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&data);
        });
    }
    let out = child.wait_with_output().map_err(|e| format!("等待 git 失败:{e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(err.lines().find(|l| !l.trim().is_empty()).unwrap_or("git blame 失败").trim().to_string());
    }
    Ok(parse_porcelain(&String::from_utf8_lossy(&out.stdout)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa 1 1 2
author 张三
author-mail <z@x.com>
author-time 1700000000
author-tz +0800
committer 张三
summary feat: 初始
filename a.rs
\tfn main() {
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa 2 2
\t    let x = 1;
0000000000000000000000000000000000000000 3 3 1
author Not Committed Yet
author-mail <not.committed.yet>
author-time 1700001000
summary Version of a.rs from a.rs
filename a.rs
\t}
";

    #[test]
    fn 解析同一提交的连续行共享元数据() {
        let lines = parse_porcelain(SAMPLE);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].commit.author, "张三");
        assert_eq!(lines[0].commit.email, "z@x.com");
        assert_eq!(lines[0].commit.summary, "feat: 初始");
        assert!(std::sync::Arc::ptr_eq(&lines[0].commit, &lines[1].commit), "同一提交共享");
        assert_eq!(lines[1].commit.author, "张三", "后续行没有重复元数据也能取到");
        assert!(lines[2].commit.uncommitted());
        assert_eq!(lines[2].commit.short(), "0000000");
    }

    #[test]
    fn 真实仓库追溯_含未保存内容() {
        let dir = std::env::temp_dir().join(format!("loom-blame-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let d = dir.to_str().unwrap();
        let sh = |args: &[&str]| {
            assert!(Command::new("git").arg("-C").arg(d).args(args).env("GIT_AUTHOR_NAME", "甲").env("GIT_AUTHOR_EMAIL", "a@a").env("GIT_COMMITTER_NAME", "甲").env("GIT_COMMITTER_EMAIL", "a@a").output().unwrap().status.success());
        };
        sh(&["init", "-q"]);
        std::fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
        sh(&["add", "."]);
        sh(&["commit", "-q", "-m", "第一次"]);
        let b = blame(d, "a.txt", None).unwrap();
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].commit.summary, "第一次");
        assert_eq!(b[0].commit.author, "甲");
        // 编辑器里改了第二行、新增第三行(尚未保存)
        let b = blame(d, "a.txt", Some("one\nTWO\nthree\n")).unwrap();
        assert_eq!(b.len(), 3);
        assert!(!b[0].commit.uncommitted());
        assert!(b[1].commit.uncommitted() && b[2].commit.uncommitted());
        assert!(blame(d, "不存在.txt", None).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
