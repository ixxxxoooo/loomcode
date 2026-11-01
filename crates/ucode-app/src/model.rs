//! 会话模型:把后端事件泵进 `SessionState`,并驱动时间线的虚拟列表。

use gpui_kit::component::message_scroller::MessageScrollerState;
use gpui_kit::*;
use std::collections::HashSet;
use ucode_core::*;

/// 一个活动会话。界面通过它读取状态、发送命令。
pub struct SessionModel {
    pub state: SessionState,
    pub handle: BackendHandle,
    pub scroller: Entity<MessageScrollerState>,
    /// 已展开的条目 id(工具卡、思考、子 Agent、计划等)
    pub expanded: HashSet<String>,
    /// 运行中排队的消息(回合结束后依次发送)
    pub queued: Vec<String>,
    /// 提问向导当前所在题号(按提问 id)
    pub wizard_step: std::collections::HashMap<String, usize>,
    /// 提问向导已选答案草稿(按提问 id → 每题的已选项)
    pub wizard_draft: std::collections::HashMap<String, Vec<Vec<String>>>,
    /// 一次滚轮手势里累计的垂直位移与防抖任务是否已排队(用来吸附误触的微小滚动)
    pub wheel_acc: f32,
    pub wheel_pending: bool,
    wheel_last: f32,
}

impl SessionModel {
    /// 创建会话并启动事件泵。
    pub fn new(handle: BackendHandle, cx: &mut Context<Self>) -> Self {
        let scroller = cx.new(|cx| MessageScrollerState::new(0, cx));
        let rx = handle.events.clone();
        cx.spawn(async move |this, cx| {
            // 批量消费:一次唤醒尽量取空队列,只通知一次界面
            while let Ok(first) = rx.recv().await {
                let mut batch = vec![first];
                while let Ok(more) = rx.try_recv() {
                    batch.push(more);
                    if batch.len() >= 64 {
                        break;
                    }
                }
                if this.update(cx, |m, cx| m.on_events(batch, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        Self {
            state: SessionState::default(),
            handle,
            scroller,
            expanded: HashSet::new(),
            queued: Vec::new(),
            wizard_step: Default::default(),
            wizard_draft: Default::default(),
            wheel_acc: 0.,
            wheel_pending: false,
            wheel_last: 0.,
        }
    }

    /// 处理一批事件:归约状态,并让虚拟列表同步行数与高度。
    fn on_events(&mut self, events: Vec<AgentEvent>, cx: &mut Context<Self>) {
        let mut dirty_rows: Vec<usize> = Vec::new();
        let mut turn_ended = false;
        for ev in events {
            let target = ev.target_id().map(str::to_string);
            if matches!(ev, AgentEvent::TurnEnded(_)) {
                turn_ended = true;
            }
            self.state.apply(ev);
            if let Some(id) = target {
                if let Some(row) = self.state.row_of(&id) {
                    dirty_rows.push(row);
                }
            }
        }
        let new_count = self.state.items.len();
        let scroller = self.scroller.clone();
        scroller.update(cx, |s, cx| {
            let old = s.item_count();
            if new_count > old {
                s.append(new_count - old, cx);
            } else if new_count < old {
                s.reset(new_count, cx);
            }
            dirty_rows.sort_unstable();
            dirty_rows.dedup();
            let dirty_last = new_count > 0 && dirty_rows.last() == Some(&(new_count - 1));
            for row in dirty_rows {
                if row < old.min(new_count) {
                    s.remeasure_items(row..row + 1, cx);
                }
            }
            // 流式输出时最后一行也需要重新测量
            // (已在脏行里的不重复测量,避免同一帧重复失效造成跳动)
            if new_count > 0 && new_count <= old && !dirty_last {
                s.remeasure_items(new_count - 1..new_count, cx);
            }
        });
        if turn_ended {
            // 回合结束:历史列表与分支可能变了,通知工作台刷新
            cx.emit(UiRequest::SessionSettled);
        }
        if turn_ended && !self.queued.is_empty() {
            let next = self.queued.remove(0);
            self.send_prompt(next, cx);
        }
        cx.notify();
    }

    /// 载入历史会话的回放事件(真实模式恢复会话时使用)。
    ///
    /// 回放是异步完成的,期间用户可能已经发出了消息:历史必须排在这些实时条目的前面。
    pub fn load_history(&mut self, events: Vec<AgentEvent>, cx: &mut Context<Self>) {
        let live = std::mem::take(&mut self.state.items);
        for ev in events {
            self.state.apply(ev);
        }
        self.state.items.extend(live);
        // 条目整体变了:一次性重置虚拟列表,避免逐条增量与高度缓存错位
        let n = self.state.items.len();
        self.scroller.update(cx, |s, cx| s.reset(n, cx));
        cx.notify();
    }

    /// 发送提示。运行中会作为「引导」消息追加。
    pub fn send_prompt(&mut self, text: String, cx: &mut Context<Self>) {
        let steer = self.state.phase == Phase::Running;
        self.handle.send(AgentCommand::Prompt { text, images: vec![], files: vec![], steer });
        cx.notify();
    }

    /// 带图片与文件引用发送。
    pub fn send_prompt_with(&mut self, text: String, images: Vec<String>, files: Vec<String>, cx: &mut Context<Self>) {
        let steer = self.state.phase == Phase::Running;
        self.handle.send(AgentCommand::Prompt { text, images, files, steer });
        cx.notify();
    }

    /// 运行中把消息排到队尾。
    pub fn queue_prompt(&mut self, text: String, cx: &mut Context<Self>) {
        self.queued.push(text);
        cx.notify();
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        self.handle.send(AgentCommand::Cancel);
        cx.notify();
    }

    pub fn set_mode(&mut self, mode: PermissionMode, cx: &mut Context<Self>) {
        self.state.mode = mode;
        self.handle.send(AgentCommand::SetMode(mode));
        cx.notify();
    }

    pub fn set_model(&mut self, model: String, cx: &mut Context<Self>) {
        self.state.model = model.clone();
        self.handle.send(AgentCommand::SetModel(model));
        cx.notify();
    }

    pub fn set_effort(&mut self, effort: Effort, cx: &mut Context<Self>) {
        self.state.effort = effort;
        self.handle.send(AgentCommand::SetEffort(effort));
        cx.notify();
    }

    pub fn resolve_permission(&mut self, id: &str, option: &str, cx: &mut Context<Self>) {
        self.handle.send(AgentCommand::ResolvePermission { id: id.into(), option: option.into() });
        self.remeasure_id(id, cx);
    }

    pub fn answer_question(&mut self, id: &str, answers: Vec<Vec<String>>, cx: &mut Context<Self>) {
        self.handle.send(AgentCommand::AnswerQuestion { id: id.into(), answers });
        self.remeasure_id(id, cx);
    }

    pub fn resolve_plan(&mut self, id: &str, approve: bool, cx: &mut Context<Self>) {
        self.handle.send(AgentCommand::ResolvePlan { id: id.into(), approve });
        self.remeasure_id(id, cx);
    }

    /// 展开 / 折叠一个条目,并重新测量它所在的行。
    pub fn toggle(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.expanded.remove(id) {
            self.expanded.insert(id.to_string());
        }
        self.remeasure_id(id, cx);
    }

    pub fn is_expanded(&self, id: &str) -> bool {
        self.expanded.contains(id)
    }

    fn remeasure_id(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(row) = self.state.row_of(id) {
            self.scroller.update(cx, |s, cx| {
                s.remeasure_items(row..row + 1, cx);
            });
        }
        cx.notify();
    }

    /// 滚轮事件:累计位移,手势停下 150ms 后,若只是在贴底处轻微晃动(净位移 < 40px)
    /// 就吸附回底部并恢复跟随,避免触控板误触让会话流脱离底部。
    ///
    /// 参数:`dy` 本次滚轮的垂直位移(像素)。
    pub fn on_wheel(&mut self, dy: f32, cx: &mut Context<Self>) {
        self.wheel_acc += dy;
        if self.wheel_pending {
            return;
        }
        self.wheel_pending = true;
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(std::time::Duration::from_millis(150)).await;
            let done = this.update(cx, |m, cx| {
                // 上一个 150ms 内还有滚轮事件就继续等:用累计值是否变化判断
                let settled = (m.wheel_acc - m.wheel_last).abs() < f32::EPSILON;
                m.wheel_last = m.wheel_acc;
                if !settled {
                    return false;
                }
                let net = m.wheel_acc.abs();
                m.wheel_acc = 0.;
                m.wheel_last = 0.;
                m.wheel_pending = false;
                let near = net < 40.;
                m.scroller.update(cx, |s, cx| {
                    if near && s.is_scrolled_up() {
                        s.scroll_to_end(cx);
                    }
                });
                true
            });
            if done.unwrap_or(true) {
                break;
            }
        })
        .detach();
    }

    /// 整体重新测量(主题或字号变化后调用)。
    pub fn remeasure_all(&mut self, cx: &mut Context<Self>) {
        self.scroller.update(cx, |s, cx| s.remeasure(cx));
    }
}

/// 会话向工作台发出的界面请求(打开面板、文件等)。
#[derive(Debug, Clone)]
pub enum UiRequest {
    /// 打开右侧「改动」面板
    OpenChanges,
    /// 打开 Canvas 画布
    OpenCanvas(String),
    /// 一个回合结束(真实模式下用来刷新侧栏会话列表与 Git 分支)
    SessionSettled,
}

impl EventEmitter<UiRequest> for SessionModel {}

impl SessionModel {
    /// 提问向导:选择(单选替换,多选切换)。
    pub fn wizard_pick(&mut self, id: &str, q_idx: usize, n_questions: usize, label: &str, multi: bool, cx: &mut Context<Self>) {
        let draft = self.wizard_draft.entry(id.to_string()).or_insert_with(|| vec![Vec::new(); n_questions]);
        if draft.len() < n_questions {
            draft.resize(n_questions, Vec::new());
        }
        let slot = &mut draft[q_idx];
        if multi {
            if let Some(pos) = slot.iter().position(|l| l == label) {
                slot.remove(pos);
            } else {
                slot.push(label.to_string());
            }
        } else {
            *slot = vec![label.to_string()];
        }
        self.remeasure_id(id, cx);
    }

    /// 提问向导:跳到指定题。
    pub fn wizard_go(&mut self, id: &str, step: usize, cx: &mut Context<Self>) {
        self.wizard_step.insert(id.to_string(), step);
        self.remeasure_id(id, cx);
    }

    /// 提问向导:提交全部答案。
    pub fn wizard_submit(&mut self, id: &str, n_questions: usize, cx: &mut Context<Self>) {
        let mut answers = self.wizard_draft.get(id).cloned().unwrap_or_default();
        answers.resize(n_questions, Vec::new());
        self.answer_question(id, answers, cx);
    }
}
