//! 会话状态:对 `AgentEvent` 的纯函数式归约。

use crate::event::{AgentEvent, StopReason, Usage};
use crate::task::*;
use crate::modes::{Effort, ModelInfo, PermissionMode, default_models};
use crate::timeline::*;

/// 会话运行阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    /// 尚未就绪(启动中)
    #[default]
    Starting,
    /// 空闲,可以发送
    Idle,
    /// 回合进行中
    Running,
}

/// 一个会话的全部可展示状态。
#[derive(Debug, Clone)]
pub struct SessionState {
    pub session_id: Option<String>,
    pub title: String,
    pub phase: Phase,
    pub items: Vec<TimelineItem>,
    pub models: Vec<ModelInfo>,
    pub model: String,
    pub mode: PermissionMode,
    pub effort: Effort,
    pub usage: Usage,
    /// 最近一次回合的结束原因
    pub last_stop: Option<StopReason>,
    /// 条目 id → 用时(秒):思考条目是这次思考的用时,用户消息条目是它引出的整个回合的用时
    pub durations: std::collections::HashMap<String, u64>,
    /// 后台任务表(按开始顺序);含已结束的,界面用 `counts_as_running` 过滤
    pub tasks: Vec<BackgroundTask>,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            session_id: None,
            title: "新会话".into(),
            phase: Phase::Starting,
            items: Vec::new(),
            models: default_models(),
            model: "default".into(),
            mode: PermissionMode::Default,
            effort: Effort::Medium,
            usage: Usage::default(),
            durations: Default::default(),
            tasks: Vec::new(),
            last_stop: None,
        }
    }
}

/// 在条目列表(可能嵌套在子 Agent 内)中定位目标列表。
fn target_list<'a>(items: &'a mut Vec<TimelineItem>, parent: &Option<String>) -> &'a mut Vec<TimelineItem> {
    // 先算出子 Agent 的下标,再统一返回,避免借用冲突
    let idx = parent.as_ref().and_then(|pid| {
        items
            .iter()
            .position(|i| matches!(i, TimelineItem::Subagent(s) if &s.id == pid))
    });
    match idx {
        Some(idx) => match &mut items[idx] {
            TimelineItem::Subagent(s) => &mut s.children,
            _ => unreachable!("下标由 position 在同一列表上得出"),
        },
        None => items,
    }
}

impl SessionState {
    /// 是否有等待用户操作的条目。
    pub fn awaiting_user(&self) -> bool {
        fn any(items: &[TimelineItem]) -> bool {
            items.iter().any(|i| match i {
                TimelineItem::Subagent(s) => any(&s.children),
                other => other.awaits_user(),
            })
        }
        any(&self.items)
    }

    /// 归约一个事件。
    pub fn apply(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::Ready { session_id, models, model, mode } => {
                self.session_id = Some(session_id);
                if !models.is_empty() {
                    self.models = models;
                }
                self.model = model;
                self.mode = mode;
                self.phase = Phase::Idle;
            }
            AgentEvent::TurnStarted => self.phase = Phase::Running,
            AgentEvent::Duration { id, secs } => {
                self.durations.insert(id, secs);
            }
            AgentEvent::Append { parent, mut item } => {
                // 子 Agent 卡片进入时间线时记录开始时间;嵌套条目的到来算作父卡片的一次活动
                if let TimelineItem::Subagent(c) = &mut item {
                    c.stamp_start(now_ms());
                }
                if let Some(p) = &parent {
                    self.touch(p);
                }
                target_list(&mut self.items, &parent).push(item);
            }
            AgentEvent::TextDelta { parent, id, delta } => {
                if let Some(p) = &parent {
                    self.touch(p);
                }
                let list = target_list(&mut self.items, &parent);
                match list.iter_mut().rev().find(|i| i.id() == id) {
                    Some(TimelineItem::Assistant { text, .. }) => text.push_str(&delta),
                    _ => list.push(TimelineItem::Assistant { id, text: delta, streaming: true }),
                }
            }
            AgentEvent::ThinkingDelta { parent, id, delta } => {
                let list = target_list(&mut self.items, &parent);
                match list.iter_mut().rev().find(|i| i.id() == id) {
                    Some(TimelineItem::Thinking { text, .. }) => text.push_str(&delta),
                    _ => list.push(TimelineItem::Thinking { id, text: delta, streaming: true }),
                }
            }
            AgentEvent::ToolUpdate { parent, id, patch } => {
                if let Some(p) = &parent {
                    self.touch(p);
                }
                let list = target_list(&mut self.items, &parent);
                for item in list.iter_mut().rev() {
                    match item {
                        TimelineItem::Tool(t) if t.id == id => {
                            t.apply(patch);
                            return;
                        }
                        TimelineItem::Permission(p) if p.tool.id == id => {
                            p.tool.apply(patch);
                            return;
                        }
                        _ => {}
                    }
                }
            }
            AgentEvent::ItemDone { parent, id } => {
                let list = target_list(&mut self.items, &parent);
                if let Some(item) = list.iter_mut().rev().find(|i| i.id() == id) {
                    match item {
                        TimelineItem::Assistant { streaming, .. } | TimelineItem::Thinking { streaming, .. } => {
                            *streaming = false
                        }
                        _ => {}
                    }
                }
            }
            AgentEvent::SubagentStatus { id, status } => {
                let now = now_ms();
                if let Some(TimelineItem::Subagent(s)) = self.find_mut(&id) {
                    s.status = status;
                    if s.is_active() {
                        s.stamp_start(now);
                        s.ended_ms = None;
                        s.background = status == SubagentStatus::Background;
                    } else {
                        s.ended_ms.get_or_insert(now);
                    }
                    s.last_activity_ms = Some(now);
                }
            }
            AgentEvent::SubagentFinished { id, status, summary, tokens, tool_count, duration_ms } => {
                let now = now_ms();
                if let Some(TimelineItem::Subagent(s)) = self.find_mut(&id) {
                    s.status = status;
                    s.ended_ms = Some(now);
                    if let Some(d) = duration_ms {
                        s.started_ms = Some(now.saturating_sub(d));
                    }
                    if tokens > 0 {
                        s.tokens = tokens;
                    }
                    if tool_count > 0 {
                        s.tool_count = tool_count;
                    }
                    if !summary.is_empty() {
                        s.summary = summary;
                    }
                }
            }
            AgentEvent::Todos { id, items } => match self.find_mut(&id) {
                Some(TimelineItem::Todos { items: old, .. }) => *old = items,
                _ => self.items.push(TimelineItem::Todos { id, items }),
            },
            AgentEvent::PromoteToPermission { parent, request } => {
                let list = target_list(&mut self.items, &parent);
                let pos = list.iter().rposition(|i| matches!(i, TimelineItem::Tool(t) if t.id == request.tool.id));
                match pos {
                    Some(p) => list[p] = TimelineItem::Permission(request),
                    None => list.push(TimelineItem::Permission(request)),
                }
            }
            AgentEvent::PermissionResolved { id, option } => {
                if let Some(TimelineItem::Permission(p)) = self.find_mut(&id) {
                    // 选了「拒绝」类选项:工具卡标记为被拒绝,与失败区分
                    let rejected = p
                        .options
                        .iter()
                        .find(|o| o.id == option)
                        .map(|o| matches!(o.kind, PermissionOptionKind::RejectOnce | PermissionOptionKind::RejectAlways))
                        .unwrap_or_else(|| option.contains("reject") || option.contains("deny"));
                    if rejected {
                        p.tool.status = crate::tool::ToolStatus::Denied;
                    }
                    p.resolved = Some(option);
                }
            }
            AgentEvent::QuestionAnswered { id, answers } => {
                if let Some(TimelineItem::Question(q)) = self.find_mut(&id) {
                    q.answers = Some(answers);
                }
            }
            AgentEvent::PlanResolved { id, approved } => {
                if let Some(TimelineItem::PlanApproval(p)) = self.find_mut(&id) {
                    p.approved = Some(approved);
                }
            }
            AgentEvent::Usage(u) => self.usage = u,
            AgentEvent::ModeChanged(m) => self.mode = m,
            AgentEvent::ModelChanged(m) => self.model = m,
            AgentEvent::TitleChanged(t) => self.title = t,
            AgentEvent::Notice { level, text } => {
                let id = format!("notice-{}", self.items.len());
                self.items.push(TimelineItem::Notice { id, level, text });
            }
            AgentEvent::TaskStarted(task) => self.task_started(task),
            AgentEvent::TaskProgress { task_id, tokens, tool_uses, last_tool, summary } => {
                let now = now_ms();
                if let Some(t) = self.task_mut(&task_id) {
                    if let Some(v) = tokens {
                        t.tokens = v;
                    }
                    if let Some(v) = tool_uses {
                        t.tool_uses = v;
                    }
                    if let Some(v) = last_tool.filter(|v| !v.is_empty()) {
                        t.last_tool = v;
                    }
                    if let Some(v) = summary.filter(|v| !v.is_empty()) {
                        t.summary = v;
                    }
                    t.last_activity_ms = now;
                    let snapshot = t.clone();
                    self.sync_card(&snapshot);
                }
            }
            AgentEvent::TaskUpdated { task_id, status, description, output_file } => {
                let now = now_ms();
                if let Some(t) = self.task_mut(&task_id) {
                    if let Some(f) = output_file.filter(|f| !f.is_empty()) {
                        t.output_file = Some(f);
                    }
                    if let Some(d) = description.filter(|d| !d.is_empty()) {
                        t.description = d;
                    }
                    if let Some(st) = status {
                        t.status = st;
                        if st.is_finished() && t.ended_ms.is_none() {
                            t.ended_ms = Some(now);
                        }
                    }
                    t.last_activity_ms = now;
                    let snapshot = t.clone();
                    self.sync_card(&snapshot);
                }
            }
            AgentEvent::TaskNotification { task_id, status, summary, output_file } => self.task_notified(task_id, status, summary, output_file),
            AgentEvent::TasksChanged(list) => self.tasks_changed(list),
            AgentEvent::TaskOutput { task_id, text } => {
                if let Some(t) = self.task_mut(&task_id) {
                    t.push_output(&text);
                }
            }
            AgentEvent::TurnEnded(reason) => {
                self.phase = Phase::Idle;
                self.last_stop = Some(reason);
                self.finish_streaming(reason == StopReason::Cancelled);
            }
        }
    }

    /// 回合结束时收尾:停止所有流式条目;取消时把未完成的工具标为失败。
    fn finish_streaming(&mut self, cancelled: bool) {
        fn walk(items: &mut [TimelineItem], cancelled: bool) {
            for item in items {
                match item {
                    TimelineItem::Assistant { streaming, .. } | TimelineItem::Thinking { streaming, .. } => {
                        *streaming = false
                    }
                    TimelineItem::Tool(t) if cancelled && !t.status.is_finished() => {
                        t.status = crate::tool::ToolStatus::Interrupted
                    }
                    TimelineItem::Subagent(s) => {
                        // 只有前台子 Agent 会随回合一起被中断;后台的继续跑,等 task_notification 给出终态
                        if cancelled && !s.background && matches!(s.status, SubagentStatus::Starting | SubagentStatus::Running) {
                            s.status = SubagentStatus::Stopped;
                            s.ended_ms.get_or_insert_with(now_ms);
                        }
                        walk(&mut s.children, cancelled)
                    }
                    _ => {}
                }
            }
        }
        walk(&mut self.items, cancelled);
    }

    /// 记录子 Agent 卡片有了新活动(用于疑似停滞判断)。
    fn touch(&mut self, card_id: &str) {
        if let Some(TimelineItem::Subagent(c)) = self.find_mut(card_id) {
            c.last_activity_ms = Some(now_ms());
        }
    }

    // ---------- 后台任务 ----------

    fn task_mut(&mut self, id: &str) -> Option<&mut BackgroundTask> {
        self.tasks.iter_mut().find(|t| t.task_id == id)
    }

    /// 后台任务:按 id 查找。
    pub fn task(&self, id: &str) -> Option<&BackgroundTask> {
        self.tasks.iter().find(|t| t.task_id == id)
    }

    /// 正在后台运行的任务(不含环境任务与子 Agent 私有任务)。
    pub fn running_tasks(&self) -> impl Iterator<Item = &BackgroundTask> {
        self.tasks.iter().filter(|t| t.counts_as_running())
    }

    /// 正在后台运行的任务数,用于任务坞、状态栏与会话行的计数。
    pub fn running_task_count(&self) -> usize {
        self.running_tasks().count()
    }

    /// 把任务的进度同步到启动它的子 Agent 卡片与后台工具卡。
    fn sync_card(&mut self, task: &BackgroundTask) {
        let Some(tool_use) = task.tool_use_id.as_deref() else { return };
        match self.find_mut(tool_use) {
            Some(TimelineItem::Subagent(card)) => {
                card.task_id = Some(task.task_id.clone());
                card.background = task.backgrounded;
                card.tokens = task.tokens;
                if task.tool_uses > 0 {
                    card.tool_count = task.tool_uses;
                }
                if !task.last_tool.is_empty() {
                    card.last_tool = task.last_tool.clone();
                }
                if !task.summary.is_empty() {
                    card.summary = task.summary.clone();
                }
                card.last_activity_ms = Some(task.last_activity_ms);
                card.status = match task.status {
                    TaskStatus::Running if task.backgrounded => SubagentStatus::Background,
                    TaskStatus::Running => SubagentStatus::Running,
                    TaskStatus::Completed => SubagentStatus::Completed,
                    TaskStatus::Failed => SubagentStatus::Failed,
                    TaskStatus::Stopped => SubagentStatus::Stopped,
                };
                if task.status.is_finished() {
                    card.ended_ms = task.ended_ms.or_else(|| Some(now_ms()));
                } else {
                    card.ended_ms = None;
                }
            }
            Some(TimelineItem::Tool(tool)) => tool.task_id = Some(task.task_id.clone()),
            _ => {}
        }
    }

    fn task_started(&mut self, task: BackgroundTask) {
        let id = task.task_id.clone();
        match self.tasks.iter().position(|t| t.task_id == id) {
            // 同一个 task_id 再次启动(恢复运行):整体替换,保留输出
            Some(i) => {
                let output = std::mem::take(&mut self.tasks[i].output);
                self.tasks[i] = BackgroundTask { output, ..task };
            }
            None => self.tasks.push(task),
        }
        if let Some(snapshot) = self.task(&id).cloned() {
            self.sync_card(&snapshot);
        }
    }

    fn task_notified(&mut self, task_id: String, status: TaskStatus, summary: String, output_file: Option<String>) {
        let now = now_ms();
        let mut note_kind = TaskKind::Other;
        let mut visible = true;
        let mut was_finished = false;
        if let Some(t) = self.task_mut(&task_id) {
            was_finished = t.status.is_finished();
            t.status = status;
            t.ended_ms.get_or_insert(now);
            if !summary.is_empty() {
                t.summary = summary.clone();
            }
            if let Some(f) = output_file.filter(|f| !f.is_empty()) {
                t.output_file = Some(f);
            }
            note_kind = t.kind;
            visible = t.visible() && t.backgrounded;
            let snapshot = t.clone();
            self.sync_card(&snapshot);
        } else if summary.is_empty() {
            // 恢复会话时 claude 会为上个进程留下的任务补一条空的 stopped 通知,没有信息量
            return;
        }
        // 只有任务此前已是终态时才去重;恢复运行后的再次完成应当再出一条通知
        let dup = was_finished && self.items.iter().rev().take(6).any(|i| matches!(i, TimelineItem::TaskNote { task_id: t, status: s, .. } if *t == task_id && *s == status));
        if visible && !dup {
            let id = format!("tasknote-{task_id}-{}", self.items.len());
            let text = if summary.is_empty() { format!("{}{}", note_kind.label(), status.label()) } else { summary };
            self.items.push(TimelineItem::TaskNote { id, task_id, kind: note_kind, status, text });
        }
    }

    fn tasks_changed(&mut self, list: Vec<BackgroundTask>) {
        let now = now_ms();
        let live: Vec<String> = list.iter().map(|t| t.task_id.clone()).collect();
        for incoming in list {
            match self.tasks.iter().position(|t| t.task_id == incoming.task_id) {
                Some(i) => {
                    // 快照只带描述与标记,进度、输出沿用已有的
                    let old = &mut self.tasks[i];
                    old.description = if incoming.description.is_empty() { old.description.clone() } else { incoming.description };
                    old.ambient = incoming.ambient;
                    old.backgrounded = incoming.backgrounded;
                    old.owned_by_subagent = incoming.owned_by_subagent;
                    if incoming.run_id.is_some() {
                        old.run_id = incoming.run_id;
                    }
                    if incoming.tool_use_id.is_some() {
                        old.tool_use_id = incoming.tool_use_id;
                    }
                    if old.status.is_finished() && incoming.status == TaskStatus::Running {
                        // 已结束的任务又出现在存活集合里:恢复运行
                        old.status = TaskStatus::Running;
                        old.ended_ms = None;
                    }
                    let snapshot = old.clone();
                    self.sync_card(&snapshot);
                }
                None => {
                    let snapshot = incoming.clone();
                    self.tasks.push(incoming);
                    self.sync_card(&snapshot);
                }
            }
        }
        // 快照里没有、但还在后台跑的任务:已经结束(通知漏了也能被纠正)
        let mut ended = Vec::new();
        for t in self.tasks.iter_mut() {
            if t.backgrounded && t.status == TaskStatus::Running && !live.contains(&t.task_id) {
                t.status = TaskStatus::Stopped;
                t.ended_ms = Some(now);
                ended.push(t.clone());
            }
        }
        for t in ended {
            self.sync_card(&t);
        }
    }

    /// 返回包含指定 id 的顶层行下标(含子 Agent 内部的条目),用于局部重新测量行高。
    pub fn row_of(&self, id: &str) -> Option<usize> {
        fn contains(item: &TimelineItem, id: &str) -> bool {
            if item.id() == id {
                return true;
            }
            match item {
                TimelineItem::Subagent(s) => s.children.iter().any(|c| contains(c, id)),
                TimelineItem::Permission(p) => p.tool.id == id,
                _ => false,
            }
        }
        self.items.iter().position(|i| contains(i, id))
    }

    /// 递归按 id 查找条目(含子 Agent 内部)。
    pub fn find_mut(&mut self, id: &str) -> Option<&mut TimelineItem> {
        fn find<'a>(items: &'a mut [TimelineItem], id: &str) -> Option<&'a mut TimelineItem> {
            for item in items.iter_mut() {
                if item.id() == id {
                    return Some(item);
                }
                if let TimelineItem::Subagent(s) = item {
                    if let Some(found) = find(&mut s.children, id) {
                        return Some(found);
                    }
                }
            }
            None
        }
        find(&mut self.items, id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::{ToolCall, ToolPatch, ToolStatus};

    #[test]
    fn 文本增量合并为同一条目() {
        let mut s = SessionState::default();
        for d in ["你", "好"] {
            s.apply(AgentEvent::TextDelta { parent: None, id: "a1".into(), delta: d.into() });
        }
        assert_eq!(s.items.len(), 1);
        assert!(matches!(&s.items[0], TimelineItem::Assistant { text, streaming: true, .. } if text == "你好"));
    }

    #[test]
    fn 子_agent_内部事件进入嵌套时间线() {
        let mut s = SessionState::default();
        s.apply(AgentEvent::Append {
            parent: None,
            item: TimelineItem::Subagent(SubagentCard::new("sa", "Explore", "调研", SubagentStatus::Running)),
        });
        s.apply(AgentEvent::Append {
            parent: Some("sa".into()),
            item: TimelineItem::Tool(ToolCall::new("t1", "Read", "读取")),
        });
        s.apply(AgentEvent::ToolUpdate {
            parent: Some("sa".into()),
            id: "t1".into(),
            patch: ToolPatch { status: Some(ToolStatus::Completed), ..Default::default() },
        });
        let TimelineItem::Subagent(card) = &s.items[0] else { panic!() };
        assert!(matches!(&card.children[0], TimelineItem::Tool(t) if t.status == ToolStatus::Completed));
    }

    #[test]
    fn 工具卡升级为权限请求() {
        let mut s = SessionState::default();
        s.apply(AgentEvent::Append { parent: None, item: TimelineItem::Tool(ToolCall::new("t1", "Bash", "ls")) });
        s.apply(AgentEvent::PromoteToPermission {
            parent: None,
            request: PermissionRequest { id: "t1".into(), tool: ToolCall::new("t1", "Bash", "ls"), options: vec![], resolved: None },
        });
        assert_eq!(s.items.len(), 1);
        assert!(matches!(&s.items[0], TimelineItem::Permission(_)));
        assert!(s.awaiting_user());
    }

    #[test]
    fn 取消回合把运行中的工具标为被中断() {
        let mut s = SessionState::default();
        s.apply(AgentEvent::TurnStarted);
        s.apply(AgentEvent::Append { parent: None, item: TimelineItem::Tool(ToolCall::new("t", "Bash", "x")) });
        s.apply(AgentEvent::TurnEnded(StopReason::Cancelled));
        assert_eq!(s.phase, Phase::Idle);
        assert!(matches!(&s.items[0], TimelineItem::Tool(t) if t.status == ToolStatus::Interrupted));
    }

    fn sub(id: &str, status: SubagentStatus) -> AgentEvent {
        AgentEvent::Append { parent: None, item: TimelineItem::Subagent(SubagentCard::new(id, "Explore", "调研", status)) }
    }

    #[test]
    fn 取消回合不误杀后台子代理() {
        let mut s = SessionState::default();
        s.apply(sub("fg", SubagentStatus::Running));
        s.apply(sub("bg", SubagentStatus::Running));
        s.apply(AgentEvent::TaskStarted(BackgroundTask::new("t-bg", TaskKind::Agent, "后台").with_tool_use("bg")));
        s.apply(AgentEvent::TurnEnded(StopReason::Cancelled));
        let get = |s: &SessionState, id: &str| match s.items.iter().find(|i| i.id() == id) {
            Some(TimelineItem::Subagent(c)) => c.clone(),
            _ => panic!(),
        };
        assert_eq!(get(&s, "fg").status, SubagentStatus::Stopped);
        assert_eq!(get(&s, "bg").status, SubagentStatus::Background);
        assert_eq!(s.running_task_count(), 1);
    }

    #[test]
    fn 任务启动进度与通知同步到子代理卡片() {
        let mut s = SessionState::default();
        s.apply(sub("a1", SubagentStatus::Running));
        s.apply(AgentEvent::TaskStarted(BackgroundTask::new("t1", TaskKind::Agent, "调研").with_tool_use("a1")));
        s.apply(AgentEvent::TaskProgress { task_id: "t1".into(), tokens: Some(5200), tool_uses: Some(3), last_tool: Some("Grep".into()), summary: Some("正在搜索".into()) });
        let TimelineItem::Subagent(c) = &s.items[0] else { panic!() };
        assert_eq!((c.status, c.tokens, c.tool_count, c.last_tool.as_str(), c.summary.as_str()), (SubagentStatus::Background, 5200, 3, "Grep", "正在搜索"));
        s.apply(AgentEvent::TaskNotification { task_id: "t1".into(), status: TaskStatus::Completed, summary: "调研完成".into(), output_file: None });
        let TimelineItem::Subagent(c) = &s.items[0] else { panic!() };
        assert_eq!(c.status, SubagentStatus::Completed);
        assert!(c.ended_ms.is_some());
        assert_eq!(s.running_task_count(), 0);
        // 完成时在时间线追加一条通知,重复通知不重复追加
        s.apply(AgentEvent::TaskNotification { task_id: "t1".into(), status: TaskStatus::Completed, summary: "调研完成".into(), output_file: None });
        assert_eq!(s.items.iter().filter(|i| matches!(i, TimelineItem::TaskNote { .. })).count(), 1);
    }

    #[test]
    fn 后台工具卡关联任务() {
        let mut s = SessionState::default();
        s.apply(AgentEvent::Append { parent: None, item: TimelineItem::Tool(ToolCall::new("b1", "Bash", "启动服务")) });
        s.apply(AgentEvent::TaskStarted(BackgroundTask::new("sh1", TaskKind::Shell, "启动服务").with_tool_use("b1")));
        assert!(matches!(&s.items[0], TimelineItem::Tool(t) if t.task_id.as_deref() == Some("sh1")));
    }

    #[test]
    fn 快照替换缓存且环境任务与子代理私有任务不计数() {
        let mut s = SessionState::default();
        s.apply(AgentEvent::TaskStarted(BackgroundTask::new("a", TaskKind::Shell, "A")));
        s.apply(AgentEvent::TaskStarted(BackgroundTask::new("b", TaskKind::Shell, "B")));
        assert_eq!(s.running_task_count(), 2);
        // 快照里没有 a:视为已结束,漏掉通知也能被纠正
        let mut amb = BackgroundTask::new("amb", TaskKind::Other, "内部");
        amb.ambient = true;
        let mut own = BackgroundTask::new("own", TaskKind::Shell, "子代理自己的命令");
        own.owned_by_subagent = true;
        s.apply(AgentEvent::TasksChanged(vec![BackgroundTask::new("b", TaskKind::Shell, "B"), amb, own]));
        assert_eq!(s.running_task_count(), 1);
        assert_eq!(s.task("a").map(|t| t.status), Some(TaskStatus::Stopped));
    }

    #[test]
    fn 恢复运行复用任务编号并保留输出() {
        let mut s = SessionState::default();
        s.apply(AgentEvent::TaskStarted(BackgroundTask::new("t", TaskKind::Agent, "第一次")));
        s.apply(AgentEvent::TaskOutput { task_id: "t".into(), text: "hello".into() });
        s.apply(AgentEvent::TaskNotification { task_id: "t".into(), status: TaskStatus::Completed, summary: "完".into(), output_file: None });
        s.apply(AgentEvent::TaskStarted(BackgroundTask::new("t", TaskKind::Agent, "第二次")));
        assert_eq!(s.tasks.len(), 1);
        assert_eq!(s.tasks[0].status, TaskStatus::Running);
        assert_eq!(s.tasks[0].output, "hello");
    }

    #[test]
    fn 拒绝权限把工具标为被拒绝() {
        let mut s = SessionState::default();
        let opts = vec![
            PermissionOption { id: "allow_once".into(), label: "允许".into(), kind: PermissionOptionKind::AllowOnce },
            PermissionOption { id: "reject".into(), label: "拒绝".into(), kind: PermissionOptionKind::RejectOnce },
        ];
        s.apply(AgentEvent::Append {
            parent: None,
            item: TimelineItem::Permission(PermissionRequest { id: "p".into(), tool: ToolCall::new("p", "Bash", "rm"), options: opts, resolved: None }),
        });
        s.apply(AgentEvent::PermissionResolved { id: "p".into(), option: "reject".into() });
        assert!(matches!(&s.items[0], TimelineItem::Permission(p) if p.tool.status == ToolStatus::Denied));
    }

    #[test]
    fn 等待用户状态可检测() {
        let mut s = SessionState::default();
        s.apply(AgentEvent::Append {
            parent: None,
            item: TimelineItem::PlanApproval(PlanApproval { id: "p".into(), markdown: "# x".into(), approved: None }),
        });
        assert!(s.awaiting_user());
        s.apply(AgentEvent::PlanResolved { id: "p".into(), approved: true });
        assert!(!s.awaiting_user());
    }
}
