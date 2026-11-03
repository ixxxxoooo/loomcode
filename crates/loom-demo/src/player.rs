//! 脚本播放器:在独立线程里按时间轴发出事件,并响应用户命令。

use crate::scenarios::{self, Scenario};
use crate::script::{BgEvent, BgJob, Script, Step};
use async_channel::{Receiver, Sender};
use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};
use loom_core::*;

/// 播放结果。
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Finished,
    Cancelled,
    Shutdown,
}

pub struct Player {
    cmd_rx: Receiver<AgentCommand>,
    ev_tx: Sender<AgentEvent>,
    speed: f32,
    config: BackendConfig,
    /// 已被用户处理过的等待项 id(权限/提问/计划),避免并发堆叠时丢失
    resolved: HashSet<String>,
    mode: PermissionMode,
    model: String,
    /// 生成用户条目 id 的序号
    seq: u32,
    /// 运行中的后台任务时间线
    jobs: Vec<LiveJob>,
    /// 当前是否有回合在进行(后台任务的续写只在空闲时发出)
    in_turn: bool,
    /// 演示的 MCP 服务器状态(`McpToggle` 会改它)
    mcp: Vec<McpServerStatus>,
}

/// 一个正在推进的后台任务。
struct LiveJob {
    task_id: String,
    start: Instant,
    events: VecDeque<BgEvent>,
    on_stop: Vec<AgentEvent>,
    /// 已发出终态通知(之后不再可停止)
    done: bool,
}

impl Player {
    pub fn new(cmd_rx: Receiver<AgentCommand>, ev_tx: Sender<AgentEvent>, speed: f32, config: BackendConfig) -> Self {
        let mode = config.mode.unwrap_or_default();
        let model = config.model.clone().unwrap_or_else(|| "sonnet".into());
        Self { cmd_rx, ev_tx, speed, config, resolved: HashSet::new(), mode, model, seq: 0, jobs: Vec::new(), in_turn: false, mcp: crate::script::demo_mcp_servers() }
    }

    fn send(&self, e: AgentEvent) {
        let _ = self.ev_tx.send_blocking(e);
    }

    /// 主循环:发出 Ready 后等待命令。
    pub fn run(&mut self) {
        let session_id = self
            .config
            .resume
            .clone()
            .unwrap_or_else(|| format!("demo-{:x}", std::process::id()));
        self.send(AgentEvent::Ready {
            session_id,
            models: default_models(),
            model: self.model.clone(),
            mode: self.mode,
        });
        self.send(AgentEvent::McpServers(self.mcp.clone()));
        loop {
            self.tick_jobs();
            // 有后台任务在跑时要轮询推进它们,否则阻塞等命令
            let cmd = if self.jobs.is_empty() {
                match self.cmd_rx.recv_blocking() {
                    Ok(c) => c,
                    Err(_) => return,
                }
            } else {
                match self.cmd_rx.try_recv() {
                    Ok(c) => c,
                    Err(async_channel::TryRecvError::Empty) => {
                        std::thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                    Err(_) => return,
                }
            };
            match cmd {
                AgentCommand::Prompt { text, .. } => self.on_prompt(&text),
                AgentCommand::RunScenario(id) => {
                    if let Some(sc) = scenarios::find(&id) {
                        self.play_scenario(&sc, None);
                    }
                }
                AgentCommand::Shutdown => return,
                other => self.handle_idle(other),
            }
        }
    }

    /// 推进后台任务:发出已到时的事件。续写(`followup`)只在空闲时发出,否则顺延。
    fn tick_jobs(&mut self) {
        let speed = self.speed;
        let mut out = Vec::new();
        for job in &mut self.jobs {
            let elapsed = job.start.elapsed();
            while let Some(front) = job.events.front() {
                if Duration::from_millis((front.at_ms as f32 * speed) as u64) > elapsed {
                    break;
                }
                if front.followup && self.in_turn {
                    break;
                }
                if let Some(ev) = job.events.pop_front() {
                    if matches!(ev.event, AgentEvent::TaskNotification { .. }) {
                        job.done = true;
                    }
                    out.push(ev.event);
                }
            }
        }
        // 还有待发事件,或尚未结束且可被停止的任务继续保留(例如一直运行的开发服务器)
        self.jobs.retain(|j| !j.events.is_empty() || (!j.done && !j.on_stop.is_empty()));
        for e in out {
            self.send(e);
        }
    }

    /// 停止后台任务:丢掉它剩余的时间线,发出已停止通知。
    fn stop_job(&mut self, task_id: &str) {
        if let Some(i) = self.jobs.iter().position(|j| j.task_id == task_id) {
            let job = self.jobs.remove(i);
            for e in job.on_stop {
                self.send(e);
            }
        }
    }

    fn on_prompt(&mut self, text: &str) {
        match scenarios::resolve(text) {
            Some(sc) => self.play_scenario(&sc, Some(text)),
            None => self.play_echo(text),
        }
    }

    /// 空闲时收到的设置类命令。
    fn handle_idle(&mut self, cmd: AgentCommand) {
        match cmd {
            AgentCommand::SetMode(m) => {
                self.mode = m;
                self.send(AgentEvent::ModeChanged(m));
            }
            AgentCommand::SetModel(m) => {
                self.model = m.clone();
                self.send(AgentEvent::ModelChanged(m));
            }
            AgentCommand::McpToggle { name, enabled } => {
                if let Some(s) = self.mcp.iter_mut().find(|s| s.name == name) {
                    s.conn = if enabled { McpConn::Connected } else { McpConn::Disabled };
                }
                self.send(AgentEvent::McpServers(self.mcp.clone()));
            }
            AgentCommand::RefreshMcp => self.send(AgentEvent::McpServers(self.mcp.clone())),
            AgentCommand::StopTask { task_id } => self.stop_job(&task_id),
            AgentCommand::RewindFiles { uuid, dry_run } => self.send(AgentEvent::Rewind(crate::rewind::outcome(&uuid, dry_run))),
            _ => {}
        }
    }

    /// 未识别的提示:给一个简短的演示回复。
    fn play_echo(&mut self, text: &str) {
        let mut b = crate::script::ScriptBuilder::new();
        b.think("这是演示模式:没有连接真实的 Claude。我会按脚本回复。").sleep(200);
        b.say(&format!(
            "你好!这里是 **LoomCode 演示模式**。你刚才说的是:\n\n> {text}\n\n试试输入 `/demo-showcase-tour`,或按 ⌘K 打开「演示场景」查看全部场景。"
        ));
        self.play(&b.build(), &format!("echo:{text}"), text, "回复");
    }

    fn play_scenario(&mut self, sc: &Scenario, typed: Option<&str>) {
        let script = (sc.build)();
        let prompt = typed.map(str::to_string).unwrap_or_else(|| sc.prompt.to_string());
        self.send(AgentEvent::TitleChanged(sc.title.to_string()));
        self.play(&script, sc.id, &prompt, sc.title);
    }

    /// 播放一个脚本:用户消息 → 回合开始 → 步骤 → 回合结束。
    fn play(&mut self, script: &Script, tag: &str, prompt: &str, _title: &str) {
        self.seq += 1;
        let id = format!("u-{tag}-{}", self.seq);
        self.send(AgentEvent::Append {
            parent: None,
            item: TimelineItem::User {
                id: id.clone(),
                text: prompt.to_string(),
                images: vec![],
                files: vec![],
            },
        });
        self.send(AgentEvent::UserUuid { uuid: crate::rewind::uuid_for(&id), id });
        self.send(AgentEvent::TurnStarted);
        self.in_turn = true;
        let outcome = self.run_steps(&script.steps);
        self.in_turn = false;
        match outcome {
            Outcome::Shutdown => {}
            Outcome::Cancelled => self.send(AgentEvent::TurnEnded(StopReason::Cancelled)),
            Outcome::Finished => {
                let reason = script.stop.unwrap_or(StopReason::EndTurn);
                if reason == StopReason::EndTurn {
                    self.send(AgentEvent::TurnStats { id: None, stats: demo_stats(script.steps.len()) });
                }
                self.send(AgentEvent::TurnEnded(reason));
            }
        }
    }

    fn spawn(&mut self, job: &BgJob) {
        self.jobs.retain(|j| j.task_id != job.task_id);
        let mut events = job.events.clone();
        events.sort_by_key(|e| e.at_ms);
        self.jobs.push(LiveJob { task_id: job.task_id.clone(), start: Instant::now(), events: events.into(), on_stop: job.on_stop.clone(), done: false });
    }

    fn run_steps(&mut self, steps: &[Step]) -> Outcome {
        for step in steps {
            let outcome = match step {
                Step::Emit(e) => {
                    self.send(e.clone());
                    None
                }
                Step::Sleep(ms) => self.wait(Some(Duration::from_millis((*ms as f32 * self.speed) as u64)), |_| false),
                Step::WaitPermission(id) | Step::WaitQuestion(id) | Step::WaitPlan(id) => {
                    let id = id.clone();
                    self.wait(None, move |p| p.resolved.contains(&id))
                }
                Step::Hold => self.wait(None, |_| false),
                Step::Spawn(job) => {
                    self.spawn(job);
                    None
                }
            };
            if let Some(o) = outcome {
                return o;
            }
        }
        Outcome::Finished
    }

    /// 等待:到时间、条件满足、被取消或关闭时返回。
    /// 返回 `None` 表示正常继续,`Some(outcome)` 表示中止当前脚本。
    fn wait(&mut self, timeout: Option<Duration>, done: impl Fn(&Self) -> bool) -> Option<Outcome> {
        let deadline = timeout.map(|d| Instant::now() + d);
        loop {
            self.tick_jobs();
            if done(self) {
                return None;
            }
            match self.cmd_rx.try_recv() {
                Ok(AgentCommand::Cancel) => return Some(Outcome::Cancelled),
                Ok(AgentCommand::Shutdown) => return Some(Outcome::Shutdown),
                Ok(cmd) => self.handle_running(cmd),
                Err(async_channel::TryRecvError::Closed) => return Some(Outcome::Shutdown),
                Err(async_channel::TryRecvError::Empty) => {}
            }
            if let Some(d) = deadline {
                if Instant::now() >= d {
                    return None;
                }
                std::thread::sleep(Duration::from_millis(2));
            } else {
                std::thread::sleep(Duration::from_millis(8));
            }
        }
    }

    /// 运行中收到的命令。
    fn handle_running(&mut self, cmd: AgentCommand) {
        match cmd {
            AgentCommand::ResolvePermission { id, option } => {
                self.resolved.insert(id.clone());
                self.send(AgentEvent::PermissionResolved { id, option });
            }
            AgentCommand::AnswerQuestion { id, answers } => {
                self.resolved.insert(id.clone());
                self.send(AgentEvent::QuestionAnswered { id, answers });
            }
            AgentCommand::ResolvePlan { id, approve, mode } => {
                self.resolved.insert(id.clone());
                self.send(AgentEvent::PlanResolved { id, approved: approve });
                if approve && let Some(mode) = mode {
                    self.send(AgentEvent::ModeChanged(mode));
                }
            }
            AgentCommand::Prompt { text, images, files, .. } => {
                // 运行中的引导消息:直接作为用户条目追加
                self.seq += 1;
                self.send(AgentEvent::Append {
                    parent: None,
                    item: TimelineItem::User { id: format!("steer-{}", self.seq), text, images, files },
                });
            }
            other => self.handle_idle(other),
        }
    }
}

/// 演示用的回合消耗统计:按脚本步数确定性地造一份(同一场景每次一样),覆盖「有费用 / 有缓存 / 有思考」。
fn demo_stats(steps: usize) -> TurnStats {
    let n = steps as u64 + 1;
    TurnStats {
        input: 3 + n % 5,
        output: 180 + n * 37,
        thinking: 60 + n * 11,
        cache_read: 24_000 + n * 1_300,
        cache_write: 2_000 + n * 210,
        cost_usd: Some(0.004 + n as f64 * 0.0013),
        api_ms: Some(3_200 + n * 410),
        ttft_ms: Some(1_800 + n * 25),
        calls: Some(1 + (n / 3) as u32),
    }
}
