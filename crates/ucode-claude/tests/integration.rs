//! 用 shell 脚本模拟的 claude 做端到端测试:覆盖进程管理、握手、权限应答、中断与异常退出。

use std::time::{Duration, Instant};
use ucode_claude::{ClaudeBackend, LazyBackend};
use ucode_core::*;

fn mock_backend() -> ClaudeBackend {
    let path = format!("{}/tests/mock_claude.sh", env!("CARGO_MANIFEST_DIR"));
    ClaudeBackend::new(Some(path))
}

fn start() -> BackendHandle {
    mock_backend()
        .start(BackendConfig { cwd: "/tmp".into(), ..Default::default() })
        .expect("mock 可启动")
}

/// 持续收事件直到 `until` 返回 true(最多等 10 秒),返回收到的全部事件。
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

fn prompt(text: &str) -> AgentCommand {
    AgentCommand::Prompt { text: text.into(), images: vec![], files: vec![], steer: false }
}

fn reduce(events: &[AgentEvent]) -> SessionState {
    let mut s = SessionState::default();
    for e in events {
        s.apply(e.clone());
    }
    s
}

#[test]
fn 握手后得到_ready_与模型列表() {
    let h = start();
    let events = collect(&h, |e| matches!(e, AgentEvent::Ready { .. }));
    let AgentEvent::Ready { models, mode, session_id, .. } = events.last().unwrap() else { unreachable!() };
    assert_eq!(models.len(), 2);
    assert_eq!(models[1].id, "opus");
    assert_eq!(*mode, PermissionMode::Default);
    assert!(!session_id.is_empty());
    h.send(AgentCommand::Shutdown);
}

#[test]
fn 允许权限后回合正常结束() {
    let h = start();
    collect(&h, |e| matches!(e, AgentEvent::Ready { .. }));
    h.send(prompt("你好"));
    let mut events = collect(&h, |e| matches!(e, AgentEvent::PromoteToPermission { .. }));
    let AgentEvent::PromoteToPermission { request, .. } = events.last().unwrap().clone() else { unreachable!() };
    assert_eq!(request.id, "tu1");
    h.send(AgentCommand::ResolvePermission { id: "tu1".into(), option: "allow_once".into() });
    events.extend(collect(&h, |e| matches!(e, AgentEvent::TurnEnded(_))));

    let s = reduce(&events);
    assert_eq!(s.last_stop, Some(StopReason::EndTurn));
    assert_eq!(s.phase, Phase::Idle);
    // claude 不回显用户输入,后端要自己把提示追加进时间线
    assert!(s.items.iter().any(|i| matches!(i, TimelineItem::User { text, .. } if text == "你好")));
    let assistants = s.items.iter().filter(|i| matches!(i, TimelineItem::Assistant { .. })).count();
    assert_eq!(assistants, 1, "流式文字不重复");
    let perm = s.items.iter().find_map(|i| if let TimelineItem::Permission(p) = i { Some(p) } else { None }).unwrap();
    assert_eq!(perm.resolved.as_deref(), Some("allow_once"));
    assert_eq!(perm.tool.status, ToolStatus::Completed);
    assert!(matches!(&perm.tool.content[0], ToolContent::Terminal { output, .. } if output == "hi"));
    assert!((s.usage.cost_usd - 0.01).abs() < 1e-9);
    h.send(AgentCommand::Shutdown);
}

#[test]
fn 拒绝权限让工具失败() {
    let h = start();
    collect(&h, |e| matches!(e, AgentEvent::Ready { .. }));
    h.send(prompt("你好"));
    collect(&h, |e| matches!(e, AgentEvent::PromoteToPermission { .. }));
    h.send(AgentCommand::ResolvePermission { id: "tu1".into(), option: "reject".into() });
    let events = collect(&h, |e| matches!(e, AgentEvent::TurnEnded(_)));
    assert!(events.iter().any(|e| matches!(e, AgentEvent::PermissionResolved { option, .. } if option == "reject")));
    assert!(events.iter().any(|e| matches!(e, AgentEvent::ToolUpdate { patch, .. } if patch.status == Some(ToolStatus::Failed))));
    h.send(AgentCommand::Shutdown);
}

#[test]
fn 切换权限模式得到确认事件() {
    let h = start();
    collect(&h, |e| matches!(e, AgentEvent::Ready { .. }));
    h.send(AgentCommand::SetMode(PermissionMode::Plan));
    let events = collect(&h, |e| matches!(e, AgentEvent::ModeChanged(_)));
    assert!(matches!(events.last(), Some(AgentEvent::ModeChanged(PermissionMode::Plan))));
    h.send(AgentCommand::Shutdown);
}

#[test]
fn 取消回合标记为已取消() {
    let h = start();
    collect(&h, |e| matches!(e, AgentEvent::Ready { .. }));
    h.send(prompt("SLEEP 不回应"));
    collect(&h, |e| matches!(e, AgentEvent::TurnStarted));
    h.send(AgentCommand::Cancel);
    let events = collect(&h, |e| matches!(e, AgentEvent::TurnEnded(_)));
    assert!(matches!(events.last(), Some(AgentEvent::TurnEnded(StopReason::Cancelled))));
    h.send(AgentCommand::Shutdown);
}

#[test]
fn 进程异常退出时给出诊断() {
    let h = start();
    collect(&h, |e| matches!(e, AgentEvent::Ready { .. }));
    h.send(prompt("EXIT"));
    let events = collect(&h, |e| matches!(e, AgentEvent::Notice { level: NoticeLevel::Error, .. }));
    assert!(events.iter().any(|e| matches!(e, AgentEvent::TurnEnded(StopReason::Error))));
    let Some(AgentEvent::Notice { text, .. }) = events.last() else { unreachable!() };
    assert!(text.contains("退出码 3"), "{text}");
    assert!(text.contains("boom on stderr"), "{text}");
}

#[test]
fn 无效自定义路径不会崩溃() {
    let err = ClaudeBackend::new(Some("/不存在/claude".into()));
    // 自定义路径无效时会回退到 PATH;为了让测试与本机安装无关,只断言不 panic 且返回类型正确
    let r = err.start(BackendConfig { cwd: "/tmp".into(), ..Default::default() });
    if let Ok(h) = r {
        h.send(AgentCommand::Shutdown);
    }
}

#[test]
fn 惰性后端收到第一条命令才启动() {
    let h = LazyBackend::new(mock_backend()).start(BackendConfig { cwd: "/tmp".into(), ..Default::default() }).unwrap();
    // 启动后没有任何命令:不会有事件(尤其没有 Ready)
    std::thread::sleep(Duration::from_millis(300));
    assert!(h.events.try_recv().is_err(), "未发送命令前不应启动子进程");
    h.send(prompt("你好"));
    let evs = collect(&h, |e| matches!(e, AgentEvent::PromoteToPermission { .. }));
    assert!(evs.iter().any(|e| matches!(e, AgentEvent::Ready { .. })), "{evs:?}");
    h.send(AgentCommand::Shutdown);
}

/// 真实 claude 的冒烟测试(会产生极小的费用),默认不运行:
/// `cargo test -p ucode-claude --test integration -- --ignored --nocapture`
#[test]
#[ignore]
fn 真实_claude_冒烟() {
    let backend = ClaudeBackend::new(None);
    let h = backend
        .start(BackendConfig { cwd: "/tmp/ucode-probe".into(), model: Some("haiku".into()), ..Default::default() })
        .expect("需要本机已安装 claude");
    let ready = collect(&h, |e| matches!(e, AgentEvent::Ready { .. }));
    assert!(matches!(ready.last(), Some(AgentEvent::Ready { models, .. }) if !models.is_empty()));
    h.send(prompt("只回复两个字母:ok"));
    let events = collect(&h, |e| matches!(e, AgentEvent::TurnEnded(_)));
    let s = reduce(&events);
    println!("{:#?}", s.items);
    assert_eq!(s.last_stop, Some(StopReason::EndTurn));
    assert!(s.items.iter().any(|i| matches!(i, TimelineItem::Assistant { text, .. } if text.to_lowercase().contains("ok"))));
    h.send(AgentCommand::Shutdown);
}

/// 真实 claude 的会话分叉(会产生两次极小的 haiku 调用),默认不运行:
/// `cargo test -p ucode-claude --test integration -- --ignored --nocapture 分叉`
///
/// 验证:分叉出的新会话 id 与预分配的一致、原会话文件不被改动、新会话带着原会话的上下文。
#[test]
#[ignore]
fn 真实_claude_会话分叉() {
    std::fs::create_dir_all("/tmp/ucode-probe").unwrap();
    // macOS 上 /tmp 是 /private/tmp 的符号链接,claude 按真实路径给项目目录编码
    let cwd = std::fs::canonicalize("/tmp/ucode-probe").unwrap().to_string_lossy().to_string();
    let cwd = cwd.as_str();
    let dir = ucode_claude::history::projects_dir().join(ucode_claude::history::encode_project_dir(cwd));
    let (src, new) = (uuid::Uuid::new_v4().to_string(), uuid::Uuid::new_v4().to_string());
    let file = |id: &str| dir.join(format!("{id}.jsonl"));
    let lines = |id: &str| std::fs::read_to_string(file(id)).map(|t| t.lines().count()).unwrap_or(0);
    let run_turn = |cfg: BackendConfig, text: &str| -> SessionState {
        let h = ClaudeBackend::new(None).start(cfg).expect("需要本机已安装 claude");
        let ready = collect(&h, |e| matches!(e, AgentEvent::Ready { .. }));
        let ready_id = match ready.last() {
            Some(AgentEvent::Ready { session_id, .. }) => session_id.clone(),
            other => panic!("没有 Ready:{other:?}"),
        };
        h.send(prompt(text));
        let mut events = ready;
        events.extend(collect(&h, |e| matches!(e, AgentEvent::TurnEnded(_))));
        h.send(AgentCommand::Shutdown);
        // 等 claude 把会话文件写完再返回
        std::thread::sleep(Duration::from_millis(800));
        let mut s = reduce(&events);
        s.session_id = Some(ready_id);
        s
    };

    // 1) 原会话:让它记住一个暗号
    let base = BackendConfig { cwd: cwd.into(), model: Some("haiku".into()), ..Default::default() };
    run_turn(BackendConfig { session_id: Some(src.clone()), ..base.clone() }, "记住暗号:蓝鲸。只回复两个字:好的");
    let src_lines = lines(&src);
    assert!(src_lines > 0, "原会话应已写进磁盘");

    // 2) 分叉:恢复原会话但生成新会话,追问暗号
    let forked = run_turn(
        BackendConfig { resume: Some(src.clone()), session_id: Some(new.clone()), fork_session: true, ..base },
        "刚才的暗号是什么?只回答暗号本身",
    );
    assert_eq!(forked.session_id.as_deref(), Some(new.as_str()), "Ready 事件里的会话 id 应是分叉出来的新 id");
    assert!(lines(&new) > src_lines, "新会话应包含原会话的历史加上这一轮");
    assert_eq!(lines(&src), src_lines, "原会话文件不应被分叉改动");
    assert!(
        forked.items.iter().any(|i| matches!(i, TimelineItem::Assistant { text, .. } if text.contains("蓝鲸"))),
        "分叉会话应带着原会话的上下文:{:#?}",
        forked.items
    );

    for id in [&src, &new] {
        let _ = ucode_claude::history::delete_session_in(&dir, id);
    }
}

/// 性能基线:读取本机全部 Claude 历史并回放最大的会话,打印耗时。
/// `cargo test -p ucode-claude --release --test integration -- --ignored --nocapture 性能`
#[test]
#[ignore]
fn 性能基线_历史扫描与回放() {
    use std::time::Instant;
    let dir = ucode_claude::history::projects_dir();
    let t = Instant::now();
    let projects = ucode_claude::history::list_projects_with_sessions(&dir);
    let sessions: usize = projects.iter().map(|(_, s)| s.len()).sum();
    println!("扫描 {} 个项目 / {} 个会话:{:?}", projects.len(), sessions, t.elapsed());
    let biggest = projects.iter().flat_map(|(_, s)| s.iter()).max_by_key(|s| std::fs::metadata(&s.path).map(|m| m.len()).unwrap_or(0));
    if let Some(s) = biggest {
        let size = std::fs::metadata(&s.path).map(|m| m.len()).unwrap_or(0);
        let t = Instant::now();
        let events = ucode_claude::history::load_session(&s.path, &s.cwd).unwrap();
        println!("回放最大会话 {:.1} MB → {} 个事件:{:?}", size as f64 / 1e6, events.len(), t.elapsed());
    }
}
