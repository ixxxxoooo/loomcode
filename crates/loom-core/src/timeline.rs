//! 会话时间线条目。

use crate::task::{TaskKind, TaskStatus};
use crate::tool::ToolCall;
use serde::{Deserialize, Serialize};

/// 提示级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NoticeLevel {
    Info,
    Warning,
    Error,
}

/// 待办状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

/// 一条待办。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TodoItem {
    pub content: String,
    pub status: TodoStatus,
}

/// 权限选项的性质。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionOptionKind {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
}

/// 权限选项。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PermissionOption {
    pub id: String,
    pub label: String,
    pub kind: PermissionOptionKind,
}

/// 一次权限请求及其结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub id: String,
    pub tool: ToolCall,
    pub options: Vec<PermissionOption>,
    /// 已选择的选项 id;`None` 表示仍在等待
    pub resolved: Option<String>,
}

/// 提问的候选项。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
}

/// 单个问题(AskUserQuestion 的一题)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Question {
    pub header: String,
    pub question: String,
    pub options: Vec<QuestionOption>,
    pub multi_select: bool,
}

/// 一组提问(向导)及回答。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestionRequest {
    pub id: String,
    pub questions: Vec<Question>,
    /// 每题选中的文本(多选为多个;自定义答案也放这里);`None` 表示尚未回答
    pub answers: Option<Vec<Vec<String>>>,
}

/// 子 Agent 状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubagentStatus {
    Starting,
    Running,
    Background,
    Completed,
    Failed,
    /// 被用户停止(取消回合或停止任务)
    Stopped,
}

/// 子 Agent 卡片,内部嵌套它自己的时间线。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubagentCard {
    pub id: String,
    /// 子 Agent 类型,如 `Explore`、`code-reviewer`
    pub agent_type: String,
    pub description: String,
    pub status: SubagentStatus,
    pub children: Vec<TimelineItem>,
    /// 对应的后台任务 id(`task_started` 之后才有)
    #[serde(default)]
    pub task_id: Option<String>,
    /// 是否在后台运行(启动时 `run_in_background`,或之后被转入后台)
    #[serde(default)]
    pub background: bool,
    /// 开始时间(unix 毫秒);回放的历史没有就是 `None`
    #[serde(default)]
    pub started_ms: Option<u64>,
    /// 结束时间(unix 毫秒)
    #[serde(default)]
    pub ended_ms: Option<u64>,
    /// 累计 token 数(来自 `task_progress`)
    #[serde(default)]
    pub tokens: u64,
    /// 工具调用次数(来自 `task_progress`;没有时用嵌套时间线里的工具数)
    #[serde(default)]
    pub tool_count: u32,
    /// 最近一个工具名
    #[serde(default)]
    pub last_tool: String,
    /// 进度摘要(运行中)或结果摘要(结束后)
    #[serde(default)]
    pub summary: String,
    /// 最近一次有进展的时间(unix 毫秒),用于判断疑似停滞
    #[serde(default)]
    pub last_activity_ms: Option<u64>,
}

impl SubagentCard {
    /// 新建子 Agent 卡片。开始时间在卡片进入时间线(`Append`)时由归约器记录,这样脚本提前构建也不会算错用时。
    pub fn new(id: &str, agent_type: &str, description: &str, status: SubagentStatus) -> Self {
        Self {
            id: id.into(),
            agent_type: agent_type.into(),
            description: description.into(),
            status,
            children: Vec::new(),
            task_id: None,
            background: status == SubagentStatus::Background,
            started_ms: None,
            ended_ms: None,
            tokens: 0,
            tool_count: 0,
            last_tool: String::new(),
            summary: String::new(),
            last_activity_ms: None,
        }
    }

    /// 运行中的卡片第一次进入时间线时记录开始时间与最近活动。
    pub fn stamp_start(&mut self, now: u64) {
        if self.is_active() && self.started_ms.is_none() {
            self.started_ms = Some(now);
            self.last_activity_ms = Some(now);
        }
    }

    /// 是否仍在运行(含后台)。
    pub fn is_active(&self) -> bool {
        matches!(self.status, SubagentStatus::Starting | SubagentStatus::Running | SubagentStatus::Background)
    }

    /// 工具调用次数:优先用协议上报的,否则数嵌套时间线里的工具。
    pub fn tools(&self) -> u32 {
        if self.tool_count > 0 {
            return self.tool_count;
        }
        self.children.iter().filter(|c| matches!(c, TimelineItem::Tool(_))).count() as u32
    }

    /// 已运行时长(毫秒);没有开始时间返回 `None`。
    pub fn elapsed_ms(&self, now: u64) -> Option<u64> {
        let start = self.started_ms?;
        Some(self.ended_ms.unwrap_or(now).saturating_sub(start))
    }

    /// 是否疑似停滞:仍在运行,且超过 `STALL_MS` 没有任何进展。
    pub fn stalled(&self, now: u64) -> bool {
        self.is_active() && self.last_activity_ms.is_some_and(|t| now.saturating_sub(t) >= crate::task::STALL_MS)
    }
}

/// 计划批准卡(ExitPlanMode)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanApproval {
    pub id: String,
    pub markdown: String,
    /// 批准结果:`None` 等待中,`Some(true)` 已批准,`Some(false)` 已拒绝
    pub approved: Option<bool>,
}

/// 时间线上的一个条目。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TimelineItem {
    User { id: String, text: String, images: Vec<String>, files: Vec<String> },
    Assistant { id: String, text: String, streaming: bool },
    Thinking { id: String, text: String, streaming: bool },
    Tool(ToolCall),
    Todos { id: String, items: Vec<TodoItem> },
    Subagent(SubagentCard),
    Permission(PermissionRequest),
    Question(QuestionRequest),
    PlanApproval(PlanApproval),
    Notice { id: String, level: NoticeLevel, text: String },
    /// Canvas 画布卡片
    Canvas { id: String, title: String },
    /// 本轮改动汇总
    Changes { id: String, files: Vec<ChangedFile> },
    /// 后台任务结束通知(`task_notification`):完成 / 失败 / 被停止
    TaskNote { id: String, task_id: String, kind: TaskKind, status: TaskStatus, text: String },
}

/// 改动汇总中的一个文件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangedFile {
    pub path: String,
    pub added: u32,
    pub removed: u32,
}

impl TimelineItem {
    /// 条目 id。
    pub fn id(&self) -> &str {
        match self {
            Self::User { id, .. }
            | Self::Assistant { id, .. }
            | Self::Thinking { id, .. }
            | Self::Todos { id, .. }
            | Self::Notice { id, .. }
            | Self::Canvas { id, .. }
            | Self::Changes { id, .. }
            | Self::TaskNote { id, .. } => id,
            Self::Tool(t) => &t.id,
            Self::Subagent(s) => &s.id,
            Self::Permission(p) => &p.id,
            Self::Question(q) => &q.id,
            Self::PlanApproval(p) => &p.id,
        }
    }

    /// 是否在等待用户操作(权限 / 提问 / 计划批准)。
    pub fn awaits_user(&self) -> bool {
        match self {
            Self::Permission(p) => p.resolved.is_none(),
            Self::Question(q) => q.answers.is_none(),
            Self::PlanApproval(p) => p.approved.is_none(),
            _ => false,
        }
    }
}
