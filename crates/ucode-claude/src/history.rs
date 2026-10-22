//! 历史会话:列出 `~/.claude/projects` 下的会话,并把 jsonl 回放成事件。

use crate::translate::Translator;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use ucode_core::AgentEvent;

/// 一个历史会话的概要。
#[derive(Debug, Clone)]
pub struct SessionMeta {
    pub id: String,
    pub path: PathBuf,
    pub cwd: String,
    pub title: String,
    pub modified: SystemTime,
}

/// 一个项目目录的概要。
#[derive(Debug, Clone)]
pub struct ProjectMeta {
    pub dir: PathBuf,
    /// 项目的真实工作目录(取自会话内容,目录名的编码是有损的)
    pub cwd: String,
    pub session_count: usize,
    pub modified: SystemTime,
}

/// claude 的配置根目录:`$CLAUDE_CONFIG_DIR` 或 `~/.claude`。
pub fn config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("CLAUDE_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".claude")
}

/// 默认的 projects 目录。
pub fn projects_dir() -> PathBuf {
    config_dir().join("projects")
}

/// 工作目录 → projects 下的目录名(非字母数字一律替换为 `-`)。
pub fn encode_project_dir(cwd: &str) -> String {
    cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

fn jsonl_files(dir: &Path) -> Vec<(PathBuf, SystemTime)> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    rd.filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
        .filter_map(|e| Some((e.path(), e.metadata().ok()?.modified().ok()?)))
        .collect()
}

/// 从会话文件里提取概要:标题优先用 `ai-title`,其次最后一条提示,再次第一条用户输入。
fn read_meta(path: &Path, modified: SystemTime) -> Option<SessionMeta> {
    let text = std::fs::read_to_string(path).ok()?;
    let id = path.file_stem()?.to_string_lossy().into_owned();
    let (mut cwd, mut ai_title, mut last_prompt, mut first_user) = (String::new(), None, None, None);
    for line in text.lines() {
        // 只解析可能有用的行,避免对巨大的会话文件做全量 JSON 解析
        let interesting = line.starts_with(r#"{"type":"ai-title""#)
            || line.starts_with(r#"{"type":"last-prompt""#)
            || (cwd.is_empty() && line.contains(r#""cwd":""#))
            || (first_user.is_none() && line.starts_with(r#"{"type":"user""#));
        if !interesting {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if cwd.is_empty()
            && let Some(c) = v.get("cwd").and_then(Value::as_str)
        {
            cwd = c.into();
        }
        match v.get("type").and_then(Value::as_str) {
            Some("ai-title") => ai_title = v.get("aiTitle").and_then(Value::as_str).map(String::from),
            Some("last-prompt") => last_prompt = v.get("lastPrompt").and_then(Value::as_str).map(String::from),
            Some("user") if first_user.is_none() => {
                first_user = user_text(&v).filter(|t| !t.starts_with('<') && !t.starts_with("Caveat:"));
            }
            _ => {}
        }
    }
    // 没有任何用户输入的会话(只有元数据)不展示
    let title = ai_title.or(first_user).or(last_prompt)?;
    let title: String = title.lines().next().unwrap_or_default().chars().take(60).collect();
    Some(SessionMeta { id, path: path.into(), cwd, title, modified })
}

fn user_text(v: &Value) -> Option<String> {
    let content = &v["message"]["content"];
    match content {
        Value::String(s) => Some(s.clone()),
        Value::Array(a) => a
            .iter()
            .find(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .and_then(|b| b.get("text").and_then(Value::as_str))
            .map(String::from),
        _ => None,
    }
}

/// 列出某个工作目录下的历史会话,最近修改的在前。
pub fn list_sessions(projects: &Path, cwd: &str) -> Vec<SessionMeta> {
    list_sessions_in(&projects.join(encode_project_dir(cwd)))
}

/// 列出某个 projects 子目录里的会话。
pub fn list_sessions_in(dir: &Path) -> Vec<SessionMeta> {
    let mut metas: Vec<SessionMeta> = jsonl_files(dir).into_iter().filter_map(|(p, m)| read_meta(&p, m)).collect();
    metas.sort_by(|a, b| b.modified.cmp(&a.modified));
    metas
}

/// 列出所有项目,最近有活动的在前。没有任何有效会话的目录会被忽略。
pub fn list_projects(projects: &Path) -> Vec<ProjectMeta> {
    list_projects_with_sessions(projects).into_iter().map(|(p, _)| p).collect()
}

/// 列出所有项目及其会话(只扫描一遍磁盘),项目与会话都按最近活动倒序。
pub fn list_projects_with_sessions(projects: &Path) -> Vec<(ProjectMeta, Vec<SessionMeta>)> {
    let Ok(rd) = std::fs::read_dir(projects) else { return Vec::new() };
    let mut out: Vec<(ProjectMeta, Vec<SessionMeta>)> = rd
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let sessions = list_sessions_in(&e.path());
            let newest = sessions.first()?;
            let cwd = sessions.iter().map(|s| s.cwd.clone()).find(|c| !c.is_empty())?;
            let meta = ProjectMeta { dir: e.path(), cwd, session_count: sessions.len(), modified: newest.modified };
            Some((meta, sessions))
        })
        .collect();
    out.sort_by(|a, b| b.0.modified.cmp(&a.0.modified));
    out
}

/// 回放一个会话文件,得到可直接喂给 `SessionState::apply` 的事件序列。
pub fn load_session(path: &Path, cwd: &str) -> std::io::Result<Vec<AgentEvent>> {
    let text = std::fs::read_to_string(path)?;
    Ok(replay_lines(text.lines(), cwd))
}

/// 回放若干行 jsonl。
pub fn replay_lines<'a>(lines: impl Iterator<Item = &'a str>, cwd: &str) -> Vec<AgentEvent> {
    let mut tr = Translator::replaying(cwd);
    let mut events = Vec::new();
    let mut title = None;
    for line in lines {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if v.get("isSidechain").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        match v.get("type").and_then(Value::as_str) {
            Some("ai-title") => title = v.get("aiTitle").and_then(Value::as_str).map(String::from),
            Some("user" | "assistant") => events.extend(tr.handle(&v)),
            _ => {}
        }
    }
    if let Some(t) = title {
        events.insert(0, AgentEvent::TitleChanged(t));
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use ucode_core::{SessionState, TimelineItem};

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ucode-claude-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const SAMPLE: &str = r#"{"type":"queue-operation","operation":"enqueue"}
{"type":"user","cwd":"/work/demo","message":{"role":"user","content":[{"type":"text","text":"读一下 a.txt"}]}}
{"type":"user","isMeta":true,"message":{"role":"user","content":"<command-name>/model</command-name>"}}
{"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/work/demo/a.txt"}}]}}
{"type":"user","toolUseResult":{"file":{"numLines":3}},"message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"x"}]}}
{"type":"assistant","isSidechain":true,"message":{"id":"m9","content":[{"type":"text","text":"子线程不应出现"}]}}
{"type":"assistant","message":{"id":"m2","content":[{"type":"text","text":"文件有 3 行"}]}}
{"type":"ai-title","aiTitle":"读取文件","sessionId":"s"}
"#;

    #[test]
    fn 项目目录编码() {
        assert_eq!(encode_project_dir("/Users/a/my.proj"), "-Users-a-my-proj");
    }

    #[test]
    fn 回放还原时间线() {
        let events = replay_lines(SAMPLE.lines(), "/work/demo");
        let mut s = SessionState::default();
        for e in events {
            s.apply(e);
        }
        assert_eq!(s.title, "读取文件");
        assert!(matches!(&s.items[0], TimelineItem::User { text, .. } if text == "读一下 a.txt"));
        assert!(matches!(&s.items[1], TimelineItem::Tool(t) if t.title == "读取 a.txt · 3 行"));
        assert!(matches!(&s.items[2], TimelineItem::Assistant { text, .. } if text == "文件有 3 行"));
        assert_eq!(s.items.len(), 3, "元信息与子线程被过滤");
    }

    #[test]
    fn 列出会话与项目() {
        let root = temp_dir("list");
        let proj = root.join(encode_project_dir("/work/demo"));
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(proj.join("s1.jsonl"), SAMPLE).unwrap();
        std::fs::write(proj.join("empty.jsonl"), "{\"type\":\"queue-operation\"}\n").unwrap();

        let sessions = list_sessions(&root, "/work/demo");
        assert_eq!(sessions.len(), 1, "没有用户输入的会话被忽略");
        assert_eq!(sessions[0].id, "s1");
        assert_eq!(sessions[0].title, "读取文件");
        assert_eq!(sessions[0].cwd, "/work/demo");

        let projects = list_projects(&root);
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].cwd, "/work/demo");
        assert_eq!(projects[0].session_count, 1);
        let _ = std::fs::remove_dir_all(&root);
    }
}
