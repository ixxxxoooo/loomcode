//! 真实模式的数据来源:Claude 历史会话、Git 分支、时间格式化。
//!
//! 这些函数会读磁盘 / 起子进程,调用方应放到后台线程里执行。

use std::path::Path;
use std::process::Command;
use std::time::SystemTime;
use ucode_claude::history;
use ucode_core::*;

/// 当前 Git 分支;不是仓库(或没装 git)返回 `None`。游离 HEAD 时给出短哈希。
pub fn git_branch(dir: &str) -> Option<String> {
    let out = Command::new("git").args(["-C", dir, "rev-parse", "--abbrev-ref", "HEAD"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if name == "HEAD" {
        let out = Command::new("git").args(["-C", dir, "rev-parse", "--short", "HEAD"]).output().ok()?;
        return Some(String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty());
    }
    Some(name).filter(|s| !s.is_empty())
}

/// 把修改时间描述成「刚刚 / 5 分钟前 / 昨天」。
pub fn time_ago(t: SystemTime) -> String {
    let secs = SystemTime::now().duration_since(t).map(|d| d.as_secs()).unwrap_or(0);
    match secs {
        0..=59 => "刚刚".into(),
        60..=3599 => format!("{} 分钟前", secs / 60),
        3600..=86_399 => format!("{} 小时前", secs / 3600),
        86_400..=172_799 => "昨天".into(),
        _ => format!("{} 天前", secs / 86_400),
    }
}

fn to_meta(s: &history::SessionMeta) -> SessionMeta {
    SessionMeta { id: s.id.clone(), title: s.title.clone(), updated: time_ago(s.modified), pinned: false, badge: SessionBadge::Idle }
}

/// 读取全部项目:先是有历史会话的(最近活动在前),再补上「最近项目」里还没有会话的目录。
pub fn load_projects(recent: &[String]) -> Vec<Project> {
    let mut projects: Vec<Project> = history::list_projects_with_sessions(&history::projects_dir())
        .into_iter()
        .filter(|(p, _)| Path::new(&p.cwd).is_dir())
        .map(|(p, sessions)| Project {
            name: project_name_from_path(&p.cwd),
            branch: git_branch(&p.cwd),
            sessions: sessions.iter().map(to_meta).collect(),
            path: p.cwd,
            expanded: false,
        })
        .collect();
    for path in recent {
        if Path::new(path).is_dir() && !projects.iter().any(|p| &p.path == path) {
            projects.push(Project { name: project_name_from_path(path), branch: git_branch(path), sessions: Vec::new(), path: path.clone(), expanded: false });
        }
    }
    if let Some(first) = projects.first_mut() {
        first.expanded = true;
    }
    projects
}

/// 重新读取单个项目的会话列表与分支。
pub fn load_project_sessions(path: &str) -> (Vec<SessionMeta>, Option<String>) {
    let dir = history::projects_dir().join(history::encode_project_dir(path));
    (history::list_sessions_in(&dir).iter().map(to_meta).collect(), git_branch(path))
}

/// 回放一个历史会话,得到可以直接喂给状态归约器的事件。
pub fn replay(cwd: &str, session_id: &str) -> Vec<AgentEvent> {
    let file = history::projects_dir().join(history::encode_project_dir(cwd)).join(format!("{session_id}.jsonl"));
    history::load_session(&file, cwd).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn 时间描述() {
        let ago = |s| SystemTime::now() - Duration::from_secs(s);
        assert_eq!(time_ago(ago(5)), "刚刚");
        assert_eq!(time_ago(ago(300)), "5 分钟前");
        assert_eq!(time_ago(ago(7200)), "2 小时前");
        assert_eq!(time_ago(ago(90_000)), "昨天");
        assert_eq!(time_ago(ago(86_400 * 5)), "5 天前");
    }

    #[test]
    fn 非仓库没有分支() {
        assert_eq!(git_branch("/不存在的目录"), None);
    }
}
