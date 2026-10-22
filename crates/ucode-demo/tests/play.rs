//! 端到端:演示后端 + 会话状态归约。

use std::time::Duration;
use ucode_core::*;
use ucode_demo::DemoBackend;

/// 收集事件并归约,直到回合结束或超时。
fn run(prompt: &str, auto_resolve: bool) -> SessionState {
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
                    handle.send(AgentCommand::ResolvePlan { id: p.id.clone(), approve: true })
                }
                _ => {}
            }
        }
        state.apply(ev);
        if ended && started {
            break;
        }
    }
    handle.send(AgentCommand::Shutdown);
    state
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
