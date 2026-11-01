//! 提交历史与 Git 图谱:读取 `git log`、把提交列表排成「泳道」行(画分支 / 合并线)、
//! 读取某次提交改动的文件,以及储藏(stash)。
//!
//! 泳道布局是纯函数(`layout`),与界面无关,算法思路同 VS Code 的 Git Graph / OhMyAgent 的 `GitGraph`:
//! 每条泳道记录「下一个要到达的提交」,沿首个父提交延续同色,合并时多出的父提交另开泳道。
//! 全部走 `git` 命令行,会起子进程,调用方应放到后台线程。
//!
//! @author ixxxxoooo

use crate::git::git;

/// 一次提交。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Commit {
    pub hash: String,
    /// 父提交(合并提交有多个)
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    /// 作者时间(Unix 秒)
    pub time: i64,
    /// 引用标签:分支、远程分支、tag、HEAD(已拆成条目,如 `HEAD -> main`、`origin/main`、`tag: v1`)
    pub refs: Vec<String>,
    pub subject: String,
    pub body: String,
}

impl Commit {
    /// 短哈希(7 位)。
    pub fn short(&self) -> &str {
        &self.hash[..self.hash.len().min(7)]
    }
}

/// 一条泳道(某一列上等待出现的下一个提交)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lane {
    /// 这条线下一个要到达的提交
    pub hash: String,
    /// 颜色序号(界面对调色板取模)
    pub color: usize,
}

/// 图中的一行(对应一个提交)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRow {
    pub commit: Commit,
    /// 提交点所在列
    pub column: usize,
    /// 提交点颜色序号
    pub color: usize,
    /// 行顶部各列的泳道(`None` 表示该列空)
    pub top: Vec<Option<Lane>>,
    /// 行底部各列的泳道
    pub bottom: Vec<Option<Lane>>,
    /// 汇入该提交的列(含 `column` 自身)
    pub incoming: Vec<usize>,
    /// 从该提交向下分出的列(各父提交所在的底部列),与 `commit.parents` 一一对应
    pub outgoing: Vec<usize>,
}

impl GraphRow {
    /// 本行需要的列数。
    pub fn lane_count(&self) -> usize {
        self.top.len().max(self.bottom.len()).max(self.column + 1)
    }
}

/// 解析 `git log` 的输出(记录以 `\x1e` 开头、字段以 `\x1f` 分隔,见 [`LOG_FORMAT`])。
pub fn parse_log(out: &str) -> Vec<Commit> {
    out.split('\x1e')
        .filter(|r| !r.trim().is_empty())
        .filter_map(|rec| {
            let f: Vec<&str> = rec.trim_start_matches('\n').splitn(8, '\x1f').collect();
            if f.len() < 7 || f[0].trim().is_empty() {
                return None;
            }
            Some(Commit {
                hash: f[0].trim().to_string(),
                parents: f[1].split_whitespace().map(String::from).collect(),
                author: f[2].to_string(),
                email: f[3].to_string(),
                time: f[4].trim().parse().unwrap_or(0),
                refs: f[5].split(", ").map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect(),
                subject: f[6].to_string(),
                body: f.get(7).map(|b| b.trim().to_string()).unwrap_or_default(),
            })
        })
        .collect()
}

/// `git log --format` 的格式串。
pub const LOG_FORMAT: &str = "--format=%x1e%H%x1f%P%x1f%an%x1f%ae%x1f%at%x1f%D%x1f%s%x1f%b";

/// 读取提交历史。
///
/// 参数:`limit` 条数;`path` 只看某文件(仓库内相对路径,`--follow`);`all` 为真看所有分支(含远程与 tag)。
/// 按时间排序(`--date-order`,保证子提交总在父提交之前,泳道布局依赖这点)。
pub fn log(dir: &str, limit: usize, path: Option<&str>, all: bool) -> Result<Vec<Commit>, String> {
    let n = format!("-n{limit}");
    let mut args = vec!["--no-optional-locks", "log", "--date-order", "--no-color", LOG_FORMAT, n.as_str()];
    if all {
        args.push("--all");
    }
    if let Some(p) = path {
        args.extend(["--follow", "--", p]);
    }
    // 空仓库(还没有提交)时 git 报错,按空历史处理
    match git(dir, &args) {
        Ok(out) => Ok(parse_log(&out)),
        Err(e) if e.contains("does not have any commits") || e.contains("bad default revision") => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// 计算图布局。`commits` 为新 → 旧排列。返回与输入等长的行。
pub fn layout(commits: &[Commit]) -> Vec<GraphRow> {
    let mut lanes: Vec<Option<Lane>> = Vec::new();
    let mut next_color = 0usize;
    let mut rows = Vec::with_capacity(commits.len());
    fn first_free(lanes: &[Option<Lane>]) -> usize {
        lanes.iter().position(Option::is_none).unwrap_or(lanes.len())
    }
    for commit in commits {
        let top = lanes.clone();
        let incoming: Vec<usize> = lanes.iter().enumerate().filter(|(_, l)| l.as_ref().is_some_and(|l| l.hash == commit.hash)).map(|(i, _)| i).collect();
        let (column, color) = match incoming.first() {
            Some(&first) => (first, lanes[first].as_ref().map_or(0, |l| l.color)),
            None => {
                // 没有泳道指向它(分支末端的最新提交):占第一个空位并分配新颜色
                let column = first_free(&lanes);
                let color = next_color;
                next_color += 1;
                if column == lanes.len() {
                    lanes.push(None);
                }
                (column, color)
            }
        };
        for &i in &incoming {
            lanes[i] = None;
        }
        let mut outgoing = Vec::new();
        for (offset, parent) in commit.parents.iter().enumerate() {
            if let Some(existing) = lanes.iter().position(|l| l.as_ref().is_some_and(|l| &l.hash == parent)) {
                // 父提交已在某条泳道里等待:连过去即可
                outgoing.push(existing);
                continue;
            }
            let (slot, lane_color) = if offset == 0 {
                // 首父沿用本提交的列与颜色
                (if lanes.get(column).is_some_and(Option::is_none) { column } else { first_free(&lanes) }, color)
            } else {
                let c = next_color;
                next_color += 1;
                (first_free(&lanes), c)
            };
            if slot == lanes.len() {
                lanes.push(None);
            }
            lanes[slot] = Some(Lane { hash: parent.clone(), color: lane_color });
            outgoing.push(slot);
        }
        while lanes.last().is_some_and(Option::is_none) {
            lanes.pop();
        }
        rows.push(GraphRow { commit: commit.clone(), column, color, top, bottom: lanes.clone(), incoming, outgoing });
    }
    rows
}

/// 某次提交改动的一个文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitFile {
    /// 状态字母:A 新增 / M 修改 / D 删除 / R 重命名 / C 复制 / T 类型变更
    pub status: char,
    pub path: String,
    /// 重命名 / 复制时的原路径
    pub orig: Option<String>,
}

/// 解析 `git show --name-status -z` 的文件部分:`状态\0路径\0`(重命名为 `R100\0旧\0新\0`)。
pub fn parse_name_status_z(out: &str) -> Vec<CommitFile> {
    let mut it = out.split('\0').filter(|s| !s.is_empty());
    let mut files = Vec::new();
    while let Some(st) = it.next() {
        let status = st.chars().next().unwrap_or('M');
        if matches!(status, 'R' | 'C') {
            let (Some(old), Some(new)) = (it.next(), it.next()) else { break };
            files.push(CommitFile { status, path: new.to_string(), orig: Some(old.to_string()) });
        } else {
            let Some(path) = it.next() else { break };
            files.push(CommitFile { status, path: path.to_string(), orig: None });
        }
    }
    files
}

/// 读取一次提交改动的文件列表(合并提交相对第一个父提交)。
pub fn commit_files(dir: &str, hash: &str) -> Result<Vec<CommitFile>, String> {
    let out = git(dir, &["--no-optional-locks", "diff-tree", "--no-commit-id", "--name-status", "-r", "-z", "-M", "--root", "-m", "--first-parent", hash])?;
    Ok(parse_name_status_z(&out))
}

/// 读取某个提交里某文件的内容;不存在(新增前 / 已删除)返回 `None`。
pub fn file_at_rev(dir: &str, rev: &str, rel: &str) -> Option<String> {
    git(dir, &["--no-optional-locks", "show", &format!("{rev}:{rel}")]).ok()
}

/// 储藏条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stash {
    /// 序号(`stash@{n}` 里的 n)
    pub index: usize,
    /// 说明(如 `On main: 临时保存`)
    pub message: String,
}

/// 解析 `git stash list --format=%gd%x1f%gs`。
pub fn parse_stashes(out: &str) -> Vec<Stash> {
    out.lines()
        .filter_map(|l| {
            let (name, msg) = l.split_once('\x1f')?;
            let index = name.trim().strip_prefix("stash@{")?.strip_suffix('}')?.parse().ok()?;
            Some(Stash { index, message: msg.trim().to_string() })
        })
        .collect()
}

/// 储藏列表。
pub fn stashes(dir: &str) -> Vec<Stash> {
    git(dir, &["--no-optional-locks", "stash", "list", "--format=%gd%x1f%gs"]).map(|o| parse_stashes(&o)).unwrap_or_default()
}

/// 储藏当前改动(含未跟踪文件)。`message` 为空时用 git 默认说明。
pub fn stash_push(dir: &str, message: &str) -> Result<(), String> {
    let mut args = vec!["stash", "push", "--include-untracked"];
    if !message.trim().is_empty() {
        args.extend(["-m", message.trim()]);
    }
    git(dir, &args).map(|_| ())
}

/// 应用并删除某条储藏。
pub fn stash_pop(dir: &str, index: usize) -> Result<(), String> {
    git(dir, &["stash", "pop", &format!("stash@{{{index}}}")]).map(|_| ())
}

/// 应用某条储藏但保留它。
pub fn stash_apply(dir: &str, index: usize) -> Result<(), String> {
    git(dir, &["stash", "apply", &format!("stash@{{{index}}}")]).map(|_| ())
}

/// 删除某条储藏。
pub fn stash_drop(dir: &str, index: usize) -> Result<(), String> {
    git(dir, &["stash", "drop", &format!("stash@{{{index}}}")]).map(|_| ())
}

/// 引用签名:HEAD 与所有分支 / 远程分支 / tag 的哈希摘要。图谱据此判断「历史有没有变」,不必每次都重读整段日志。
pub fn refs_signature(dir: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    git(dir, &["--no-optional-locks", "rev-parse", "-q", "--verify", "HEAD"]).unwrap_or_default().hash(&mut h);
    git(dir, &["--no-optional-locks", "for-each-ref", "--format=%(objectname) %(refname)"]).unwrap_or_default().hash(&mut h);
    h.finish()
}

/// 把 Unix 秒格式化成「3 分钟前 / 2 天前 / 2024-05-01」这样的相对时间。`now` 为当前 Unix 秒。
pub fn relative_time(time: i64, now: i64) -> String {
    let d = (now - time).max(0);
    match d {
        0..=59 => "刚刚".into(),
        60..=3599 => format!("{} 分钟前", d / 60),
        3600..=86399 => format!("{} 小时前", d / 3600),
        86400..=2_591_999 => format!("{} 天前", d / 86400),
        2_592_000..=31_535_999 => format!("{} 个月前", d / 2_592_000),
        _ => format!("{} 年前", d / 31_536_000),
    }
}

/// 把 Unix 秒格式化成 `YYYY-MM-DD HH:MM`(UTC 偏移由 `tz_offset_secs` 给出,避免调用 libc 时间函数)。
pub fn format_datetime(time: i64, tz_offset_secs: i64) -> String {
    let t = time + tz_offset_secs;
    let days = t.div_euclid(86400);
    let rem = t.rem_euclid(86400);
    // 公历换算(Howard Hinnant 的 civil_from_days)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", rem / 3600, rem % 3600 / 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn c(hash: &str, parents: &[&str]) -> Commit {
        Commit { hash: hash.into(), parents: parents.iter().map(|s| s.to_string()).collect(), ..Default::default() }
    }

    #[test]
    fn 线性历史只占一列() {
        let rows = layout(&[c("c", &["b"]), c("b", &["a"]), c("a", &[])]);
        assert!(rows.iter().all(|r| r.column == 0 && r.color == rows[0].color));
        assert_eq!(rows[0].outgoing, vec![0]);
        assert!(rows[2].outgoing.is_empty());
        assert!(rows[2].bottom.is_empty());
    }

    #[test]
    fn 合并提交分出第二条泳道并在汇合点收拢() {
        // m 合并 b 与 d;b、d 的父提交都是 a
        let rows = layout(&[c("m", &["b", "d"]), c("d", &["a"]), c("b", &["a"]), c("a", &[])]);
        assert_eq!(rows[0].outgoing, vec![0, 1], "合并提交向两个父提交各连一条线");
        assert_eq!(rows[1].column, 1, "第二个父提交在新泳道");
        assert_ne!(rows[1].color, rows[0].color);
        // b 的线向右并入 d 那条泳道(a 已在第 1 列等待),a 落在第 1 列
        assert_eq!(rows[2].outgoing, vec![1]);
        assert_eq!(rows[3].column, 1);
        assert!(rows[3].bottom.is_empty());
    }

    #[test]
    fn 分支末端提交占第一个空位() {
        // 两个分支头 x、y 各自指向 a
        let rows = layout(&[c("x", &["a"]), c("y", &["a"]), c("a", &[])]);
        assert_eq!(rows[0].column, 0);
        assert_eq!(rows[1].column, 1);
        // y 的线在自己这一行并入第 0 列的泳道,a 只剩一条汇入线
        assert_eq!(rows[1].outgoing, vec![0]);
        assert_eq!(rows[2].incoming, vec![0]);
    }

    #[test]
    fn 解析日志含引用与正文() {
        let out = "\x1eaaa\x1fbbb ccc\x1f张三\x1fz@x.com\x1f1700000000\x1fHEAD -> main, origin/main, tag: v1\x1ffeat: 标题\x1f第一行\n第二行\n\x1ebbb\x1f\x1f李四\x1fl@x.com\x1f1600000000\x1f\x1finit\x1f\n";
        let cs = parse_log(out);
        assert_eq!(cs.len(), 2);
        assert_eq!(cs[0].parents, vec!["bbb", "ccc"]);
        assert_eq!(cs[0].refs, vec!["HEAD -> main", "origin/main", "tag: v1"]);
        assert_eq!(cs[0].subject, "feat: 标题");
        assert_eq!(cs[0].body, "第一行\n第二行");
        assert!(cs[1].parents.is_empty() && cs[1].refs.is_empty());
        assert_eq!(cs[1].short(), "bbb");
    }

    #[test]
    fn 解析文件状态含重命名() {
        let files = parse_name_status_z("M\0a.rs\0R100\0old.rs\0new.rs\0D\0gone.rs\0");
        assert_eq!(files.len(), 3);
        assert_eq!(files[1], CommitFile { status: 'R', path: "new.rs".into(), orig: Some("old.rs".into()) });
        assert_eq!(files[2].status, 'D');
    }

    #[test]
    fn 解析储藏() {
        let s = parse_stashes("stash@{0}\x1fOn main: 临时\nstash@{1}\x1fWIP on main: abc\n");
        assert_eq!(s, vec![Stash { index: 0, message: "On main: 临时".into() }, Stash { index: 1, message: "WIP on main: abc".into() }]);
    }

    #[test]
    fn 时间格式() {
        assert_eq!(relative_time(100, 130), "刚刚");
        assert_eq!(relative_time(0, 7200), "2 小时前");
        assert_eq!(relative_time(0, 86400 * 3), "3 天前");
        assert_eq!(format_datetime(0, 0), "1970-01-01 00:00");
        assert_eq!(format_datetime(1_700_000_000, 8 * 3600), "2023-11-15 06:13");
    }

    fn sh(dir: &std::path::Path, args: &[&str]) {
        let ok = Command::new("git").arg("-C").arg(dir).args(args).env("GIT_AUTHOR_NAME", "T").env("GIT_AUTHOR_EMAIL", "t@t").env("GIT_COMMITTER_NAME", "T").env("GIT_COMMITTER_EMAIL", "t@t").output().unwrap().status.success();
        assert!(ok, "git {args:?} 失败");
    }

    #[test]
    fn 真实仓库的历史_图谱_文件与储藏() {
        let dir = std::env::temp_dir().join(format!("loom-gitlog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let d = dir.to_str().unwrap();
        sh(&dir, &["init", "-q", "-b", "main"]);
        assert_eq!(log(d, 10, None, true).unwrap().len(), 0, "空仓库返回空历史");
        std::fs::write(dir.join("a.txt"), "1\n").unwrap();
        sh(&dir, &["add", "."]);
        sh(&dir, &["commit", "-q", "-m", "feat: 第一次"]);
        sh(&dir, &["checkout", "-q", "-b", "dev"]);
        std::fs::write(dir.join("b.txt"), "2\n").unwrap();
        sh(&dir, &["add", "."]);
        sh(&dir, &["commit", "-q", "-m", "feat: dev 提交"]);
        sh(&dir, &["checkout", "-q", "main"]);
        std::fs::write(dir.join("a.txt"), "1\n3\n").unwrap();
        sh(&dir, &["commit", "-q", "-am", "fix: main 提交"]);
        sh(&dir, &["merge", "-q", "--no-ff", "dev", "-m", "merge: 合并 dev"]);
        let commits = log(d, 10, None, true).unwrap();
        assert_eq!(commits.len(), 4);
        assert_eq!(commits[0].subject, "merge: 合并 dev");
        assert_eq!(commits[0].parents.len(), 2);
        assert!(commits[0].refs.iter().any(|r| r.contains("main")));
        let rows = layout(&commits);
        assert_eq!(rows.iter().map(|r| r.column).max(), Some(1), "合并会用到第二条泳道");
        // 文件历史
        let only_a = log(d, 10, Some("a.txt"), false).unwrap();
        assert_eq!(only_a.len(), 2);
        // 提交的文件
        let files = commit_files(d, &commits[2].hash).unwrap();
        assert!(!files.is_empty());
        let first = commits.last().unwrap();
        assert_eq!(commit_files(d, &first.hash).unwrap(), vec![CommitFile { status: 'A', path: "a.txt".into(), orig: None }], "根提交也能列出文件");
        assert_eq!(file_at_rev(d, &first.hash, "a.txt").as_deref(), Some("1\n"));
        assert_eq!(file_at_rev(d, &first.hash, "b.txt"), None);
        // 储藏
        std::fs::write(dir.join("a.txt"), "改动\n").unwrap();
        std::fs::write(dir.join("new.txt"), "x\n").unwrap();
        stash_push(d, "临时保存").unwrap();
        assert!(!dir.join("new.txt").exists(), "未跟踪文件也被储藏");
        let list = stashes(d);
        assert_eq!(list.len(), 1);
        assert!(list[0].message.contains("临时保存"));
        stash_pop(d, 0).unwrap();
        assert!(dir.join("new.txt").exists());
        assert!(stashes(d).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
