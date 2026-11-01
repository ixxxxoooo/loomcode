//! 端到端:演示后端 + 会话状态归约。

use std::time::Duration;
use loom_core::*;
use loom_demo::DemoBackend;

/// 收集事件并归约,直到回合结束或超时。
fn run(prompt: &str, auto_resolve: bool) -> SessionState {
    let (handle, state) = run_keep(prompt, auto_resolve);
    handle.send(AgentCommand::Shutdown);
    state
}

/// 同 `run`,但保留后端句柄(后台任务仍在运行,可继续发命令)。
fn run_keep(prompt: &str, auto_resolve: bool) -> (BackendHandle, SessionState) {
    let backend = DemoBackend { speed: 0.0 };
    let handle = backend.start(BackendConfig::default()).unwrap();
    handle.send(AgentCommand::Prompt { text: prompt.into(), images: vec![], files: vec![], steer: false });
    let mut state = SessionState::default();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut started = false;
    while std::time::Instant::now() < deadline {
        let Ok(ev) = handle.events.recv_blocking() else { break };
        let ended = matches!(ev, AgentEvent::TurnEnded(_));
        if matches!(ev, AgentEvent::TurnStarted) {
            started = true;
        }
        // 自动处理所有等待项
        if auto_resolve {
            match &ev {
                AgentEvent::Append { item: TimelineItem::Permission(p), .. } => {
                    handle.send(AgentCommand::ResolvePermission { id: p.id.clone(), option: "allow".into() })
                }
                AgentEvent::Append { item: TimelineItem::Question(q), .. } => {
                    handle.send(AgentCommand::AnswerQuestion { id: q.id.clone(), answers: vec![vec!["x".into()]; q.questions.len()] })
                }
                AgentEvent::Append { item: TimelineItem::PlanApproval(p), .. } => {
                    handle.send(AgentCommand::ResolvePlan { id: p.id.clone(), approve: true, mode: None })
                }
                _ => {}
            }
        }
        state.apply(ev);
        if ended && started {
            break;
        }
    }
    (handle, state)
}

/// 跑完回合后继续收取后台任务的事件,直到连续 300ms 没有新事件。
fn run_drain(handle: &BackendHandle, state: &mut SessionState) {
    let mut quiet_since = std::time::Instant::now();
    while quiet_since.elapsed() < Duration::from_millis(120) {
        match handle.events.try_recv() {
            Ok(ev) => {
                state.apply(ev);
                quiet_since = std::time::Instant::now();
            }
            Err(_) => std::thread::sleep(Duration::from_millis(5)),
        }
    }
}

#[test]
fn 全景场景跑完且没有遗留等待项() {
    let s = run("/demo-showcase-tour", true);
    assert_eq!(s.phase, Phase::Idle);
    assert_eq!(s.last_stop, Some(StopReason::EndTurn));
    assert!(!s.awaiting_user());
    assert!(s.items.iter().any(|i| matches!(i, TimelineItem::Subagent(c) if !c.children.is_empty())));
    assert!(s.items.iter().any(|i| matches!(i, TimelineItem::Changes { .. })));
}

#[test]
fn 未识别提示给出演示回复() {
    let s = run("你好", false);
    assert!(s.items.iter().any(|i| matches!(i, TimelineItem::Assistant { text, .. } if text.contains("演示模式"))));
}

#[test]
fn 取消冻结场景() {
    let backend = DemoBackend { speed: 0.0 };
    let h = backend.start(BackendConfig::default()).unwrap();
    h.send(AgentCommand::Prompt { text: "/demo-states-frozen".into(), images: vec![], files: vec![], steer: false });
    let mut state = SessionState::default();
    // 等到出现子 Agent(说明脚本已停在 Hold)
    loop {
        let ev = h.events.recv_blocking().unwrap();
        let is_sub = matches!(&ev, AgentEvent::Append { item: TimelineItem::Subagent(_), .. });
        state.apply(ev);
        if is_sub {
            break;
        }
    }
    assert_eq!(state.phase, Phase::Running);
    h.send(AgentCommand::Cancel);
    loop {
        let ev = h.events.recv_blocking().unwrap();
        let end = matches!(ev, AgentEvent::TurnEnded(_));
        state.apply(ev);
        if end {
            break;
        }
    }
    assert_eq!(state.last_stop, Some(StopReason::Cancelled));
    assert!(state.items.iter().all(|i| !matches!(i, TimelineItem::Tool(t) if !t.status.is_finished())));
}

/// 所有时间线条目 id(含子 Agent 内部)必须唯一。
fn all_ids(items: &[TimelineItem], out: &mut Vec<String>) {
    for i in items {
        out.push(i.id().to_string());
        if let TimelineItem::Subagent(c) = i {
            all_ids(&c.children, out);
        }
    }
}

#[test]
fn 每个场景跑完都满足不变量() {
    for sc in loom_demo::scenarios::all() {
        if matches!(sc.kind, loom_demo::scenarios::Kind::Frozen | loom_demo::scenarios::Kind::Abnormal) {
            continue;
        }
        let (h, mut s) = run_keep(&format!("/demo-{}", sc.id), true);
        run_drain(&h, &mut s);
        h.send(AgentCommand::Shutdown);
        assert_eq!(s.phase, Phase::Idle, "{}: 回合没有结束", sc.id);
        assert!(!s.awaiting_user(), "{}: 遗留等待项", sc.id);
        assert!(s.items.iter().all(|i| !matches!(i, TimelineItem::Tool(t) if !t.status.is_finished())), "{}: 有未结束的工具", sc.id);
        let mut ids = Vec::new();
        all_ids(&s.items, &mut ids);
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n, "{}: 时间线 id 重复", sc.id);
        // 终态的任务不能还有「运行中」的子 Agent 卡片
        for t in s.tasks.iter().filter(|t| t.status.is_finished()) {
            let active = s.items.iter().any(|i| matches!(i, TimelineItem::Subagent(c) if c.task_id.as_deref() == Some(&t.task_id) && c.is_active()));
            assert!(!active, "{}: 任务 {} 已结束但卡片仍在运行", sc.id, t.task_id);
        }
        // 运行中的后台任务数 = 任务表里满足条件的数量
        assert_eq!(s.running_task_count(), s.tasks.iter().filter(|t| t.counts_as_running()).count(), "{}", sc.id);
    }
}

fn find_task<'a>(s: &'a SessionState, id: &str) -> &'a BackgroundTask {
    s.task(id).unwrap_or_else(|| panic!("缺少任务 {id}"))
}

#[test]
fn 后台命令场景的任务状态() {
    let (h, mut s) = run_keep("/demo-bg-shell", true);
    run_drain(&h, &mut s);
    // 开发服务器一直运行;构建完成;lint 失败;旧监听已停止
    assert_eq!(find_task(&s, "sh-dev").status, TaskStatus::Running);
    assert_eq!(find_task(&s, "sh-build").status, TaskStatus::Completed);
    assert_eq!(find_task(&s, "sh-lint").status, TaskStatus::Failed);
    assert_eq!(find_task(&s, "sh-old").status, TaskStatus::Stopped);
    assert!(find_task(&s, "sh-dev").output.contains("VITE"));
    assert_eq!(s.running_task_count(), 1);
    // 停止开发服务器:状态变已停止,出现一条通知,计数归零
    h.send(AgentCommand::StopTask { task_id: "sh-dev".into() });
    run_drain(&h, &mut s);
    assert_eq!(find_task(&s, "sh-dev").status, TaskStatus::Stopped);
    assert_eq!(s.running_task_count(), 0);
    assert!(s.items.iter().any(|i| matches!(i, TimelineItem::TaskNote { task_id, status: TaskStatus::Stopped, .. } if task_id == "sh-dev")));
    h.send(AgentCommand::Shutdown);
}

#[test]
fn 后台子代理场景的卡片与任务同步() {
    let (h, mut s) = run_keep("/demo-bg-agent", true);
    run_drain(&h, &mut s);
    fn card(s: &SessionState, id: &str) -> SubagentCard {
        s.items.iter().find_map(|i| match i { TimelineItem::Subagent(c) if c.id == id => Some(c.clone()), _ => None }).unwrap()
    }
    let card = |id: &str, s: &SessionState| card(s, id);
    assert_eq!(card("ag-a", &s).status, SubagentStatus::Completed);
    assert_eq!(card("ag-r", &s).status, SubagentStatus::Failed);
    assert!(card("ag-c", &s).is_active() && card("ag-c", &s).background);
    assert!(card("ag-d", &s).stalled(now_ms()), "停滞的子代理应被标记");
    assert!(card("ag-a", &s).tool_count >= 3);
    // 停止长时间调研
    h.send(AgentCommand::StopTask { task_id: "t-ag-c".into() });
    run_drain(&h, &mut s);
    assert_eq!(find_task(&s, "t-ag-c").status, TaskStatus::Stopped);
    assert_eq!(card("ag-c", &s).status, SubagentStatus::Stopped);
    h.send(AgentCommand::Shutdown);
}

#[test]
fn 恢复运行复用任务编号() {
    let (h, mut s) = run_keep("/demo-bg-resume", true);
    run_drain(&h, &mut s);
    assert_eq!(s.tasks.iter().filter(|t| t.task_id == "t-res").count(), 1);
    assert_eq!(find_task(&s, "t-res").status, TaskStatus::Completed);
    // 子 Agent 的状态在自己的卡片上,不再重复插通知行
    let notes = s.items.iter().filter(|i| matches!(i, TimelineItem::TaskNote { task_id, .. } if task_id == "t-res")).count();
    assert_eq!(notes, 0);
    assert!(s.items.iter().any(|i| matches!(i, TimelineItem::Subagent(c) if c.id == "res-1" && c.status == SubagentStatus::Completed)));
    h.send(AgentCommand::Shutdown);
}

#[test]
fn 全场景会话覆盖所有展示() {
    let (h, mut s) = run_keep("/demo-showcase-all", true);
    run_drain(&h, &mut s);
    h.send(AgentCommand::Shutdown);
    let has = |f: &dyn Fn(&TimelineItem) -> bool| s.items.iter().any(|i| f(i));
    assert!(has(&|i| matches!(i, TimelineItem::TaskNote { .. })));
    assert!(has(&|i| matches!(i, TimelineItem::Subagent(c) if c.background)));
    assert!(has(&|i| matches!(i, TimelineItem::Subagent(c) if c.status == SubagentStatus::Stopped)));
    assert!(has(&|i| matches!(i, TimelineItem::Tool(t) if t.status == ToolStatus::Denied)));
    assert!(has(&|i| matches!(i, TimelineItem::Tool(t) if t.status == ToolStatus::Interrupted)));
    assert!(has(&|i| matches!(i, TimelineItem::Tool(t) if t.status == ToolStatus::Failed)));
    assert!(has(&|i| matches!(i, TimelineItem::Todos { .. })));
    assert!(s.tasks.iter().any(|t| t.kind == TaskKind::Monitor));
    assert!(s.tasks.iter().any(|t| t.kind == TaskKind::Workflow));
    assert!(s.running_task_count() >= 3);
}

#[test]
fn 后台子代理的用时按真实时间计算() {
    // 速度 0.02:脚本偏移被压缩,但各事件之间仍有真实间隔
    let backend = DemoBackend { speed: 1.0 };
    let h = backend.start(BackendConfig::default()).unwrap();
    h.send(AgentCommand::Prompt { text: "/demo-bg-agent".into(), images: vec![], files: vec![], steer: false });
    let mut s = SessionState::default();
    let deadline = std::time::Instant::now() + Duration::from_secs(12);
    while std::time::Instant::now() < deadline {
        match h.events.try_recv() {
            Ok(ev) => s.apply(ev),
            Err(_) => std::thread::sleep(Duration::from_millis(5)),
        }
        if s.task("t-ag-a").is_some_and(|t| t.status.is_finished()) {
            break;
        }
    }
    h.send(AgentCommand::Shutdown);
    let card = s.items.iter().find_map(|i| match i { TimelineItem::Subagent(c) if c.id == "ag-a" => Some(c.clone()), _ => None }).unwrap();
    assert_eq!(card.status, SubagentStatus::Completed);
    let ms = card.elapsed_ms(now_ms()).expect("应有用时");
    assert!(ms >= 3000, "用时应来自真实时间,实际 {ms}ms");
}
