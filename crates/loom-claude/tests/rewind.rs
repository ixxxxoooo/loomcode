//! 文件检查点回退的端到端测试(真实 claude,`--ignored`):
//! `cargo test -p loom-claude --test rewind -- --ignored --nocapture`
//!
//! @author ixxxxoooo

use loom_claude::ClaudeBackend;
use loom_core::*;
use std::time::{Duration, Instant};

/// 持续收事件直到 `until` 返回 true(最多等 `secs` 秒),返回收到的全部事件。
fn collect(h: &BackendHandle, secs: u64, until: impl Fn(&AgentEvent) -> bool) -> Vec<AgentEvent> {
    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut out = Vec::new();
    while Instant::now() < deadline {
        match h.events.try_recv() {
            Ok(e) => {
                let stop = until(&e);
                out.push(e);
                if stop {
                    return out;
                }
            }
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    panic!("等待事件超时,已收到:{out:?}");
}

/// 发一次回退请求并等结果。
fn rewind(h: &BackendHandle, uuid: &str, dry_run: bool) -> RewindOutcome {
    h.send(AgentCommand::RewindFiles { uuid: uuid.into(), dry_run });
    match collect(h, 30, |e| matches!(e, AgentEvent::Rewind(_))).pop() {
        Some(AgentEvent::Rewind(o)) => o,
        other => panic!("没有回退结果:{other:?}"),
    }
}

/// 注意:检查点只记录编辑工具的改动,Claude 若改用 Bash 写文件,预览会是空的。
/// 验证:实时发出的消息带上我们的 uuid → Claude 改文件 → 预览列出文件 → 回退恢复磁盘;
/// 历史回放给出同一个 uuid(重开会话后也能回退)。
#[test]
#[ignore]
fn 真实_claude_回退文件() {
    let dir = std::env::temp_dir().join(format!("loom-rewind-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cwd = std::fs::canonicalize(&dir).unwrap().to_string_lossy().to_string();
    std::fs::write(dir.join("a.txt"), "原始内容\n").unwrap();
    let session = uuid::Uuid::new_v4().to_string();
    let cfg = BackendConfig { cwd: cwd.clone(), session_id: Some(session.clone()), model: Some("haiku".into()), mode: Some(PermissionMode::AcceptEdits), ..Default::default() };
    let h = ClaudeBackend::new(None).start(cfg).expect("需要本机已安装 claude");
    let mut events = collect(&h, 60, |e| matches!(e, AgentEvent::Ready { .. }));
    h.send(AgentCommand::Prompt { text: "用 Write 工具(不要用 Bash)把 a.txt 的内容改成「已修改」,并新建 b.txt 写入「新文件」。只做这两件事,不要解释。".into(), images: vec![], files: vec![], steer: false });
    events.extend(collect(&h, 180, |e| matches!(e, AgentEvent::TurnEnded(_))));
    let mut state = SessionState::default();
    for e in &events {
        state.apply(e.clone());
    }
    let user_id = state.items.iter().find_map(|i| matches!(i, TimelineItem::User { .. }).then(|| i.id().to_string())).expect("有用户消息");
    let uuid = state.user_uuids.get(&user_id).cloned().expect("用户消息应记下 uuid");
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap().trim(), "已修改", "Claude 应已改动文件");

    // 检查点只记编辑工具(Write / Edit)的改动,Bash 改的文件不在其中
    let tools: Vec<String> = state.items.iter().filter_map(|i| match i { TimelineItem::Tool(t) => Some(format!("{}:{:?}", t.name, t.status)), _ => None }).collect();
    let preview = rewind(&h, &uuid, true);
    assert!(preview.has_changes(), "预览应列出改动:{preview:?};工具调用:{tools:?}");
    assert_eq!(preview.files.len(), 2, "{:?}", preview.files);
    assert!(dir.join("b.txt").exists(), "预览不改磁盘");

    let done = rewind(&h, &uuid, false);
    assert!(done.can_rewind && done.error.is_none(), "{done:?}");
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap().trim(), "原始内容");
    assert!(!dir.join("b.txt").exists(), "新建的文件应被删除");
    h.send(AgentCommand::Shutdown);
    std::thread::sleep(Duration::from_millis(800));

    // 回放历史:同一条消息拿到同一个 uuid
    let projects = loom_claude::history::projects_dir().join(loom_claude::history::encode_project_dir(&cwd));
    let replayed = loom_claude::history::load_session(&projects.join(format!("{session}.jsonl")), &cwd).expect("会话已写盘");
    assert!(replayed.iter().any(|e| matches!(e, AgentEvent::UserUuid { uuid: u, .. } if *u == uuid)), "回放应给出同一个 uuid");

    let _ = loom_claude::history::delete_session_in(&projects, &session);
    let _ = std::fs::remove_dir_all(&dir);
}
