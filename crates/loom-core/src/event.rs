//! 后端 → 界面的事件,以及界面 → 后端的命令。

use crate::modes::{Effort, ModelInfo, PermissionMode};
use crate::timeline::{NoticeLevel, PermissionRequest, TimelineItem, TodoItem};
use crate::task::{BackgroundTask, TaskStatus};
use crate::tool::ToolPatch;

/// 回合结束原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    EndTurn,
    Cancelled,
    Refusal,
    MaxTokens,
    /// 请求失败(限流、鉴权等),细节见随后的 Notice
    Error,
}

/// 用量快照。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Usage {
    /// 上下文已用 token
    pub context_used: u64,
    /// 上下文窗口大小
    pub context_size: u64,
    /// 本会话累计花费(美元)
    pub cost_usd: f64,
    /// 5 小时限额使用百分比
    pub five_hour_pct: Option<f32>,
    /// 每周限额使用百分比
    pub weekly_pct: Option<f32>,
}

impl Usage {
    /// 上下文使用比例 0..=1。
    pub fn context_ratio(&self) -> f32 {
        if self.context_size == 0 {
            0.0
        } else {
            (self.context_used as f32 / self.context_size as f32).clamp(0.0, 1.0)
        }
    }
}

/// 后端产出的事件。`parent` 为子 Agent 卡片 id,表示该事件属于嵌套时间线。
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// 会话就绪
    Ready { session_id: String, models: Vec<ModelInfo>, model: String, mode: PermissionMode },
    TurnStarted,
    /// 记录某个条目的用时(秒):思考条目是一次思考的用时,用户消息条目是整个回合的用时(历史回放用时间戳算出)
    Duration { id: String, secs: u64 },
    /// 追加一个条目
    Append { parent: Option<String>, item: TimelineItem },
    /// 助手文本增量
    TextDelta { parent: Option<String>, id: String, delta: String },
    /// 思考增量
    ThinkingDelta { parent: Option<String>, id: String, delta: String },
    /// 工具更新
    ToolUpdate { parent: Option<String>, id: String, patch: ToolPatch },
    /// 条目流式结束(文本 / 思考)
    ItemDone { parent: Option<String>, id: String },
    /// 子 Agent 状态变更
    SubagentStatus { id: String, status: crate::timeline::SubagentStatus },
    /// 子 Agent 结束:状态、结果摘要与统计(来自 `tool_result` 与 `toolUseResult`)
    SubagentFinished { id: String, status: crate::timeline::SubagentStatus, summary: String, tokens: u64, tool_count: u32, duration_ms: Option<u64> },
    /// 待办全量替换
    Todos { id: String, items: Vec<TodoItem> },
    /// 把已有的同 id 工具卡升级成权限请求(真实后端:工具调用先出现,权限请求随后到达);
    /// 找不到同 id 工具卡时直接追加。
    PromoteToPermission { parent: Option<String>, request: PermissionRequest },
    /// 权限请求被解决(后端确认)
    PermissionResolved { id: String, option: String },
    /// 提问被回答
    QuestionAnswered { id: String, answers: Vec<Vec<String>> },
    /// 计划批准结果
    PlanResolved { id: String, approved: bool },
    Usage(Usage),
    ModeChanged(PermissionMode),
    ModelChanged(String),
    TitleChanged(String),
    Notice { level: NoticeLevel, text: String },
    /// 后台任务启动(`task_started`);同一 `task_id` 再次启动视为恢复运行
    TaskStarted(BackgroundTask),
    /// 后台任务进度(`task_progress`),字段为 `None` 表示不变
    TaskProgress { task_id: String, tokens: Option<u64>, tool_uses: Option<u32>, last_tool: Option<String>, summary: Option<String> },
    /// 后台任务状态补丁(`task_updated`)
    TaskUpdated { task_id: String, status: Option<TaskStatus>, description: Option<String>, output_file: Option<String> },
    /// 后台任务终态通知(`task_notification`),这是结束的权威信号
    TaskNotification { task_id: String, status: TaskStatus, summary: String, output_file: Option<String> },
    /// 后台任务全量快照(`background_tasks_changed`):用它替换缓存,漏掉的事件会被纠正
    TasksChanged(Vec<BackgroundTask>),
    /// 后台任务产生了新输出(追加到任务的输出缓冲)
    TaskOutput { task_id: String, text: String },
    TurnEnded(StopReason),
}

/// 界面发给后端的命令。
#[derive(Debug, Clone)]
pub enum AgentCommand {
    /// 发送提示;`steer` 为真表示运行中追加的「引导」消息
    Prompt { text: String, images: Vec<String>, files: Vec<String>, steer: bool },
    Cancel,
    SetModel(String),
    SetMode(PermissionMode),
    SetEffort(Effort),
    ResolvePermission { id: String, option: String },
    AnswerQuestion { id: String, answers: Vec<Vec<String>> },
    /// 计划批准;批准时 `mode` 是之后要切到的权限模式(`None` 沿用 claude 的建议)
    ResolvePlan { id: String, approve: bool, mode: Option<PermissionMode> },
    /// 停止一个后台任务(对应 claude 的 `stop_task` 控制请求)
    StopTask { task_id: String },
    /// 把前台运行中的 Bash / 子 Agent 转入后台(Ctrl+B);`tool_use_id` 为空表示全部
    BackgroundCurrent { tool_use_id: Option<String> },
    /// 触发演示场景(仅演示后端识别)
    RunScenario(String),
    /// 关闭后端(结束子进程)
    Shutdown,
}

impl AgentEvent {
    /// 该事件影响的条目 id(用于定位需要重绘的行);无关条目的事件返回 `None`。
    pub fn target_id(&self) -> Option<&str> {
        match self {
            AgentEvent::Append { parent, .. } => parent.as_deref(),
            AgentEvent::TextDelta { parent, id, .. }
            | AgentEvent::ThinkingDelta { parent, id, .. }
            | AgentEvent::ToolUpdate { parent, id, .. }
            | AgentEvent::ItemDone { parent, id } => Some(parent.as_deref().unwrap_or(id)),
            AgentEvent::PromoteToPermission { parent, request } => Some(parent.as_deref().unwrap_or(&request.id)),
            AgentEvent::SubagentStatus { id, .. }
            | AgentEvent::SubagentFinished { id, .. }
            | AgentEvent::Todos { id, .. }
            | AgentEvent::PermissionResolved { id, .. }
            | AgentEvent::QuestionAnswered { id, .. }
            | AgentEvent::PlanResolved { id, .. } => Some(id),
            _ => None,
        }
    }
}
