//! 会话时间线条目。

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
            | Self::Changes { id, .. } => id,
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
