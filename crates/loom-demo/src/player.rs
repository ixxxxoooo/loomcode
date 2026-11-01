//! 脚本播放器:在独立线程里按时间轴发出事件,并响应用户命令。

use crate::scenarios::{self, Scenario};
use crate::script::{Script, Step};
use async_channel::{Receiver, Sender};
use std::collections::HashSet;
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
}

impl Player {
    pub fn new(cmd_rx: Receiver<AgentCommand>, ev_tx: Sender<AgentEvent>, speed: f32, config: BackendConfig) -> Self {
        let mode = config.mode.unwrap_or_default();
        let model = config.model.clone().unwrap_or_else(|| "sonnet".into());
        Self { cmd_rx, ev_tx, speed, config, resolved: HashSet::new(), mode, model, seq: 0 }
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
        while let Ok(cmd) = self.cmd_rx.recv_blocking() {
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
        self.send(AgentEvent::Append {
            parent: None,
            item: TimelineItem::User {
                id: format!("u-{tag}-{}", self.seq),
                text: prompt.to_string(),
                images: vec![],
                files: vec![],
            },
        });
        self.send(AgentEvent::TurnStarted);
        let outcome = self.run_steps(&script.steps);
        match outcome {
            Outcome::Shutdown => {}
            Outcome::Cancelled => self.send(AgentEvent::TurnEnded(StopReason::Cancelled)),
            Outcome::Finished => self.send(AgentEvent::TurnEnded(script.stop.unwrap_or(StopReason::EndTurn))),
        }
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
            AgentCommand::ResolvePlan { id, approve } => {
                self.resolved.insert(id.clone());
                self.send(AgentEvent::PlanResolved { id, approved: approve });
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
