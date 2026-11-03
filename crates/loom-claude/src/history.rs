//! 历史会话:列出 `~/.claude/projects` 下的会话,并把 jsonl 回放成事件。

use crate::translate::Translator;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use loom_core::{AgentEvent, TurnStats};

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

fn jsonl_files(dir: &Path) -> Vec<(PathBuf, u64, SystemTime)> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    rd.filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((e.path(), m.len(), m.modified().ok()?))
        })
        .collect()
}

/// 从会话文件里提取概要(走 `session_index` 的流式读取与增量缓存)。
/// 标题优先用 `ai-title`,其次第一条用户输入,再次最后一条提示;没有任何用户输入的会话不展示。
fn read_meta(path: &Path, size: u64, modified: SystemTime) -> Option<SessionMeta> {
    let scan = crate::session_index::scan(path, size, modified)?;
    let title = scan.title()?;
    let id = path.file_stem()?.to_string_lossy().into_owned();
    Some(SessionMeta { id, path: path.into(), cwd: scan.cwd, title, modified })
}

/// 列出某个工作目录下的历史会话,最近修改的在前。
pub fn list_sessions(projects: &Path, cwd: &str) -> Vec<SessionMeta> {
    list_sessions_in(&projects.join(encode_project_dir(cwd)))
}

/// 列出某个 projects 子目录里的会话。
pub fn list_sessions_in(dir: &Path) -> Vec<SessionMeta> {
    let metas = sessions_in(dir);
    crate::session_index::flush();
    metas
}

/// 扫描一个目录里的会话(不写盘,由对外入口统一 `flush`)。
fn sessions_in(dir: &Path) -> Vec<SessionMeta> {
    let mut metas: Vec<SessionMeta> = jsonl_files(dir).into_iter().filter_map(|(p, n, m)| read_meta(&p, n, m)).collect();
    metas.sort_by(|a, b| b.modified.cmp(&a.modified));
    metas
}

/// 删除某个会话的历史文件(`<dir>/<id>.jsonl`)。
///
/// 参数:`dir` 项目对应的 projects 子目录;`id` 会话 id,只允许字母、数字、`-`、`_`,
/// 防止借 `..` 或路径分隔符删到目录之外。
/// 返回:文件不存在视为成功(新会话还没写入历史);id 不合法返回 `InvalidInput`。
pub fn delete_session_in(dir: &Path, id: &str) -> std::io::Result<()> {
    let valid = !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !valid {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "会话 id 不合法"));
    }
    match std::fs::remove_file(dir.join(format!("{id}.jsonl"))) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// 删除某个工作目录下的会话历史文件。
pub fn delete_session(cwd: &str, id: &str) -> std::io::Result<()> {
    delete_session_in(&projects_dir().join(encode_project_dir(cwd)), id)
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
            let sessions = sessions_in(&e.path());
            let newest = sessions.first()?;
            let cwd = sessions.iter().map(|s| s.cwd.clone()).find(|c| !c.is_empty())?;
            let meta = ProjectMeta { dir: e.path(), cwd, session_count: sessions.len(), modified: newest.modified };
            Some((meta, sessions))
        })
        .collect();
    out.sort_by(|a, b| b.0.modified.cmp(&a.0.modified));
    crate::session_index::flush();
    out
}

/// 回放一个会话文件,得到可直接喂给 `SessionState::apply` 的事件序列。
pub fn load_session(path: &Path, cwd: &str) -> std::io::Result<Vec<AgentEvent>> {
    let text = std::fs::read_to_string(path)?;
    Ok(replay_lines(text.lines(), cwd))
}

/// 记下一条助手记录的用量:同一条消息(`message.id`)会有多行记录,后出现的覆盖先出现的(输出 token 以最后一行为准)。
fn record_usage(v: &Value, acc: &mut Vec<(String, TurnStats)>) {
    let m = &v["message"];
    let (Some(id), Some(u)) = (m.get("id").and_then(Value::as_str), m.get("usage")) else { return };
    let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
    let stats = TurnStats {
        input: n("input_tokens"),
        output: n("output_tokens"),
        thinking: u.pointer("/output_tokens_details/thinking_tokens").and_then(Value::as_u64).unwrap_or(0),
        cache_read: n("cache_read_input_tokens"),
        cache_write: n("cache_creation_input_tokens"),
        ..Default::default()
    };
    match acc.iter_mut().find(|(i, _)| i == id) {
        Some(slot) => slot.1 = stats,
        None => acc.push((id.to_string(), stats)),
    }
}

/// 把当前回合累积的各消息用量加总,作为 `TurnStats` 事件追加;没有用量就什么也不加。
fn flush_stats(uid: &mut Option<String>, acc: &mut Vec<(String, TurnStats)>, out: &mut Vec<AgentEvent>) {
    let total = acc.drain(..).fold(TurnStats::default(), |mut t, (_, s)| {
        t.input += s.input;
        t.output += s.output;
        t.thinking += s.thinking;
        t.cache_read += s.cache_read;
        t.cache_write += s.cache_write;
        t.calls = Some(t.calls.unwrap_or(0) + 1);
        t
    });
    if let Some(id) = uid.take().filter(|_| !total.is_empty()) {
        out.push(AgentEvent::TurnStats { id: Some(id), stats: total });
    }
}

/// 回放若干行 jsonl。
///
/// 除了条目事件,还会用每条记录的 `timestamp` 算出思考与整个回合的用时,作为 `Duration` 事件附在末尾。
pub fn replay_lines<'a>(lines: impl Iterator<Item = &'a str>, cwd: &str) -> Vec<AgentEvent> {
    use crate::history_time::{parse_iso_ms, secs_between};
    use loom_core::TimelineItem;
    let mut tr = Translator::replaying(cwd);
    let mut events = Vec::new();
    let mut title = None;
    // 上一条记录的时间;当前回合(用户消息 id、开始时间);算出的用时
    let mut prev_ts: Option<i64> = None;
    let mut turn: Option<(String, i64)> = None;
    let mut durations: Vec<AgentEvent> = Vec::new();
    // 当前回合各条助手消息的用量(消息 id → 用量;同一条消息的多行记录取后出现的),回合结束时汇总成 `TurnStats`
    let mut turn_usage: Vec<(String, TurnStats)> = Vec::new();
    let mut cur_uid: Option<String> = None;
    for line in lines {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if v.get("isSidechain").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        match v.get("type").and_then(Value::as_str) {
            Some("ai-title") => title = v.get("aiTitle").and_then(Value::as_str).map(String::from),
            Some("user" | "assistant") => {
                let ts = v.get("timestamp").and_then(Value::as_str).and_then(parse_iso_ms);
                if v.get("type").and_then(Value::as_str) == Some("assistant") {
                    record_usage(&v, &mut turn_usage);
                }
                let evs = tr.handle(&v);
                for e in &evs {
                    match e {
                        // 新的用户提示:上一个回合到此结束(用上一条记录的时间收尾)
                        AgentEvent::Append { parent: None, item: TimelineItem::User { id, .. } } => {
                            if let (Some((uid, start)), Some(end)) = (turn.take(), prev_ts) {
                                durations.extend(secs_between(start, end, 86_400).map(|secs| AgentEvent::Duration { id: uid, secs }));
                            }
                            flush_stats(&mut cur_uid, &mut turn_usage, &mut durations);
                            cur_uid = Some(id.clone());
                            turn = ts.map(|t| (id.clone(), t));
                        }
                        // 思考:从上一条记录到这一条的时间
                        AgentEvent::Append { parent: None, item: TimelineItem::Thinking { id, .. } } => {
                            if let (Some(t), Some(p)) = (ts, prev_ts) {
                                durations.extend(secs_between(p, t, 3600).map(|secs| AgentEvent::Duration { id: id.clone(), secs }));
                            }
                        }
                        _ => {}
                    }
                }
                events.extend(evs);
                if ts.is_some() {
                    prev_ts = ts;
                }
            }
            _ => {}
        }
    }
    if let (Some((uid, start)), Some(end)) = (turn.take(), prev_ts) {
        durations.extend(secs_between(start, end, 86_400).map(|secs| AgentEvent::Duration { id: uid, secs }));
    }
    flush_stats(&mut cur_uid, &mut turn_usage, &mut durations);
    events.extend(durations);
    if let Some(t) = title {
        events.insert(0, AgentEvent::TitleChanged(t));
    }
    events
}

#[cfg(test)]
mod tests {
    #[test]
    fn 回放按回合汇总用量() {
        let lines = [
            r#"{"type":"user","timestamp":"2026-01-01T00:00:00Z","message":{"role":"user","content":"问一"}}"#,
            // 同一条消息两行记录:后出现的覆盖先出现的
            r#"{"type":"assistant","timestamp":"2026-01-01T00:00:02Z","message":{"id":"m1","content":[{"type":"text","text":"a"}],"usage":{"input_tokens":2,"output_tokens":5,"cache_read_input_tokens":100,"cache_creation_input_tokens":10}}}"#,
            r#"{"type":"assistant","timestamp":"2026-01-01T00:00:03Z","message":{"id":"m1","content":[{"type":"text","text":"b"}],"usage":{"input_tokens":2,"output_tokens":40,"cache_read_input_tokens":100,"cache_creation_input_tokens":10}}}"#,
            r#"{"type":"assistant","timestamp":"2026-01-01T00:00:04Z","message":{"id":"m2","content":[{"type":"text","text":"c"}],"usage":{"input_tokens":1,"output_tokens":10,"cache_read_input_tokens":50,"cache_creation_input_tokens":0}}}"#,
            r#"{"type":"user","timestamp":"2026-01-01T00:01:00Z","message":{"role":"user","content":"问二"}}"#,
            r#"{"type":"assistant","timestamp":"2026-01-01T00:01:02Z","message":{"id":"m3","content":[{"type":"text","text":"d"}],"usage":{"input_tokens":7,"output_tokens":9,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#,
        ];
        let mut s = SessionState::default();
        for e in replay_lines(lines.iter().copied(), "/x") {
            s.apply(e);
        }
        let users: Vec<&str> = s.items.iter().filter_map(|i| if let TimelineItem::User { id, .. } = i { Some(id.as_str()) } else { None }).collect();
        let a = s.turn_stats.get(users[0]).copied().expect("第一回合有统计");
        assert_eq!((a.input, a.output, a.cache_read, a.cache_write, a.calls), (3, 50, 150, 10, Some(2)));
        assert_eq!(a.cost_usd, None, "历史里没有费用");
        let b = s.turn_stats.get(users[1]).copied().expect("最后一个回合收尾时也要汇总");
        assert_eq!((b.input, b.output), (7, 9));
    }

    use super::*;
    use loom_core::{SessionState, TimelineItem};

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("loom-claude-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn 删除会话只动自己的文件() {
        let dir = temp_dir("del");
        std::fs::write(dir.join("a.jsonl"), "x").unwrap();
        std::fs::write(dir.join("b.jsonl"), "x").unwrap();
        delete_session_in(&dir, "a").unwrap();
        assert!(!dir.join("a.jsonl").exists() && dir.join("b.jsonl").exists());
        // 不存在视为成功;不合法 id 被拒绝
        delete_session_in(&dir, "a").unwrap();
        assert!(delete_session_in(&dir, "../b").is_err());
        assert!(delete_session_in(&dir, "").is_err());
        assert!(dir.join("b.jsonl").exists());
        let _ = std::fs::remove_dir_all(dir);
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
    fn 回放算出思考与回合用时() {
        let jsonl = r#"{"type":"user","timestamp":"2026-10-09T10:00:00.000Z","cwd":"/w","message":{"role":"user","content":[{"type":"text","text":"问题一"}]}}
{"type":"assistant","timestamp":"2026-10-09T10:00:08.000Z","message":{"id":"m1","content":[{"type":"thinking","thinking":"想一想","signature":"x"}]}}
{"type":"assistant","timestamp":"2026-10-09T10:01:10.000Z","message":{"id":"m2","content":[{"type":"text","text":"回答一"}]}}
{"type":"user","timestamp":"2026-10-09T10:05:00.000Z","cwd":"/w","message":{"role":"user","content":[{"type":"text","text":"问题二"}]}}
{"type":"assistant","timestamp":"2026-10-09T10:05:30.000Z","message":{"id":"m3","content":[{"type":"text","text":"回答二"}]}}
"#;
        let mut s = SessionState::default();
        for e in replay_lines(jsonl.lines(), "/w") {
            s.apply(e);
        }
        let users: Vec<&str> = s.items.iter().filter_map(|i| if let TimelineItem::User { id, .. } = i { Some(id.as_str()) } else { None }).collect();
        let think = s.items.iter().find_map(|i| if let TimelineItem::Thinking { id, .. } = i { Some(id.clone()) } else { None }).expect("有思考条目");
        assert_eq!(s.durations.get(&think), Some(&8), "思考:上一条记录到这一条 8 秒");
        assert_eq!(s.durations.get(users[0]), Some(&70), "第一个回合 70 秒");
        assert_eq!(s.durations.get(users[1]), Some(&30), "最后一个回合用最后一条记录收尾");
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

