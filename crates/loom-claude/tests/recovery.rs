//! 用 mock claude 验证故障恢复:握手超时、进程退出后自动重启、忙闲兜底、握手晚到不打断回合。
//!
//! @author ixxxxoooo

use loom_claude::{ClaudeBackend, LazyBackend};
use loom_core::*;
use std::time::{Duration, Instant};

/// mock claude 的后端;`init_timeout` 调短以便测试握手超时。
fn mock(init_timeout: Option<Duration>) -> ClaudeBackend {
    let mut b = ClaudeBackend::new(Some(format!("{}/tests/mock_claude.sh", env!("CARGO_MANIFEST_DIR"))));
    b.init_timeout = init_timeout;
    b
}

fn prompt(text: &str) -> AgentCommand {
    AgentCommand::Prompt { text: text.into(), images: vec![], files: vec![], steer: false }
}

/// 持续收事件直到 `until` 返回 true(最多 10 秒),返回收到的全部事件。
fn collect(h: &BackendHandle, until: impl Fn(&AgentEvent) -> bool) -> Vec<AgentEvent> {
    let deadline = Instant::now() + Duration::from_secs(10);
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
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    panic!("等待事件超时,已收到:{out:?}");
}

fn has_notice(evs: &[AgentEvent], level: NoticeLevel, needle: &str) -> bool {
    evs.iter().any(|e| matches!(e, AgentEvent::Notice { level: l, text } if *l == level && text.contains(needle)))
}

#[test]
fn 握手超时结束回合并提示() {
    let cfg = BackendConfig { cwd: "/tmp".into(), extra_env: vec![("MOCK_NO_INIT".into(), "1".into())], ..Default::default() };
    let h = mock(Some(Duration::from_millis(300))).start(cfg).expect("mock 可启动");
    h.send(prompt("你好"));
    let evs = collect(&h, |e| matches!(e, AgentEvent::Notice { level: NoticeLevel::Error, .. }));
    assert!(evs.iter().any(|e| matches!(e, AgentEvent::TurnEnded(StopReason::Error))), "{evs:?}");
    assert!(has_notice(&evs, NoticeLevel::Error, "启动超时"), "{evs:?}");
}

#[test]
fn 握手正常时计时到点不影响会话() {
    let h = mock(Some(Duration::from_millis(300))).start(BackendConfig { cwd: "/tmp".into(), ..Default::default() }).unwrap();
    collect(&h, |e| matches!(e, AgentEvent::Ready { .. }));
    std::thread::sleep(Duration::from_millis(500));
    h.send(prompt("你好"));
    let evs = collect(&h, |e| matches!(e, AgentEvent::PromoteToPermission { .. }));
    assert!(!has_notice(&evs, NoticeLevel::Error, ""), "{evs:?}");
    h.send(AgentCommand::Shutdown);
}

#[test]
fn 进程退出后下一条提示自动重启() {
    let h = LazyBackend::new(mock(None)).start(BackendConfig { cwd: "/tmp".into(), ..Default::default() }).unwrap();
    h.send(prompt("EXIT"));
    let evs = collect(&h, |e| matches!(e, AgentEvent::Notice { level: NoticeLevel::Error, .. }));
    assert!(has_notice(&evs, NoticeLevel::Error, "退出码 3"), "{evs:?}");
    // 进程退出后,查用量之类的命令不应拉起新进程,也不能卡住后续提示
    std::thread::sleep(Duration::from_millis(200));
    h.send(AgentCommand::RefreshUsage);
    h.send(prompt("你好"));
    let evs = collect(&h, |e| matches!(e, AgentEvent::PromoteToPermission { .. }));
    assert!(has_notice(&evs, NoticeLevel::Info, "重新启动"), "{evs:?}");
    assert!(evs.iter().any(|e| matches!(e, AgentEvent::Ready { .. })), "{evs:?}");
    h.send(AgentCommand::Shutdown);
}

#[test]
fn 空闲但没有_result_时兜底结束回合() {
    let h = mock(None).start(BackendConfig { cwd: "/tmp".into(), ..Default::default() }).unwrap();
    h.send(prompt("NORESULT"));
    let evs = collect(&h, |e| matches!(e, AgentEvent::TurnEnded(_)));
    assert!(matches!(evs.last(), Some(AgentEvent::TurnEnded(StopReason::EndTurn))), "{evs:?}");
    h.send(AgentCommand::Shutdown);
}

#[test]
fn 惰性启动时握手晚到_界面仍是运行态() {
    let h = LazyBackend::new(mock(None)).start(BackendConfig { cwd: "/tmp".into(), ..Default::default() }).unwrap();
    // 与界面一致:发送即进入运行态,再把后端事件依次归约
    let mut s = SessionState::default();
    s.apply(AgentEvent::TurnStarted);
    h.send(prompt("你好"));
    for e in collect(&h, |e| matches!(e, AgentEvent::PromoteToPermission { .. })) {
        s.apply(e);
    }
    assert_eq!(s.phase, Phase::Running, "等待权限时回合仍在进行");
    assert!(s.session_id.is_some());
    h.send(AgentCommand::Shutdown);
}

/// 真实 claude:进程被杀后下一条提示自动重启,并用 --resume 接上原会话的上下文(两次极小的 haiku 调用)。
/// `cargo test -p loom-claude --test recovery -- --ignored --nocapture`
#[test]
#[ignore]
fn 真实_claude_被杀后重启接续上下文() {
    std::fs::create_dir_all("/tmp/loom-probe").unwrap();
    let h = LazyBackend::new(ClaudeBackend::new(None))
        .start(BackendConfig { cwd: "/tmp/loom-probe".into(), model: Some("haiku".into()), ..Default::default() })
        .unwrap();
    let wait_end = |h: &BackendHandle| {
        let deadline = Instant::now() + Duration::from_secs(90);
        let mut out = Vec::new();
        while Instant::now() < deadline {
            match h.events.try_recv() {
                Ok(e) => {
                    let end = matches!(e, AgentEvent::TurnEnded(_));
                    out.push(e);
                    if end {
                        return out;
                    }
                }
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
        panic!("超时:{out:?}");
    };
    h.send(prompt("记住暗号 7341。只回复 ok"));
    let evs = wait_end(&h);
    let sid = evs.iter().find_map(|e| if let AgentEvent::Ready { session_id, .. } = e { Some(session_id.clone()) } else { None }).expect("有 Ready");
    let _ = std::process::Command::new("pkill").args(["-f", &sid]).status();
    std::thread::sleep(Duration::from_secs(1));
    h.send(prompt("刚才的暗号是多少?只回复数字"));
    let mut evs = wait_end(&h);
    // 被杀那次的退出提示可能先到,跳过它等真正的回合结束
    if !evs.iter().any(|e| matches!(e, AgentEvent::Append { item: TimelineItem::Assistant { .. }, .. })) {
        evs.extend(wait_end(&h));
    }
    let mut s = SessionState::default();
    for e in evs.iter().cloned() {
        s.apply(e);
    }
    println!("{:#?}", s.items);
    assert!(has_notice(&evs, NoticeLevel::Info, "重新启动"), "{evs:?}");
    assert!(s.items.iter().any(|i| matches!(i, TimelineItem::Assistant { text, .. } if text.contains("7341"))), "重启后应记得上下文");
    h.send(AgentCommand::Shutdown);
}
