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

/// 账户限额数据的缓存时长(秒):限额按小时 / 周重置,变化很慢,点开面板时数据还新就不再请求;
/// 想要最新的,点面板里的「刷新」。
pub const LIMITS_TTL_SECS: u64 = 300;

/// MCP 服务器的连接状态(取自 claude `system init` 的 `mcp_servers`)。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum McpConn {
    /// 已连接
    Connected,
    /// 正在连接
    Pending,
    /// 连接失败
    Failed,
    /// 需要授权
    NeedsAuth,
    /// 已被停用(`mcp_toggle` 或配置里关掉)
    Disabled,
    /// 还没有信息(会话尚未启动)或无法识别
    #[default]
    Unknown,
}

impl McpConn {
    /// 把 claude 报的状态文字归类(`connected` / `pending` / `failed` / `needs-auth` 等)。
    pub fn parse(s: &str) -> Self {
        let l = s.to_lowercase();
        if l.contains("disabled") {
            Self::Disabled
        } else if l.contains("auth") {
            Self::NeedsAuth
        } else if l.contains("fail") || l.contains("error") {
            Self::Failed
        } else if l.contains("pending") || l.contains("connecting") {
            Self::Pending
        } else if l.contains("connect") {
            Self::Connected
        } else {
            Self::Unknown
        }
    }
}

/// 一个 MCP 服务器在当前会话里的实际情况。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerStatus {
    pub name: String,
    pub conn: McpConn,
    /// 它提供的工具名(不带 `mcp__<名称>__` 前缀)
    pub tools: Vec<String>,
    /// 连接失败时的原因(取第一行,已截断)
    pub error: Option<String>,
}

/// MCP 服务器名在工具名里的写法:claude 把名字里字母数字 `_` `-` 之外的字符换成 `_`
/// (如 `claude.ai Claude Docs` → `claude_ai_Claude_Docs`),工具全名是 `mcp__<写法>__<工具>`。
pub fn mcp_norm_name(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

/// 一个 MCP 工具在上下文里的占用(`get_context_usage` 的 `mcpTools`)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpToolUsage {
    /// 服务器名的工具名写法(见 `mcp_norm_name`)
    pub server: String,
    /// 工具名(不带前缀)
    pub tool: String,
    pub tokens: u64,
    /// 完整定义已加载进上下文(`false` = 按需加载,不占窗口)
    pub loaded: bool,
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
    /// 5 小时限额的重置时刻(Unix 秒)
    pub five_hour_resets: Option<i64>,
    /// 每周限额的重置时刻(Unix 秒)
    pub weekly_resets: Option<i64>,
    /// 上下文按类别的明细(`get_context_usage`);空表示还没查到
    pub context_categories: Vec<ContextCategory>,
    /// 最近一次请求的提示缓存情况
    pub cache_last: CacheTokens,
    /// 整段主对话累计的提示缓存情况
    pub cache_total: CacheTokens,
    /// 各 MCP 工具的占用(上下文明细里带的;没查到为空)
    pub mcp_tools: Vec<McpToolUsage>,
    /// 限额数据最近一次更新的时刻(Unix 秒);`None` 表示还没取到
    pub limits_at: Option<i64>,
    /// 限额查询状态(只在用户点开上下文弹层或点「刷新」时才查询,不自动轮询)
    pub limits_state: LimitsState,
}

/// 上下文类别的性质。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ContextKind {
    /// 占用窗口
    #[default]
    Used,
    /// 剩余空间
    Free,
    /// 为压缩预留
    Buffer,
    /// 按需加载、暂不占用窗口
    Deferred,
}

/// 上下文的一个类别(系统提示、工具、消息…)。
#[derive(Debug, Clone, PartialEq)]
pub struct ContextCategory {
    pub name: String,
    pub tokens: u64,
    pub kind: ContextKind,
}

/// 提示缓存的 token 分布:命中读取、写入、未缓存。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheTokens {
    pub read: u64,
    pub written: u64,
    pub uncached: u64,
}

impl CacheTokens {
    /// 三项之和。
    pub fn total(&self) -> u64 {
        self.read + self.written + self.uncached
    }

    /// 命中率(0..=100,取整);没有数据为 0。
    pub fn hit_percent(&self) -> u32 {
        match self.total() {
            0 => 0,
            t => (self.read as f64 / t as f64 * 100.0).round() as u32,
        }
    }
}

/// 一个回合的消耗统计(挂在回合的用户消息上,显示在助手回复下面的操作条里)。
///
/// 实时回合取自 `result` 事件;回放历史时由各条助手消息的 `usage` 汇总,没有的项为 `None`。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TurnStats {
    /// 实际使用的模型 id(如 `claude-opus-5-5`);取不到为 `None`
    pub model: Option<String>,
    /// 未命中缓存的输入 token
    pub input: u64,
    /// 输出 token(含思考)
    pub output: u64,
    /// 其中思考 token
    pub thinking: u64,
    /// 缓存读取的输入 token
    pub cache_read: u64,
    /// 缓存写入的输入 token
    pub cache_write: u64,
    /// 本回合花费(美元);服务商算不出费用时为 `None`
    pub cost_usd: Option<f64>,
    /// 模型调用总耗时(毫秒)
    pub api_ms: Option<u64>,
    /// 首字延迟(毫秒)
    pub ttft_ms: Option<u64>,
    /// 模型调用轮数(`num_turns`)
    pub calls: Option<u32>,
}

impl TurnStats {
    /// 模型显示名:去掉末尾的日期后缀(`claude-haiku-4-5-20251001` → `claude-haiku-4-5`)。
    pub fn model_label(&self) -> Option<String> {
        let m = self.model.as_deref().filter(|m| !m.is_empty() && !m.starts_with('<'))?;
        let base = match m.rsplit_once('-') {
            Some((head, tail)) if tail.len() == 8 && tail.bytes().all(|b| b.is_ascii_digit()) => head,
            _ => m,
        };
        Some(base.to_string())
    }

    /// 输入总量:未缓存 + 缓存读取 + 缓存写入。
    pub fn input_total(&self) -> u64 {
        self.input + self.cache_read + self.cache_write
    }

    /// 缓存命中率(0..=100,取整);没有输入为 0。
    pub fn hit_percent(&self) -> u32 {
        match self.input_total() {
            0 => 0,
            t => (self.cache_read as f64 / t as f64 * 100.0).round() as u32,
        }
    }

    /// 是否有可显示的内容。
    pub fn is_empty(&self) -> bool {
        self.input_total() == 0 && self.output == 0
    }

    /// 费用文案:不到 1 分钱保留三位小数(`$0.004`),否则两位(`$0.12`)。
    pub fn cost_text(cost: f64) -> String {
        if cost < 0.01 { format!("${cost:.3}") } else { format!("${cost:.2}") }
    }

    /// 操作条上的简短文案:`↑ 21.1k ↓ 230 · 缓存 98% · $0.004`(没有的项跳过)。
    pub fn short_text(&self) -> String {
        use crate::task::format_tokens as k;
        let mut parts = vec![format!("↑ {} ↓ {}", k(self.input_total()), k(self.output))];
        if self.input_total() > 0 && (self.cache_read > 0 || self.cache_write > 0) {
            parts.push(format!("缓存 {}%", self.hit_percent()));
        }
        if let Some(c) = self.cost_usd {
            parts.push(Self::cost_text(c));
        }
        parts.join(" · ")
    }

    /// 悬停明细:逐项列出 token 分类、费用、延迟与调用轮数(多行)。
    pub fn detail_text(&self) -> String {
        use crate::task::format_tokens as k;
        let mut lines = vec![
            format!("输入 {}(未缓存 {} · 缓存读取 {} · 缓存写入 {})", k(self.input_total()), k(self.input), k(self.cache_read), k(self.cache_write)),
            if self.thinking > 0 { format!("输出 {}(其中思考 {})", k(self.output), k(self.thinking)) } else { format!("输出 {}", k(self.output)) },
        ];
        if self.input_total() > 0 {
            lines.push(format!("缓存命中率 {}%", self.hit_percent()));
        }
        if let Some(c) = self.cost_usd {
            lines.push(format!("费用 {}", Self::cost_text(c)));
        }
        let secs = |ms: u64| format!("{:.1}s", ms as f64 / 1000.0);
        let mut timing = Vec::new();
        if let Some(ms) = self.ttft_ms {
            timing.push(format!("首字 {}", secs(ms)));
        }
        if let Some(ms) = self.api_ms {
            timing.push(format!("模型耗时 {}", secs(ms)));
        }
        if let Some(n) = self.calls {
            timing.push(format!("{n} 次调用"));
        }
        if !timing.is_empty() {
            lines.push(timing.join(" · "));
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod turn_stats_tests {
    use super::*;

    #[test]
    fn 简短文案与明细() {
        let s = TurnStats { model: Some("claude-haiku-4-5-20251001".into()), input: 4, output: 230, thinking: 149, cache_read: 35_613, cache_write: 19_732, cost_usd: Some(0.0044), api_ms: Some(4233), ttft_ms: Some(2916), calls: Some(2) };
        assert_eq!(s.model_label().as_deref(), Some("claude-haiku-4-5"));
        assert_eq!(TurnStats { model: Some("<synthetic>".into()), ..Default::default() }.model_label(), None);
        assert_eq!(s.short_text(), "↑ 55.3k ↓ 230 · 缓存 64% · $0.004");
        let d = s.detail_text();
        assert!(d.contains("其中思考 149") && d.contains("首字 2.9s") && d.contains("2 次调用"), "{d}");
        // 没有费用(第三方服务商 / 历史回放)不显示费用,没有缓存不显示命中率
        let h = TurnStats { input: 120, output: 40, ..Default::default() };
        assert_eq!(h.short_text(), "↑ 120 ↓ 40");
        assert_eq!(TurnStats::cost_text(0.123), "$0.12");
        assert!(TurnStats::default().is_empty());
    }
}

/// 账户限额的查询状态。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LimitsState {
    /// 没有进行中的查询
    #[default]
    Idle,
    /// 正在向官方接口查询
    Checking,
    /// 查询失败或当前账户没有限额(如使用第三方服务商)
    Unavailable,
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
    /// 用户消息条目对应的 claude 消息 uuid(文件检查点按它记录,回退时用)
    UserUuid { id: String, uuid: String },
    /// 文件回退请求的结果(预览或实际执行)
    Rewind(crate::rewind::RewindOutcome),
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
    /// 计划正文更新(流式生成中 `streaming = true`;最终定稿 `false`)
    PlanUpdated { id: String, markdown: String, streaming: bool },
    Usage(Usage),
    /// 会话启动后各 MCP 服务器的连接状态与工具数(`system init`)
    McpServers(Vec<McpServerStatus>),
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
    /// 回合消耗统计;`id` 为用户消息 id,`None` 表示最近一条用户消息(实时回合在 `TurnEnded` 之前发出)
    TurnStats { id: Option<String>, stats: TurnStats },
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
    /// 查询账户用量与限额(官方 `get_usage` 控制请求);只在用户点开上下文弹层或点「刷新」时发送。
    /// `force` 为假时,限额数据还新(见 `LIMITS_TTL_SECS`)就不再请求;为真(用户点了「刷新」)则一定请求
    RefreshUsage { force: bool },
    /// 刷新 MCP 服务器状态与各工具的上下文占用(`mcp_status` + `get_context_usage`);**不**查账户限额
    RefreshMcp,
    /// 热切换一个 MCP 服务器(`mcp_toggle`):立即生效、不重启;后端没启动时忽略(配置已写盘,启动时读取)
    McpToggle { name: String, enabled: bool },
    /// 替换注入 claude 的额外环境变量(切换模型服务商时用):惰性后端结束当前进程,
    /// 下一条提示用新环境 `--resume` 继续当前会话;其余后端忽略
    SetEnv(Vec<(String, String)>),
    /// 把 Claude 改过的文件回退到某条用户消息(claude 消息 uuid)发出之前;`dry_run` 只预览会改哪些文件
    RewindFiles { uuid: String, dry_run: bool },
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
            | AgentEvent::PlanResolved { id, .. }
            | AgentEvent::PlanUpdated { id, .. } => Some(id),
            _ => None,
        }
    }
}
