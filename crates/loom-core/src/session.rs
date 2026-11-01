//! 会话状态:对 `AgentEvent` 的纯函数式归约。

use crate::event::{AgentEvent, StopReason, Usage};
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
            AgentEvent::Append { parent, item } => {
                target_list(&mut self.items, &parent).push(item);
            }
            AgentEvent::TextDelta { parent, id, delta } => {
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
                if let Some(TimelineItem::Subagent(s)) = self.find_mut(&id) {
                    s.status = status;
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
                        t.status = crate::tool::ToolStatus::Failed
                    }
                    TimelineItem::Subagent(s) => {
                        if cancelled && matches!(s.status, SubagentStatus::Starting | SubagentStatus::Running) {
                            s.status = SubagentStatus::Failed;
                        }
                        walk(&mut s.children, cancelled)
                    }
                    _ => {}
                }
            }
        }
        walk(&mut self.items, cancelled);
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
            item: TimelineItem::Subagent(SubagentCard {
                id: "sa".into(),
                agent_type: "Explore".into(),
                description: "调研".into(),
                status: SubagentStatus::Running,
                children: vec![],
            }),
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
    fn 取消回合把运行中的工具标为失败() {
        let mut s = SessionState::default();
        s.apply(AgentEvent::TurnStarted);
        s.apply(AgentEvent::Append { parent: None, item: TimelineItem::Tool(ToolCall::new("t", "Bash", "x")) });
        s.apply(AgentEvent::TurnEnded(StopReason::Cancelled));
        assert_eq!(s.phase, Phase::Idle);
        assert!(matches!(&s.items[0], TimelineItem::Tool(t) if t.status == ToolStatus::Failed));
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
