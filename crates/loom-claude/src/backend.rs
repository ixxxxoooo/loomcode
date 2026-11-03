//! 真实后端:一个 Actor 线程驱动 `claude` 子进程。
//!
//! 线程模型:
//! - 读 stdout 的线程、读 stderr 的线程、转发界面命令的线程,都只做一件事:往同一条 `Msg` 通道投递;
//! - Actor 线程独占 stdin 与 `Translator`,按顺序消费 `Msg`,因此没有任何共享可变状态与锁。

use crate::liveness::{Liveness, Signal};
use crate::options::{ClaudeOptions, find_claude};
use crate::protocol;
use crate::translate::{Translator, suggested_mode};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use loom_core::*;

/// 真实 Claude 后端。
#[derive(Debug, Clone, Default)]
pub struct ClaudeBackend {
    /// 用户在设置里指定的 `claude` 路径
    pub claude_path: Option<String>,
    pub add_dirs: Vec<String>,
    pub allowed_tools: Vec<String>,
    pub mcp_config: Option<String>,
    /// 握手超时(`None` 用默认的 60 秒;测试里调短)
    pub init_timeout: Option<std::time::Duration>,
}

impl ClaudeBackend {
    pub fn new(claude_path: Option<String>) -> Self {
        Self { claude_path, ..Default::default() }
    }
}

impl AgentBackend for ClaudeBackend {
    fn name(&self) -> &'static str {
        "claude"
    }

    fn start(&self, config: BackendConfig) -> Result<BackendHandle, BackendError> {
        let path = find_claude(self.claude_path.as_deref()).ok_or(BackendError::ClaudeNotFound)?;
        let mut opts = ClaudeOptions::from_config(&config);
        opts.add_dirs = self.add_dirs.clone();
        opts.allowed_tools = self.allowed_tools.clone();
        opts.mcp_config = self.mcp_config.clone();

        let mut child = Command::new(&path)
            .args(opts.args())
            .envs(opts.env())
            // 在 Claude Code 会话里启动本程序时,避免继承其嵌套标记
            .env_remove("CLAUDECODE")
            .current_dir(if opts.cwd.is_empty() { "." } else { &opts.cwd })
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| BackendError::Spawn(format!("{}:{e}", path.display())))?;

        loom_diag::info!("claude 已启动 pid={} 路径={} cwd={} 恢复会话={}", child.id(), path.display(), opts.cwd, opts.args().iter().any(|a| a == "--resume"));
        let stdin = child.stdin.take().ok_or_else(|| BackendError::Spawn("无法获取标准输入".into()))?;
        let stdout = child.stdout.take().ok_or_else(|| BackendError::Spawn("无法获取标准输出".into()))?;
        let stderr = child.stderr.take().ok_or_else(|| BackendError::Spawn("无法获取标准错误".into()))?;

        let (handle, cmd_rx, ev_tx) = channel_pair();
        let (tx, rx) = mpsc::channel::<Msg>();

        // stdout:按行读取,字节流容忍非法 UTF-8
        let t = tx.clone();
        spawn_named("loom-claude-stdout", move || {
            let mut reader = BufReader::new(stdout);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match reader.read_until(b'\n', &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let line = String::from_utf8_lossy(&buf).trim().to_string();
                        if !line.is_empty() && t.send(Msg::Line(line)).is_err() {
                            break;
                        }
                    }
                }
            }
            let _ = t.send(Msg::Eof);
        })?;

        // stderr:只保留尾部,进程异常退出时用于诊断
        let t = tx.clone();
        spawn_named("loom-claude-stderr", move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if t.send(Msg::Stderr(line)).is_err() {
                    break;
                }
            }
        })?;

        // 握手超时:claude 卡在启动阶段(既不应答也不退出)时,不能让界面一直停在「处理中」
        let t = tx.clone();
        let init_timeout = self.init_timeout.unwrap_or(INIT_TIMEOUT);
        spawn_named("loom-claude-init-timer", move || {
            std::thread::sleep(init_timeout);
            let _ = t.send(Msg::InitTimeout);
        })?;

        // 命令转发:界面侧通道关闭(会话被丢弃)等同于关闭后端
        let t = tx;
        spawn_named("loom-claude-cmd", move || {
            while let Ok(cmd) = cmd_rx.recv_blocking() {
                if t.send(Msg::Cmd(cmd)).is_err() {
                    return;
                }
            }
            let _ = t.send(Msg::Cmd(AgentCommand::Shutdown));
        })?;

        let mut actor = Actor::new(child, stdin, opts, ev_tx);
        spawn_named("loom-claude-actor", move || actor.run(rx))?;
        Ok(handle)
    }
}

fn spawn_named(name: &str, f: impl FnOnce() + Send + 'static) -> Result<(), BackendError> {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(f)
        .map(|_| ())
        .map_err(|e| BackendError::Spawn(e.to_string()))
}

/// 握手(`initialize`)应答的最长等待时间,与官方 Agent SDK 一致。
const INIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

enum Msg {
    Cmd(AgentCommand),
    Line(String),
    Stderr(String),
    Eof,
    /// 握手计时到点(握手已完成时忽略)
    InitTimeout,
}

/// 已发出、等待响应的控制请求。
enum ReqKind {
    Init,
    SetModel(String),
    SetMode(PermissionMode),
    Other(&'static str),
    Usage,
    Context,
    /// `mcp_status`
    McpStatus,
    /// `mcp_toggle`:应答后再刷新一次状态与占用
    McpToggle,
    Rewind { uuid: String, dry_run: bool },
}

struct Actor {
    child: Child,
    stdin: ChildStdin,
    translator: Translator,
    opts: ClaudeOptions,
    ev_tx: async_channel::Sender<AgentEvent>,
    reqs: HashMap<String, ReqKind>,
    req_seq: u64,
    /// 上次向官方接口查询用量的时间(30 秒内不重复查)
    usage_asked_at: Option<std::time::Instant>,
    turn_active: bool,
    stderr_tail: VecDeque<String>,
    /// 界面主动关闭时,进程退出不再当作异常
    shutting_down: bool,
    /// 后端确认过的权限模式(切换失败时回退到它,界面不能停在没生效的模式上)
    confirmed_mode: PermissionMode,
    /// 进程启动时刻与 pid(退出日志里给出存活时长)
    started: std::time::Instant,
    pid: u32,
    /// 无法解析为 JSON 的输出行计数(日志只记前几条与每 100 条)
    bad_lines: u32,
    /// 忙闲信号跟踪(`session_state_changed` / `command_lifecycle`)
    liveness: Liveness,
}

impl Actor {
    fn new(child: Child, stdin: ChildStdin, opts: ClaudeOptions, ev_tx: async_channel::Sender<AgentEvent>) -> Self {
        // 每个进程一个代号:惰性后端重启进程后,新条目 id 不会与时间线里旧进程的条目撞车
        static ID_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let mut translator = Translator::new(&opts.cwd);
        translator.id_gen = ID_GEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        translator.session_id = opts.effective_session_id();
        let pid = child.id();
        Self {
            child,
            stdin,
            translator,
            opts,
            ev_tx,
            reqs: HashMap::new(),
            req_seq: 0,
            usage_asked_at: None,
            turn_active: false,
            stderr_tail: VecDeque::new(),
            shutting_down: false,
            confirmed_mode: PermissionMode::Default,
            started: std::time::Instant::now(),
            pid,
            bad_lines: 0,
            liveness: Liveness::default(),
        }
    }

    fn run(&mut self, rx: mpsc::Receiver<Msg>) {
        self.request(ReqKind::Init, |id| protocol::initialize(id));
        while let Ok(msg) = rx.recv() {
            let keep_going = match msg {
                Msg::Cmd(cmd) => self.on_command(cmd),
                Msg::Line(line) => self.on_line(&line),
                Msg::Stderr(line) => {
                    if self.stderr_tail.len() >= 20 {
                        self.stderr_tail.pop_front();
                    }
                    self.stderr_tail.push_back(line);
                    true
                }
                Msg::Eof => {
                    self.on_exit();
                    false
                }
                Msg::InitTimeout => self.on_init_timeout(),
            };
            if !keep_going {
                break;
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    // ---------- 输出 ----------

    /// 往界面发事件;界面已离开时返回 false。
    fn emit(&self, event: AgentEvent) -> bool {
        self.ev_tx.try_send(event).is_ok()
    }

    fn emit_all(&self, events: Vec<AgentEvent>) -> bool {
        events.into_iter().all(|e| self.emit(e))
    }

    fn write(&mut self, v: &Value) {
        let mut line = v.to_string();
        line.push('\n');
        // 进程已死时写入会失败,随后 Eof 会处理善后
        let _ = self.stdin.write_all(line.as_bytes()).and_then(|_| self.stdin.flush());
    }

    /// 刷新 MCP 服务器状态与各工具的上下文占用(不碰账户限额);同类请求在途时不重复发。
    fn refresh_mcp(&mut self) {
        if !self.reqs.values().any(|k| matches!(k, ReqKind::McpStatus)) {
            self.request(ReqKind::McpStatus, protocol::mcp_status);
        }
        if !self.reqs.values().any(|k| matches!(k, ReqKind::Context)) {
            self.request(ReqKind::Context, protocol::get_context_usage);
        }
    }

    fn request(&mut self, kind: ReqKind, build: impl FnOnce(&str) -> Value) {
        self.req_seq += 1;
        let id = if matches!(kind, ReqKind::Init) { "req_init".to_string() } else { format!("req_{}", self.req_seq) };
        let v = build(&id);
        self.reqs.insert(id, kind);
        self.write(&v);
    }

    // ---------- 界面命令 ----------

    fn on_command(&mut self, cmd: AgentCommand) -> bool {
        match cmd {
            AgentCommand::Prompt { text, images, steer, .. } => {
                let steering = steer && self.turn_active;
                if !self.turn_active {
                    self.turn_active = true;
                    self.translator.interrupted = false;
                    if !self.emit(AgentEvent::TurnStarted) {
                        return false;
                    }
                }
                // claude 不回显用户输入,这里补上时间线条目(引导消息同样)
                let uuid = uuid::Uuid::new_v4().to_string();
                let echo = self.translator.user_prompt(&text, &images, &uuid);
                if !self.emit_all(echo) {
                    return false;
                }
                let msg = protocol::user_message(&text, &images, steering, &uuid);
                self.liveness.sent(msg.get("uuid").and_then(Value::as_str).unwrap_or_default());
                self.write(&msg);
            }
            AgentCommand::Cancel => {
                if self.turn_active {
                    self.translator.interrupted = true;
                    self.request(ReqKind::Other("中断"), |id| protocol::interrupt(id));
                }
            }
            AgentCommand::SetModel(m) => {
                let target = m.clone();
                self.request(ReqKind::SetModel(m), |id| protocol::set_model(id, &target));
            }
            AgentCommand::SetMode(mode) => {
                self.request(ReqKind::SetMode(mode), |id| protocol::set_permission_mode(id, mode));
            }
            AgentCommand::SetEffort(e) => {
                self.request(ReqKind::Other("切换思考强度"), |id| protocol::set_effort(id, e));
            }
            AgentCommand::ResolvePermission { id, option } => {
                let Some(p) = self.translator.take_pending(&id) else { return true };
                self.write(&protocol::permission_response(&p, &option));
                let mut ok = self.emit(AgentEvent::PermissionResolved { id, option: option.clone() });
                if option == "allow_always"
                    && let Some(mode) = suggested_mode(&p)
                {
                    ok &= self.emit(AgentEvent::ModeChanged(mode));
                }
                return ok;
            }
            AgentCommand::AnswerQuestion { id, answers } => {
                let Some(p) = self.translator.take_pending(&id) else { return true };
                self.write(&protocol::question_response(&p, &answers));
                return self.emit(AgentEvent::QuestionAnswered { id, answers });
            }
            AgentCommand::ResolvePlan { id, approve, mode } => {
                let Some(p) = self.translator.take_pending(&id) else { return true };
                self.write(&protocol::plan_response(&p, approve, mode));
                let mut ok = self.emit(AgentEvent::PlanResolved { id, approved: approve });
                if approve && let Some(mode) = mode.or_else(|| suggested_mode(&p)) {
                    ok &= self.emit(AgentEvent::ModeChanged(mode));
                }
                return ok;
            }
            AgentCommand::RewindFiles { uuid, dry_run } => {
                let kind = ReqKind::Rewind { uuid: uuid.clone(), dry_run };
                self.request(kind, |id| protocol::rewind_files(id, &uuid, dry_run));
            }
            AgentCommand::StopTask { task_id } => {
                self.request(ReqKind::Other("停止任务"), |id| protocol::stop_task(id, &task_id));
            }
            AgentCommand::BackgroundCurrent { tool_use_id } => {
                self.request(ReqKind::Other("转入后台"), |id| protocol::background_tasks(id, tool_use_id.as_deref()));
            }
            AgentCommand::RefreshUsage { force } => {
                // 上下文明细(本地请求,成本低)每次打开都刷新;账户限额同一时刻只查一次,
                // 且数据还新就不重复查(缓存 `LIMITS_TTL_SECS`),只有用户点「刷新」(force)才越过缓存
                if !self.reqs.values().any(|k| matches!(k, ReqKind::Context)) {
                    self.request(ReqKind::Context, protocol::get_context_usage);
                }
                let fresh = !force && self.usage_asked_at.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(loom_core::LIMITS_TTL_SECS));
                if fresh || self.reqs.values().any(|k| matches!(k, ReqKind::Usage)) {
                    return true;
                }
                self.usage_asked_at = Some(std::time::Instant::now());
                let ev = self.translator.usage_checking();
                self.request(ReqKind::Usage, protocol::get_usage);
                return self.emit(ev);
            }
            AgentCommand::RefreshMcp => self.refresh_mcp(),
            AgentCommand::McpToggle { name, enabled } => {
                self.request(ReqKind::McpToggle, |id| protocol::mcp_toggle(id, &name, enabled));
            }
            AgentCommand::RunScenario(_) | AgentCommand::SetEnv(_) => {}
            AgentCommand::Shutdown => {
                self.shutting_down = true;
                loom_diag::info!("claude pid={} 由界面关闭,存活 {} 秒", self.pid, self.started.elapsed().as_secs());
                // 先把挂起的请求都拒绝掉,让 claude 干净退出
                for (_, p) in self.translator.drain_pending() {
                    self.write(&protocol::reject_response(&p));
                }
                return false;
            }
        }
        true
    }

    // ---------- claude 输出 ----------

    fn on_line(&mut self, line: &str) -> bool {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            // 非 JSON 行(例如启动横幅)当作诊断信息保留
            self.bad_lines += 1;
            if self.bad_lines <= 3 || self.bad_lines % 100 == 0 {
                loom_diag::warn!("claude(pid {})输出了非 JSON 行(第 {} 条):{}", self.pid, self.bad_lines, line.chars().take(120).collect::<String>());
            }
            self.stderr_tail.push_back(line.chars().take(200).collect());
            return true;
        };
        match v.get("type").and_then(Value::as_str).unwrap_or_default() {
            "control_response" => return self.on_control_response(&v),
            "control_request" => {
                let (events, reply) = self.translator.control_request(&v);
                if let Some(reply) = reply {
                    self.write(&reply);
                }
                return self.emit_all(events);
            }
            "control_cancel_request" => {
                let id = v.get("request_id").and_then(Value::as_str).unwrap_or_default();
                let events = self.translator.cancel_request(id);
                return self.emit_all(events);
            }
            _ => {}
        }

        let ty = v.get("type").and_then(Value::as_str).unwrap_or_default();
        match self.liveness.on_message(&v) {
            // claude 自行开启的回合(后台任务通知、中断后接着处理的引导消息):比第一条流式输出更早进入运行态
            Signal::Busy if !self.turn_active => {
                self.turn_active = true;
                if !self.emit(AgentEvent::TurnStarted) {
                    return false;
                }
            }
            // 兜底:claude 已空闲却没收到 result,不能让界面一直停在「处理中」
            Signal::Idle if self.turn_active => {
                loom_diag::warn!("claude(pid {})已空闲但未收到 result,按回合结束处理", self.pid);
                self.turn_active = false;
                let reason = if std::mem::take(&mut self.translator.interrupted) { StopReason::Cancelled } else { StopReason::EndTurn };
                return self.emit(AgentEvent::TurnEnded(reason));
            }
            _ => {}
        }
        // claude 自行开启的回合(排队消息、后台任务通知)也要让界面进入运行态(旧版 claude 没有忙闲事件)
        if !self.turn_active && matches!(ty, "stream_event" | "assistant") {
            self.turn_active = true;
            if !self.emit(AgentEvent::TurnStarted) {
                return false;
            }
        }
        let events = self.translator.handle(&v);
        if ty == "result" {
            self.turn_active = false;
        }
        self.emit_all(events)
    }

    fn on_control_response(&mut self, v: &Value) -> bool {
        let resp = &v["response"];
        let id = resp.get("request_id").and_then(Value::as_str).unwrap_or_default();
        let Some(kind) = self.reqs.remove(id) else { return true };
        let ok = resp.get("subtype").and_then(Value::as_str) == Some("success");
        let error = resp.get("error").and_then(Value::as_str).unwrap_or("未知错误").to_string();
        let warn = |what: &str| AgentEvent::Notice { level: NoticeLevel::Warning, text: format!("{what}失败:{error}") };

        match kind {
            ReqKind::Init => {
                let body = &resp["response"];
                let models: Vec<ModelInfo> = body
                    .get("models")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(|m| {
                                let value = m.get("value")?.as_str()?;
                                let name = m.get("displayName").and_then(Value::as_str).unwrap_or(value);
                                let desc = m.get("description").and_then(Value::as_str).unwrap_or_default();
                                Some(ModelInfo::new(value, name, desc))
                            })
                            .collect()
                    })
                    .filter(|m: &Vec<ModelInfo>| !m.is_empty())
                    .unwrap_or_else(default_models);
                let mode = self
                    .opts
                    .mode
                    .or_else(|| body.get("current_permission_mode").and_then(Value::as_str).and_then(PermissionMode::from_cli))
                    .unwrap_or_default();
                let model = self.opts.model.clone().unwrap_or_else(|| "default".into());
                self.confirmed_mode = mode;
                let mut events = vec![AgentEvent::Ready { session_id: self.opts.effective_session_id(), models, model, mode }];
                if !ok {
                    events.push(warn("初始化"));
                }
                self.emit_all(events)
            }
            ReqKind::SetModel(m) => {
                if ok { self.emit(AgentEvent::ModelChanged(m)) } else { self.emit(warn("切换模型")) }
            }
            ReqKind::SetMode(m) => {
                if ok {
                    self.confirmed_mode = m;
                    self.emit(AgentEvent::ModeChanged(m))
                } else {
                    // 没切成功:提示原因,并把界面退回到仍然生效的那个模式
                    let back = self.confirmed_mode;
                    self.emit_all(vec![warn("切换权限模式"), AgentEvent::ModeChanged(back)])
                }
            }
            ReqKind::McpStatus => match self.translator.mcp_status_event(&resp["response"], ok) {
                Some(ev) => self.emit(ev),
                None => true,
            },
            ReqKind::McpToggle => {
                if !ok {
                    loom_diag::warn!("mcp_toggle 失败:{error}");
                }
                // 不管成败都刷新一次:列表以后端实际状态为准
                self.refresh_mcp();
                true
            }
            ReqKind::Context => match self.translator.context_breakdown(&resp["response"], ok) {
                Some(ev) => self.emit(ev),
                None => true,
            },
            ReqKind::Rewind { uuid, dry_run } => {
                let outcome = crate::rewind::parse(&uuid, dry_run, ok, &resp["response"], &error);
                loom_diag::info!("回退文件 dry_run={dry_run} 结果:可回退={} 文件 {} 个 错误={:?}", outcome.can_rewind, outcome.files.len(), outcome.error);
                self.emit(AgentEvent::Rewind(outcome))
            }
            ReqKind::Usage => {
                let ev = self.translator.account_usage(&resp["response"], ok);
                self.emit(ev)
            }
            ReqKind::Other(what) => {
                // 中断失败时撤销取消标记,避免下一个回合被误判为已取消
                if !ok && what == "中断" {
                    self.translator.interrupted = false;
                }
                if ok { true } else { self.emit(warn(what)) }
            }
        }
    }

    /// 握手计时到点。
    ///
    /// 返回:是否继续运行。握手已完成时继续;仍未应答则提示原因、结束回合并关闭进程
    /// (惰性后端会在下一条提示时重新启动)。
    fn on_init_timeout(&mut self) -> bool {
        if !self.reqs.contains_key("req_init") {
            return true;
        }
        let tail = self.stderr_tail.iter().cloned().collect::<Vec<_>>().join("\n");
        loom_diag::warn!("claude(pid {}){} 秒内未应答握手,结束进程。stderr 尾部:{}", self.pid, INIT_TIMEOUT.as_secs(), tail);
        let mut text = format!("claude 启动超时:{} 秒内没有响应。请在终端运行 claude 确认能正常启动(登录、信任目录、网络或代理)。", INIT_TIMEOUT.as_secs());
        if !tail.is_empty() {
            text.push_str("\n");
            text.push_str(&tail);
        }
        if self.turn_active {
            self.turn_active = false;
            let _ = self.emit(AgentEvent::TurnEnded(StopReason::Error));
        }
        let _ = self.emit(AgentEvent::Notice { level: NoticeLevel::Error, text });
        // 进程随后会被 kill,它的退出不再另报一次
        self.shutting_down = true;
        false
    }

    fn on_exit(&mut self) {
        if self.shutting_down {
            return;
        }
        let status = self.child.wait().ok();
        let code = status.and_then(|s| s.code());
        {
            use std::os::unix::process::ExitStatusExt;
            let signal = status.and_then(|s| s.signal());
            loom_diag::warn!(
                "claude 进程退出 pid={} 退出码={:?} 信号={:?} 存活 {} 秒 回合进行中={} stderr 尾部:{}",
                self.pid, code, signal, self.started.elapsed().as_secs(), self.turn_active,
                self.stderr_tail.iter().rev().take(5).rev().cloned().collect::<Vec<_>>().join(" | ")
            );
        }
        let mut text = match code {
            Some(0) => "claude 进程已退出".to_string(),
            Some(c) => format!("claude 进程异常退出(退出码 {c})"),
            None => "claude 进程被信号终止".to_string(),
        };
        if !self.stderr_tail.is_empty() {
            text.push_str(":\n");
            text.push_str(&self.stderr_tail.iter().cloned().collect::<Vec<_>>().join("\n"));
        }
        if self.turn_active {
            self.turn_active = false;
            let _ = self.emit(AgentEvent::TurnEnded(StopReason::Error));
        }
        let _ = self.emit(AgentEvent::Notice { level: NoticeLevel::Error, text });
    }
}
