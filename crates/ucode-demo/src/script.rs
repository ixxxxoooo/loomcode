//! 演示脚本:带时间轴的事件序列,以及构建它的链式构建器。

use serde_json::Value;
use ucode_core::*;

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
    parent: Option<String>,
    stop: Option<StopReason>,
}

impl ScriptBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// 生成唯一 id。
    pub fn next_id(&mut self, prefix: &str) -> String {
        self.counter += 1;
        format!("{prefix}-{}", self.counter)
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

    fn stream(&mut self, id: &str, text: &str, thinking: bool) {
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
            self.steps.push(Step::Sleep(18));
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
            items: items.iter().map(|(c, s)| TodoItem { content: c.to_string(), status: *s }).collect(),
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

    /// 计划批准卡:追加并等待。
    pub fn plan(&mut self, markdown: &str) -> String {
        let id = self.next_id("plan");
        self.append(TimelineItem::PlanApproval(PlanApproval { id: id.clone(), markdown: markdown.into(), approved: None }));
        self.steps.push(Step::WaitPlan(id.clone()));
        id
    }

    /// 子 Agent 卡片(追加,状态由调用者随后推进)。
    pub fn subagent(&mut self, agent_type: &str, description: &str, status: SubagentStatus) -> String {
        let id = self.next_id("sub");
        self.append(TimelineItem::Subagent(SubagentCard {
            id: id.clone(),
            agent_type: agent_type.into(),
            description: description.into(),
            status,
            children: vec![],
        }));
        id
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
