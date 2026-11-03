//! 演示脚本:带时间轴的事件序列,以及构建它的链式构建器。

use serde_json::Value;
use loom_core::*;

/// 脚本中的一步。
#[derive(Debug, Clone)]
pub enum Step {
    /// 发出一个事件
    Emit(AgentEvent),
    /// 休眠毫秒(可被取消打断)
    Sleep(u64),
    /// 等待用户处理指定权限请求
    WaitPermission(String),
    /// 等待用户回答指定提问
    WaitQuestion(String),
    /// 等待用户批准指定计划
    WaitPlan(String),
    /// 永久停住,直到用户取消(冻结场景)
    Hold,
    /// 登记一个后台任务时间线:它独立于回合继续运行(回合结束后仍会推进,直到完成或被停止)
    Spawn(BgJob),
}

/// 后台任务的时间线:相对登记时刻的偏移毫秒 → 事件。
#[derive(Debug, Clone, Default)]
pub struct BgJob {
    pub task_id: String,
    /// 按偏移升序的事件;`followup` 为真的事件(任务完成后助手续写的一轮)只在会话空闲时发出
    pub events: Vec<BgEvent>,
    /// 被用户停止时发出的事件(通常是 `TaskNotification` 的已停止通知)
    pub on_stop: Vec<AgentEvent>,
}

/// 后台时间线里的一个事件。
#[derive(Debug, Clone)]
pub struct BgEvent {
    pub at_ms: u64,
    pub event: AgentEvent,
    pub followup: bool,
}

impl BgJob {
    pub fn new(task_id: &str) -> Self {
        Self { task_id: task_id.into(), ..Default::default() }
    }

    /// 在 `at_ms` 发出任意事件。
    pub fn at(mut self, at_ms: u64, event: AgentEvent) -> Self {
        self.events.push(BgEvent { at_ms, event, followup: false });
        self
    }

    /// 后台输出一段文本。
    pub fn output(self, at_ms: u64, text: &str) -> Self {
        let task_id = self.task_id.clone();
        self.at(at_ms, AgentEvent::TaskOutput { task_id, text: text.into() })
    }

    /// 进度:token 数、工具数、最近工具与摘要。
    pub fn progress(self, at_ms: u64, tokens: u64, tools: u32, last_tool: &str, summary: &str) -> Self {
        let task_id = self.task_id.clone();
        let opt = |s: &str| (!s.is_empty()).then(|| s.to_string());
        self.at(at_ms, AgentEvent::TaskProgress { task_id, tokens: Some(tokens), tool_uses: Some(tools), last_tool: opt(last_tool), summary: opt(summary) })
    }

    /// 在后台子 Agent 卡片内部追加条目(嵌套时间线)。
    pub fn child(self, at_ms: u64, card_id: &str, item: TimelineItem) -> Self {
        self.at(at_ms, AgentEvent::Append { parent: Some(card_id.into()), item })
    }

    /// 终态通知(任务结束的权威信号)。
    pub fn finish(self, at_ms: u64, status: TaskStatus, summary: &str) -> Self {
        let task_id = self.task_id.clone();
        self.at(at_ms, AgentEvent::TaskNotification { task_id, status, summary: summary.into(), output_file: None })
    }

    /// 任务结束后助手续写一轮(只在会话空闲时发出)。
    pub fn followup(mut self, at_ms: u64, text: &str) -> Self {
        let id = format!("bgfu-{}-{at_ms}", self.task_id);
        let mk = |event| BgEvent { at_ms, event, followup: true };
        self.events.push(mk(AgentEvent::TurnStarted));
        self.events.push(mk(AgentEvent::TextDelta { parent: None, id: id.clone(), delta: text.into() }));
        self.events.push(mk(AgentEvent::ItemDone { parent: None, id }));
        self.events.push(mk(AgentEvent::TurnEnded(StopReason::EndTurn)));
        self
    }

    /// 被停止时发出已停止通知。
    pub fn stoppable(mut self, summary: &str) -> Self {
        self.on_stop.push(AgentEvent::TaskNotification { task_id: self.task_id.clone(), status: TaskStatus::Stopped, summary: summary.into(), output_file: None });
        self
    }
}

/// 场景脚本。
#[derive(Debug, Clone, Default)]
pub struct Script {
    pub steps: Vec<Step>,
    /// 回合以什么原因收尾(默认正常结束)
    pub stop: Option<StopReason>,
}

/// 脚本构建器。所有 `parent` 相关调用都通过 `scope` 切换嵌套目标。
#[derive(Default)]
pub struct ScriptBuilder {
    steps: Vec<Step>,
    counter: u32,
    /// id 前缀:多个场景拼成一个长会话时避免 id 冲突
    ns: String,
    parent: Option<String>,
    stop: Option<StopReason>,
}

impl ScriptBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// 带 id 前缀的构建器。
    pub fn with_ns(ns: &str) -> Self {
        Self { ns: ns.into(), ..Self::default() }
    }

    /// 生成唯一 id。
    pub fn next_id(&mut self, prefix: &str) -> String {
        self.counter += 1;
        format!("{}{prefix}-{}", self.ns, self.counter)
    }

    fn emit(&mut self, e: AgentEvent) {
        self.steps.push(Step::Emit(e));
    }

    /// 在子 Agent `parent` 内部执行 `f`,结束后恢复。
    pub fn scope(&mut self, parent: &str, f: impl FnOnce(&mut Self)) -> &mut Self {
        let old = self.parent.replace(parent.to_string());
        f(self);
        self.parent = old;
        self
    }

    pub fn sleep(&mut self, ms: u64) -> &mut Self {
        self.steps.push(Step::Sleep(ms));
        self
    }

    pub fn hold(&mut self) -> &mut Self {
        self.steps.push(Step::Hold);
        self
    }

    /// 回合开始。
    pub fn turn_start(&mut self) -> &mut Self {
        self.emit(AgentEvent::TurnStarted);
        self
    }

    /// 回合结束。
    pub fn turn_end(&mut self, reason: StopReason) -> &mut Self {
        self.emit(AgentEvent::TurnEnded(reason));
        self
    }

    /// 用户消息。
    pub fn user(&mut self, text: &str) -> &mut Self {
        let id = self.next_id("user");
        self.emit(AgentEvent::Append {
            parent: self.parent.clone(),
            item: TimelineItem::User { id, text: text.into(), images: vec![], files: vec![] },
        });
        self
    }

    /// 带图片和文件的用户消息。
    pub fn user_rich(&mut self, text: &str, images: &[&str], files: &[&str]) -> &mut Self {
        let id = self.next_id("user");
        self.emit(AgentEvent::Append {
            parent: self.parent.clone(),
            item: TimelineItem::User {
                id,
                text: text.into(),
                images: images.iter().map(|s| s.to_string()).collect(),
                files: files.iter().map(|s| s.to_string()).collect(),
            },
        });
        self
    }

    /// 流式助手文本(按块输出)。
    pub fn say(&mut self, md: &str) -> &mut Self {
        let id = self.next_id("asst");
        self.stream(&id, md, false);
        self
    }

    /// 流式思考文本。
    pub fn think(&mut self, text: &str) -> &mut Self {
        let id = self.next_id("think");
        self.stream(&id, text, true);
        self
    }

    /// 流式思考文本,并把总耗时拉长到约 `ms` 毫秒(演示「已思考 N 秒」)。
    pub fn think_for(&mut self, text: &str, ms: u64) -> &mut Self {
        let id = self.next_id("think");
        let chunks = text.chars().count().div_ceil(24).max(1) as u64;
        self.stream_paced(&id, text, true, (ms / chunks).max(18));
        self
    }

    fn stream(&mut self, id: &str, text: &str, thinking: bool) {
        self.stream_paced(id, text, thinking, 18);
    }

    fn stream_paced(&mut self, id: &str, text: &str, thinking: bool, pace_ms: u64) {
        let chars: Vec<char> = text.chars().collect();
        for chunk in chars.chunks(24) {
            let delta: String = chunk.iter().collect();
            let parent = self.parent.clone();
            let id_s = id.to_string();
            self.emit(if thinking {
                AgentEvent::ThinkingDelta { parent, id: id_s, delta }
            } else {
                AgentEvent::TextDelta { parent, id: id_s, delta }
            });
            self.steps.push(Step::Sleep(pace_ms));
        }
        self.emit(AgentEvent::ItemDone { parent: self.parent.clone(), id: id.into() });
    }

    /// 开始一个工具调用(运行中),返回其 id。
    pub fn tool_start(&mut self, call: ToolCall) -> String {
        let id = call.id.clone();
        self.emit(AgentEvent::Append { parent: self.parent.clone(), item: TimelineItem::Tool(call) });
        id
    }

    /// 结束工具调用。
    pub fn tool_end(&mut self, id: &str, status: ToolStatus, content: Vec<ToolContent>, ms: u64) -> &mut Self {
        self.emit(AgentEvent::ToolUpdate {
            parent: self.parent.clone(),
            id: id.into(),
            patch: ToolPatch { status: Some(status), append_content: content, duration_ms: Some(ms), ..Default::default() },
        });
        self
    }

    /// 一次完整的成功工具调用。
    pub fn tool(&mut self, name: &str, title: &str, input: Value, content: Vec<ToolContent>) -> &mut Self {
        let id = self.next_id("tool");
        let call = ToolCall::new(&id, name, title).with_input(input);
        self.tool_start(call);
        self.sleep(240);
        self.tool_end(&id, ToolStatus::Completed, content, 240);
        self
    }

    /// 一次完整的带文件位置的工具调用。
    pub fn tool_at(&mut self, name: &str, title: &str, input: Value, path: &str, content: Vec<ToolContent>) -> &mut Self {
        let id = self.next_id("tool");
        let call = ToolCall::new(&id, name, title).with_input(input).with_location(path);
        self.tool_start(call);
        self.sleep(240);
        self.tool_end(&id, ToolStatus::Completed, content, 240);
        self
    }

    /// 一次失败的工具调用。
    pub fn tool_fail(&mut self, name: &str, title: &str, input: Value, content: Vec<ToolContent>) -> &mut Self {
        let id = self.next_id("tool");
        self.tool_start(ToolCall::new(&id, name, title).with_input(input));
        self.sleep(240);
        self.tool_end(&id, ToolStatus::Failed, content, 240);
        self
    }

    /// 追加任意条目。
    pub fn append(&mut self, item: TimelineItem) -> &mut Self {
        self.emit(AgentEvent::Append { parent: self.parent.clone(), item });
        self
    }

    /// 提示。
    pub fn notice(&mut self, level: NoticeLevel, text: &str) -> &mut Self {
        self.emit(AgentEvent::Notice { level, text: text.into() });
        self
    }

    /// 待办。
    pub fn todos(&mut self, id: &str, items: &[(&str, TodoStatus)]) -> &mut Self {
        self.emit(AgentEvent::Todos {
            id: id.into(),
            items: items.iter().map(|(c, s)| TodoItem { content: c.to_string(), status: *s, active_form: Some(format!("正在{c}")) }).collect(),
        });
        self
    }

    /// 用量。
    pub fn usage(&mut self, used: u64, size: u64, five: Option<f32>, weekly: Option<f32>) -> &mut Self {
        self.emit(AgentEvent::Usage(Usage {
            context_used: used,
            context_size: size,
            cost_usd: used as f64 / 1_000_000.0 * 3.0,
            five_hour_pct: five,
            weekly_pct: weekly,
            limits_state: LimitsState::Idle,
            limits_at: five.or(weekly).map(|_| now_secs() - 120),
            context_categories: demo_categories(used, size),
            mcp_tools: demo_mcp_tools(),
            cache_last: CacheTokens { read: used * 9 / 10, written: used / 20, uncached: used / 20 },
            cache_total: CacheTokens { read: used * 4, written: used / 2, uncached: used / 2 },
            five_hour_resets: five.map(|_| now_secs() + 4 * 3600 + 1500),
            weekly_resets: weekly.map(|_| now_secs() + 3 * 86400 + 7200),
        }));
        self
    }

    pub fn title(&mut self, t: &str) -> &mut Self {
        self.emit(AgentEvent::TitleChanged(t.into()));
        self
    }

    pub fn mode(&mut self, m: PermissionMode) -> &mut Self {
        self.emit(AgentEvent::ModeChanged(m));
        self
    }

    /// 权限请求:追加卡片并等待用户处理。
    pub fn permission(&mut self, tool: ToolCall, options: Vec<PermissionOption>) -> String {
        let id = self.next_id("perm");
        self.append(TimelineItem::Permission(PermissionRequest { id: id.clone(), tool, options, resolved: None }));
        self.steps.push(Step::WaitPermission(id.clone()));
        id
    }

    /// 提问:追加向导并等待回答。
    pub fn question(&mut self, questions: Vec<Question>) -> String {
        let id = self.next_id("ask");
        self.append(TimelineItem::Question(QuestionRequest { id: id.clone(), questions, answers: None }));
        self.steps.push(Step::WaitQuestion(id.clone()));
        id
    }

    /// 计划批准卡,模拟流式生成:先出现空卡片,正文分段补全(每段 24 个字符),生成完再等待批准。
    pub fn plan_streaming(&mut self, markdown: &str) -> String {
        let id = self.next_id("plan");
        self.append(TimelineItem::PlanApproval(PlanApproval { id: id.clone(), markdown: String::new(), approved: None, streaming: true }));
        let chars: Vec<char> = markdown.chars().collect();
        let mut end = 0;
        for chunk in chars.chunks(24) {
            end += chunk.len();
            self.sleep(140);
            self.emit(AgentEvent::PlanUpdated { id: id.clone(), markdown: chars[..end].iter().collect(), streaming: true });
        }
        self.emit(AgentEvent::PlanUpdated { id: id.clone(), markdown: markdown.into(), streaming: false });
        self.steps.push(Step::WaitPlan(id.clone()));
        id
    }

    /// 计划批准卡:追加并等待。
    pub fn plan(&mut self, markdown: &str) -> String {
        let id = self.next_id("plan");
        self.append(TimelineItem::PlanApproval(PlanApproval { id: id.clone(), markdown: markdown.into(), approved: None, streaming: false }));
        self.steps.push(Step::WaitPlan(id.clone()));
        id
    }

    /// 子 Agent 卡片(追加,状态由调用者随后推进)。
    pub fn subagent(&mut self, agent_type: &str, description: &str, status: SubagentStatus) -> String {
        let id = self.next_id("sub");
        self.append(TimelineItem::Subagent(SubagentCard::new(&id, agent_type, description, status)));
        id
    }

    /// 指定 id 的子 Agent 卡片(后台任务的时间线要引用同一个 id)。
    pub fn subagent_with_id(&mut self, id: &str, agent_type: &str, description: &str, status: SubagentStatus) -> &mut Self {
        self.append(TimelineItem::Subagent(SubagentCard::new(id, agent_type, description, status)));
        self
    }

    /// 后台任务启动(`task_started`)。
    pub fn task_started(&mut self, task: BackgroundTask) -> &mut Self {
        self.emit(AgentEvent::TaskStarted(task));
        self
    }

    /// 后台任务全量快照(`background_tasks_changed`)。
    pub fn tasks_changed(&mut self, tasks: Vec<BackgroundTask>) -> &mut Self {
        self.emit(AgentEvent::TasksChanged(tasks));
        self
    }

    /// 登记后台任务时间线。
    pub fn spawn(&mut self, job: BgJob) -> &mut Self {
        self.steps.push(Step::Spawn(job));
        self
    }

    /// 发出任意事件。
    pub fn event(&mut self, e: AgentEvent) -> &mut Self {
        self.emit(e);
        self
    }

    /// 带终态与统计的子 Agent 结束。
    pub fn subagent_done(&mut self, id: &str, status: SubagentStatus, summary: &str, tokens: u64, tools: u32, secs: u64) -> &mut Self {
        self.emit(AgentEvent::SubagentFinished { id: id.into(), status, summary: summary.into(), tokens, tool_count: tools, duration_ms: Some(secs * 1000) });
        self
    }

    pub fn subagent_status(&mut self, id: &str, status: SubagentStatus) -> &mut Self {
        self.emit(AgentEvent::SubagentStatus { id: id.into(), status });
        self
    }

    /// 指定回合收尾原因。
    pub fn stops(&mut self, reason: StopReason) -> &mut Self {
        self.stop = Some(reason);
        self
    }

    /// 完成构建。
    pub fn build(self) -> Script {
        Script { steps: self.steps, stop: self.stop }
    }
}

/// 常用的权限选项集合。
pub fn standard_options() -> Vec<PermissionOption> {
    vec![
        PermissionOption { id: "allow".into(), label: "允许".into(), kind: PermissionOptionKind::AllowOnce },
        PermissionOption { id: "always".into(), label: "始终允许".into(), kind: PermissionOptionKind::AllowAlways },
        PermissionOption { id: "deny".into(), label: "拒绝".into(), kind: PermissionOptionKind::RejectOnce },
    ]
}

/// 构造一个单选问题。
pub fn question(header: &str, text: &str, opts: &[(&str, &str)], multi: bool) -> Question {
    Question {
        header: header.into(),
        question: text.into(),
        options: opts
            .iter()
            .map(|(l, d)| QuestionOption { label: l.to_string(), description: d.to_string() })
            .collect(),
        multi_select: multi,
    }
}

/// 当前 Unix 秒(演示限额的重置时刻以它为基准)。
fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// 演示用的上下文分类:固定开销按比例,其余是消息,再加压缩预留与剩余。
/// 演示的 MCP 服务器(与沙盒 `.claude.json` 里的对应):连上的、要授权的、连不上的。
pub fn demo_mcp_servers() -> Vec<McpServerStatus> {
    let tools = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
    vec![
        McpServerStatus { name: "filesystem".into(), conn: McpConn::Connected, tools: tools(&["read_file", "write_file", "edit_file", "list_directory", "search_files", "move_file", "create_directory"]), error: None },
        McpServerStatus { name: "github".into(), conn: McpConn::NeedsAuth, tools: Vec::new(), error: None },
        McpServerStatus { name: "router-docs".into(), conn: McpConn::Failed, tools: Vec::new(), error: Some("Connection refused (localhost:4000)".into()) },
    ]
}

/// 演示的各 MCP 工具占用:filesystem 的工具都已加载。
fn demo_mcp_tools() -> Vec<McpToolUsage> {
    demo_mcp_servers().into_iter().flat_map(|s| s.tools.into_iter().map(move |t| McpToolUsage { server: s.name.clone(), tool: t, tokens: 380, loaded: true })).collect()
}

fn demo_categories(used: u64, size: u64) -> Vec<ContextCategory> {
    if size == 0 {
        return Vec::new();
    }
    let cat = |name: &str, tokens: u64, kind| ContextCategory { name: name.to_string(), tokens, kind };
    let fixed = [("系统提示词", 3_200u64), ("系统工具", 11_500), ("MCP 工具", 4_800), ("记忆文件", 2_100), ("技能", 900)];
    let fixed_sum: u64 = fixed.iter().map(|f| f.1).sum::<u64>().min(used);
    let mut out: Vec<ContextCategory> = fixed.iter().map(|(n, t)| cat(n, (*t).min(used), ContextKind::Used)).collect();
    out.push(cat("消息", used.saturating_sub(fixed_sum), ContextKind::Used));
    let buffer = size / 6;
    out.push(cat("压缩预留", buffer, ContextKind::Buffer));
    out.push(cat("剩余空间", size.saturating_sub(used + buffer), ContextKind::Free));
    out
}
