//! 会话模型:把后端事件泵进 `SessionState`,并驱动时间线的虚拟列表。

use crate::chat_scroll::ChatScroll;
use gpui_kit::*;
use std::collections::HashSet;
use loom_core::*;

/// 一个活动会话。界面通过它读取状态、发送命令。
pub struct SessionModel {
    pub state: SessionState,
    pub handle: BackendHandle,
    pub scroller: Entity<ChatScroll>,
    /// 已展开的条目 id(工具卡、思考、子 Agent、计划等)
    pub expanded: HashSet<String>,
    /// 运行中排队的消息(回合结束后依次发送)
    pub queued: Vec<String>,
    /// 提问向导当前所在题号(按提问 id)
    pub wizard_step: std::collections::HashMap<String, usize>,
    /// 提问向导已选答案草稿(按提问 id → 每题的已选项)
    pub wizard_draft: std::collections::HashMap<String, Vec<Vec<String>>>,
    /// 提问向导每题的「其他」输入框(按提问 id)与键盘焦点句柄,见 `wizard_input`
    pub wizard_inputs: std::collections::HashMap<String, Vec<Entity<gpui_kit::component::input::InputState>>>,
    pub wizard_focus: Option<FocusHandle>,
    /// 一次滚轮手势里累计的垂直位移与防抖任务是否已排队(用来吸附误触的微小滚动)
    /// 显示行(连续工具调用折成一行)与「条目下标 → 行下标」映射
    pub rows: Vec<crate::row_groups::Row>,
    /// 刚点过「复制」的助手条目 id(1.5 秒后清掉,用来显示「已复制」)
    pub copied: Option<String>,
    /// 思考条目的计时:id → (开始, 结束)。只有实时看到的思考才有时长,历史回放没有
    /// 当前回合开始时间(运行中才有);界面用它显示「12s · 按 Esc 中断」
    pub turn_started: Option<std::time::Instant>,
    pub thinking_times: std::collections::HashMap<String, (std::time::Instant, Option<std::time::Instant>)>,
    pub item_row: Vec<usize>,
    /// 回合结束时间:用户消息 id → 结束时刻(只有实时看到的回合才有;回复下方的操作条显示「N 分钟前」)
    pub turn_finished: std::collections::HashMap<String, std::time::SystemTime>,
    /// 正在就地编辑的用户消息(见 `user_message.rs`)
    pub editing: Option<crate::user_message::UserEdit>,
    pub wheel_acc: f32,
    pub wheel_pending: bool,
    wheel_last: f32,
    /// 输入框上方的后台任务列表是否展开
    /// 任务坞当前展开的是哪一类胶囊的列表(`None` 为全部收起)
    pub tasks_open: Option<TaskGroup>,
    /// 刚被「点外部」收起的时刻与当时展开的类别,用来让同一次点击落在胶囊上时不会把它重新打开
    pub tasks_outside_closed: Option<(std::time::Instant, TaskGroup)>,
    /// 是否已启动「有任务在跑就每秒刷新一次」的计时(用来刷新用时与停滞提示)
    task_ticking: bool,
}

impl SessionModel {
    /// 创建会话并启动事件泵。
    pub fn new(handle: BackendHandle, cx: &mut Context<Self>) -> Self {
        let scroller = cx.new(|cx| ChatScroll::new(0, cx));
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
            wizard_inputs: Default::default(),
            wizard_focus: None,
            rows: Vec::new(),
            copied: None,
            turn_started: None,
            thinking_times: Default::default(),
            item_row: Vec::new(),
            turn_finished: Default::default(),
            editing: None,
            wheel_acc: 0.,
            wheel_pending: false,
            wheel_last: 0.,
            // 调试用:LOOM_TASKS=1 启动即展开任务坞,方便截图
            tasks_open: std::env::var("LOOM_TASKS").ok().map(|v| task_group_from_key(&v)),
            tasks_outside_closed: None,
            task_ticking: false,
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
        self.track_thinking();
        self.track_turn(cx);
        self.track_tasks(cx);
        // 回合结束:最后一条助手消息下面会出现操作条,行高变了
        if turn_ended {
            if let Some(ix) = self.state.items.iter().rposition(|i| matches!(i, TimelineItem::Assistant { .. })) {
                dirty_rows.push(ix);
            }
        }
        // 重新算显示行;只有尾部会变,把变化的那一段整体替换,并重新测量内容变了的行
        let old_rows = std::mem::take(&mut self.rows);
        self.rows = crate::row_groups::build_rows(&self.state.items, self.state.phase == Phase::Running, Self::density(cx), &self.state.durations);
        self.item_row = crate::row_groups::item_to_row(&self.rows, self.state.items.len());
        let common = old_rows.iter().zip(&self.rows).take_while(|(a, b)| a == b).count();
        let dirty: Vec<usize> = {
            let mut v: Vec<usize> = dirty_rows.iter().filter_map(|i| self.item_row.get(*i).copied()).filter(|r| *r < common).collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        let (old_len, new_len) = (old_rows.len(), self.rows.len());
        self.scroller.update(cx, |s, cx| {
            if old_len != common || new_len != common {
                s.splice(common..old_len, new_len - common, cx);
            }
            for row in dirty {
                s.remeasure_items(row..row + 1, cx);
            }
            // 流式输出时最后一行内容在变,每批重新测量一次
            if common > 0 && common == old_len && common == new_len {
                s.remeasure_items(new_len - 1..new_len, cx);
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

    /// 当前的工具调用详细程度。
    fn density(cx: &App) -> loom_config::ToolDensity {
        cx.try_global::<crate::UiPrefs>().map_or_else(Default::default, |p| p.tool_density)
    }

    /// 详细程度变了:按新档位重算显示行,并重置虚拟列表。
    pub fn rebuild_rows(&mut self, cx: &mut Context<Self>) {
        self.rows = crate::row_groups::build_rows(&self.state.items, self.state.phase == Phase::Running, Self::density(cx), &self.state.durations);
        self.item_row = crate::row_groups::item_to_row(&self.rows, self.state.items.len());
        let n = self.rows.len();
        self.scroller.update(cx, |s, cx| s.reset(n, cx));
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
        self.rows = crate::row_groups::build_rows(&self.state.items, self.state.phase == Phase::Running, Self::density(cx), &self.state.durations);
        self.item_row = crate::row_groups::item_to_row(&self.rows, self.state.items.len());
        let n = self.rows.len();
        self.scroller.update(cx, |s, cx| s.reset(n, cx));
        cx.notify();
    }

    /// 发送提示。运行中会作为「引导」消息追加。
    pub fn send_prompt(&mut self, text: String, cx: &mut Context<Self>) {
        let steer = self.state.phase == Phase::Running;
        self.handle.send(AgentCommand::Prompt { text, images: vec![], files: vec![], steer });
        self.mark_running(cx);
    }

    /// 带图片与文件引用发送。
    pub fn send_prompt_with(&mut self, text: String, images: Vec<String>, files: Vec<String>, cx: &mut Context<Self>) {
        let steer = self.state.phase == Phase::Running;
        self.handle.send(AgentCommand::Prompt { text, images, files, steer });
        self.mark_running(cx);
    }

    /// 发送后立刻进入运行态:惰性后端要先启动 `claude` 子进程(可能要十几秒)才会回「回合开始」,
    /// 这段时间里界面必须已显示「处理中」与停止按钮,不能像卡住一样空白。
    /// 后端随后的 `TurnStarted` 是幂等的;启动失败会回 `TurnEnded`,自然恢复。
    fn mark_running(&mut self, cx: &mut Context<Self>) {
        if self.state.phase != Phase::Running {
            self.on_events(vec![AgentEvent::TurnStarted], cx);
        } else {
            cx.notify();
        }
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

    /// 处理计划:批准时 `mode` 是之后要用的权限模式(`None` 沿用 claude 的建议)。
    pub fn resolve_plan(&mut self, id: &str, approve: bool, mode: Option<PermissionMode>, cx: &mut Context<Self>) {
        self.handle.send(AgentCommand::ResolvePlan { id: id.into(), approve, mode });
        self.remeasure_id(id, cx);
    }

    /// 展开 / 折叠一个条目,并重新测量它所在的行。
    pub fn toggle(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.expanded.remove(id) {
            self.expanded.insert(id.to_string());
        }
        // 点开 / 收起是用户的原地操作:暂停贴底跟随,让内容从原位置往下长
        self.scroller.update(cx, |s, _| s.pause_following());
        self.remeasure_id(id, cx);
    }

    pub fn is_expanded(&self, id: &str) -> bool {
        self.expanded.contains(id)
    }

    pub(crate) fn remeasure_id(&mut self, id: &str, cx: &mut Context<Self>) {
        // 折叠组的 key 是「grp:首个工具 id」
        let id = id.strip_prefix("grp:").or_else(|| id.strip_prefix("dg:")).or_else(|| id.strip_prefix("ag:")).unwrap_or(id);
        if let Some(row) = self.state.row_of(id).and_then(|i| self.item_row.get(i).copied()) {
            self.scroller.update(cx, |s, cx| {
                s.remeasure_items(row..row + 1, cx);
            });
        }
        cx.notify();
    }

    /// 记下刚复制了哪条助手消息,1.5 秒后恢复按钮文案。
    ///
    /// 参数:`id` 助手条目 id。
    pub fn mark_copied(&mut self, id: &str, cx: &mut Context<Self>) {
        self.copied = Some(id.to_string());
        let id = id.to_string();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(1500)).await;
            let _ = this.update(cx, |m, cx| {
                if m.copied.as_deref() == Some(id.as_str()) {
                    m.copied = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// 跟踪回合计时:开始运行时记下时间并启动每秒一次的刷新,结束时清掉。
    fn track_turn(&mut self, cx: &mut Context<Self>) {
        let running = self.state.phase == Phase::Running;
        if running && self.turn_started.is_none() {
            self.turn_started = Some(std::time::Instant::now());
            cx.spawn(async move |this, cx| loop {
                cx.background_executor().timer(std::time::Duration::from_secs(1)).await;
                let alive = this.update(cx, |m, cx| {
                    let on = m.turn_started.is_some();
                    if on {
                        cx.notify();
                    }
                    on
                });
                if !alive.unwrap_or(false) {
                    break;
                }
            })
            .detach();
        } else if !running {
            // 回合结束:把用时记在这个回合的用户消息上(「已工作 N」用它)
            if let Some(start) = self.turn_started.take() {
                let secs = start.elapsed().as_secs();
                if secs >= 1 && let Some(TimelineItem::User { id, .. }) = self.state.items.iter().rev().find(|i| matches!(i, TimelineItem::User { .. })) {
                    self.state.durations.insert(id.clone(), secs);
                }
                if let Some(TimelineItem::User { id, .. }) = self.state.items.iter().rev().find(|i| matches!(i, TimelineItem::User { .. })) {
                    self.turn_finished.insert(id.clone(), std::time::SystemTime::now());
                }
            }
        }
    }

    /// 有后台任务或子 Agent 在运行时,每秒刷新一次界面(用时、停滞提示随之更新);都结束后自动停下。
    fn track_tasks(&mut self, cx: &mut Context<Self>) {
        let busy = self.has_live_work();
        if busy && !self.task_ticking {
            self.task_ticking = true;
            cx.spawn(async move |this, cx| loop {
                cx.background_executor().timer(std::time::Duration::from_secs(1)).await;
                let alive = this.update(cx, |m, cx| {
                    let on = m.has_live_work();
                    if on {
                        cx.notify();
                    } else {
                        m.task_ticking = false;
                    }
                    on
                });
                if !alive.unwrap_or(false) {
                    break;
                }
            })
            .detach();
        }
    }

    /// 向官方接口查询账户用量与限额(只在用户点开上下文弹层时调用,不自动轮询)。
    pub fn refresh_usage(&mut self) {
        self.handle.send(AgentCommand::RefreshUsage);
    }

    /// 是否有仍在运行的后台任务或子 Agent。
    pub fn has_live_work(&self) -> bool {
        self.state.running_task_count() > 0 || self.state.items.iter().any(|i| matches!(i, TimelineItem::Subagent(c) if c.is_active()))
    }

    /// 停止一个后台任务(命令、监听、子 Agent 都用同一条命令)。
    pub fn stop_task(&mut self, task_id: &str, cx: &mut Context<Self>) {
        self.handle.send(AgentCommand::StopTask { task_id: task_id.into() });
        cx.notify();
    }

    /// 点某一类任务胶囊:展开它的列表;已展开则收起;展开着别的类别则切换过去。
    /// 刚被「点外部」收起的同一次点击,点到同一类胶囊不重开,点到别的类胶囊则切换。
    ///
    /// 参数:`group` 被点的类别。
    pub fn toggle_tasks(&mut self, group: TaskGroup, cx: &mut Context<Self>) {
        let just_closed = self.tasks_outside_closed.take().filter(|(t, _)| t.elapsed() < std::time::Duration::from_millis(250)).map(|(_, g)| g);
        self.tasks_open = next_tasks_open(self.tasks_open, group, just_closed);
        cx.notify();
    }

    /// 点任务列表以外的地方:收起列表。
    pub fn dismiss_tasks_outside(&mut self, cx: &mut Context<Self>) {
        if let Some(g) = self.tasks_open.take() {
            self.tasks_outside_closed = Some((std::time::Instant::now(), g));
            cx.notify();
        }
    }

    /// 收起任务列表(Esc)。返回之前是否是展开的。
    pub fn close_tasks(&mut self, cx: &mut Context<Self>) -> bool {
        let was = self.tasks_open.take().is_some();
        if was {
            cx.notify();
        }
        was
    }

    /// 思考用时(秒);没有实时计时(历史回放)或还没结束返回 `None`。
    pub fn thinking_secs(&self, id: &str) -> Option<u64> {
        // 实时看到的思考用本地计时;历史回放用时间戳算出的用时
        let live = self.thinking_times.get(id).and_then(|(start, end)| end.map(|e| e.duration_since(*start).as_secs()));
        live.or_else(|| self.state.durations.get(id).copied())
    }

    /// 更新末尾几个思考条目的计时:开始流式记开始,结束流式记结束。
    fn track_thinking(&mut self) {
        let now = std::time::Instant::now();
        let n = self.state.items.len();
        for item in &self.state.items[n.saturating_sub(4)..] {
            if let TimelineItem::Thinking { id, streaming, .. } = item {
                let e = self.thinking_times.entry(id.clone()).or_insert((now, None));
                if !*streaming && e.1.is_none() {
                    e.1 = Some(now);
                }
            }
        }
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
                    if near && s.is_away_from_bottom() {
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

/// 点任务坞某类胶囊后,展开的是哪一类。
///
/// 参数:`current` 当前展开的类别;`clicked` 被点的类别;`just_closed` 刚被点外部收起时当时展开的类别。
/// 返回:新的展开类别(`None` 为收起)。
pub fn next_tasks_open(current: Option<TaskGroup>, clicked: TaskGroup, just_closed: Option<TaskGroup>) -> Option<TaskGroup> {
    if let Some(prev) = just_closed {
        return (prev != clicked).then_some(clicked);
    }
    (current != Some(clicked)).then_some(clicked)
}

/// 环境变量 `LOOM_TASKS` 的取值转类别(`agent` / `shell` / `monitor` / `workflow` / `other` / `finished`,其余取子代理)。
pub fn task_group_from_key(key: &str) -> TaskGroup {
    match key {
        "shell" => TaskGroup::Shell,
        "monitor" => TaskGroup::Monitor,
        "workflow" => TaskGroup::Workflow,
        "other" => TaskGroup::Other,
        "finished" => TaskGroup::Finished,
        _ => TaskGroup::Agent,
    }
}

/// 会话向工作台发出的界面请求(打开面板、文件等)。
#[derive(Debug, Clone)]
pub enum UiRequest {
    /// 打开右侧「改动」面板
    OpenChanges,
    /// 打开右侧「改动」面板并只看某个文件的 Diff
    OpenChangeFile(String),
    /// 打开 Canvas 画布
    OpenCanvas(String),
    /// 一个回合结束(真实模式下用来刷新侧栏会话列表与 Git 分支)
    SessionSettled,
    /// 在右侧标签里打开某个子 Agent 的完整对话(条目 id)
    OpenSubagent(String),
    /// 在右侧「文件」里预览某个文件
    OpenFile(String),
    /// 打开右侧「任务」标签,并选中某个任务(`None` 只打开)
    OpenTasks(Option<String>),
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
}

#[cfg(test)]
mod tasks_dock_tests {
    use super::*;
    use TaskGroup::*;

    #[::core::prelude::v1::test]
    fn 点胶囊展开_再点收起() {
        assert_eq!(next_tasks_open(None, Agent, None), Some(Agent));
        assert_eq!(next_tasks_open(Some(Agent), Agent, None), None);
    }

    #[::core::prelude::v1::test]
    fn 点另一类胶囊直接切换() {
        assert_eq!(next_tasks_open(Some(Agent), Shell, None), Some(Shell));
    }

    #[::core::prelude::v1::test]
    fn 点外部刚收起时_同类不重开_异类切换() {
        assert_eq!(next_tasks_open(None, Agent, Some(Agent)), None);
        assert_eq!(next_tasks_open(None, Shell, Some(Agent)), Some(Shell));
    }

    #[::core::prelude::v1::test]
    fn 环境变量取值() {
        assert_eq!(task_group_from_key("1"), Agent);
        assert_eq!(task_group_from_key("shell"), Shell);
        assert_eq!(task_group_from_key("finished"), Finished);
    }
}
