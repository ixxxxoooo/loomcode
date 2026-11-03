//! 有状态的消息翻译器:把 claude stream-json 输出的每一行翻译成 `AgentEvent`。
//!
//! 要点:
//! - 开启 `--include-partial-messages` 后,同一段文字会先以 `stream_event` 增量到达,
//!   随后又出现完整的 `assistant` 消息;翻译器用「认领」机制去重;
//! - 工具调用先以 `assistant.tool_use` 出现,权限请求(`control_request`)随后到达,
//!   用 `PromoteToPermission` 把已有工具卡升级成权限卡;
//! - `Task`/`Agent` 工具对应子 Agent 卡片,带 `parent_tool_use_id` 的消息归入其嵌套时间线;
//! - `AskUserQuestion` / `ExitPlanMode` 在 `tool_use` 阶段保持沉默,由权限请求生成提问向导与计划卡;
//! - `replay` 模式用于回放历史 jsonl:用户消息会被还原成时间线条目。

use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::time::Instant;
use loom_core::*;

/// 挂起的控制请求类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingKind {
    Permission,
    Question,
    Plan,
}

/// 一个等待用户决定的 `can_use_tool` 请求。
#[derive(Debug, Clone)]
pub struct Pending {
    /// 控制请求 id,应答时原样带回
    pub request_id: String,
    pub tool_use_id: Option<String>,
    pub tool_name: String,
    pub input: Value,
    /// claude 给出的权限更新建议
    pub suggestions: Vec<Value>,
    pub kind: PendingKind,
}

/// 从建议里取出会改变权限模式的那一项。
pub fn suggested_mode(pending: &Pending) -> Option<PermissionMode> {
    pending
        .suggestions
        .iter()
        .find(|s| s.get("type").and_then(Value::as_str) == Some("setMode"))
        .and_then(|s| s.get("mode").and_then(Value::as_str))
        .and_then(PermissionMode::from_cli)
}

struct ToolInfo {
    name: String,
    input: Value,
    started: Option<Instant>,
    parent: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    Text,
    Thinking,
}

/// 流式中的一个内容块。条目在第一次收到非空增量时才创建,避免出现空卡片。
/// 流式生成中的计划块。
struct PlanBlock {
    id: String,
    /// `input_json_delta` 累积的 JSON 片段
    json: String,
    /// 上一次发出的正文长度(没变化就不重复发事件)
    sent: usize,
}

/// 从不完整的 JSON 片段里取出字符串字段 `key` 已经到达的部分(处理转义,半截转义留到下次)。
///
/// 参数:`partial` 累积的 JSON 文本;`key` 字段名。返回:字段值的前缀;字段还没出现返回 `None`。
pub fn partial_json_string(partial: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let mut from = 0;
    let value_start = loop {
        let at = partial[from..].find(&needle)? + from;
        let rest = partial[at + needle.len()..].trim_start();
        if let Some(r) = rest.strip_prefix(':') {
            let r = r.trim_start();
            if let Some(body) = r.strip_prefix('"') {
                break partial.len() - body.len();
            }
        }
        from = at + needle.len();
    };
    let mut out = String::new();
    let mut chars = partial[value_start..].chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => break,
            '\\' => {
                let Some(e) = chars.next() else { break };
                match e {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    'b' => out.push('\u{8}'),
                    'f' => out.push('\u{c}'),
                    'u' => {
                        let hex: String = chars.by_ref().take(4).collect();
                        if hex.len() < 4 {
                            break;
                        }
                        let Ok(mut code) = u32::from_str_radix(&hex, 16) else { break };
                        // 代理对:高位后面必须跟 \uDC00..DFFF 才能拼成一个字符,不全就等下次
                        if (0xD800..0xDC00).contains(&code) {
                            if chars.next() == Some('\\') && chars.next() == Some('u') {
                                let lo: String = chars.by_ref().take(4).collect();
                                match (lo.len() == 4).then(|| u32::from_str_radix(&lo, 16).ok()).flatten() {
                                    Some(l) if (0xDC00..0xE000).contains(&l) => code = 0x10000 + ((code - 0xD800) << 10) + (l - 0xDC00),
                                    _ => break,
                                }
                            } else {
                                break;
                            }
                        }
                        if let Some(ch) = char::from_u32(code) {
                            out.push(ch);
                        }
                    }
                    other => out.push(other),
                }
            }
            c => out.push(c),
        }
    }
    Some(out)
}

struct OpenBlock {
    id: String,
    parent: Option<String>,
    created: bool,
}

/// 已创建、等待被完整 `assistant` 消息认领的流式条目。
struct Streamed {
    kind: BlockKind,
    claimed: bool,
}

/// 翻译器。
pub struct Translator {
    replay: bool,
    pub session_id: String,
    pub cwd: String,
    tools: HashMap<String, ToolInfo>,
    subagents: HashSet<String>,
    /// 当前流中的消息 id
    cur_msg: String,
    open_blocks: HashMap<(String, u64), OpenBlock>,
    streamed: HashMap<String, Vec<Streamed>>,
    /// 挂起的 can_use_tool 请求,键为条目 id(工具调用 id 或请求 id)
    pending: HashMap<String, Pending>,
    /// 正在流式生成的 ExitPlanMode 块:(消息 id, 块下标) → 计划条目 id 与已收到的 JSON 片段
    plan_blocks: HashMap<(String, u64), PlanBlock>,
    /// 已经以流式卡片出现在时间线上、还没定稿的计划条目 id(权限请求到达时定稿,不再重复追加)
    plans_streamed: HashSet<String>,
    usage: Usage,
    /// 用户是否请求过中断(用于把回合结束原因标成「已取消」)
    pub interrupted: bool,
    /// 本回合里 claude 发来的 API 错误合成消息的文字(它随后的 `result` 会带同样的文字,只显示一份)
    api_error: Option<String>,
    seq: u64,
    /// 实时条目 id 的代号:同一会话的进程重启后序号从头开始,靠它区分,避免与旧进程的条目撞 id
    pub id_gen: u64,
}

/// 是不是 claude 为 API 错误合成的 assistant 消息:带 `isApiErrorMessage` 标记,或模型是 `<synthetic>` 且文字以 `API Error` 开头。
fn is_api_error_message(v: &Value, text: &str) -> bool {
    let flagged = v.get("isApiErrorMessage").or_else(|| v["message"].get("isApiErrorMessage")).and_then(Value::as_bool).unwrap_or(false);
    flagged || (str_of(&v["message"], "model") == "<synthetic>" && text.trim_start().starts_with("API Error"))
}

impl Translator {
    pub fn new(cwd: &str) -> Self {
        Self {
            replay: false,
            session_id: String::new(),
            cwd: cwd.into(),
            tools: HashMap::new(),
            subagents: HashSet::new(),
            cur_msg: String::new(),
            open_blocks: HashMap::new(),
            streamed: HashMap::new(),
            pending: HashMap::new(),
            plan_blocks: HashMap::new(),
            plans_streamed: HashSet::new(),
            usage: Usage::default(),
            interrupted: false,
            api_error: None,
            seq: 0,
            id_gen: 0,
        }
    }

    /// 回放模式:用于历史 jsonl。
    pub fn replaying(cwd: &str) -> Self {
        let mut t = Self::new(cwd);
        t.replay = true;
        t
    }

    fn next_id(&mut self, prefix: &str) -> String {
        self.seq += 1;
        // 实时翻译加前缀 `l`,避免与先回放进时间线的历史条目 id(`u3`、`a5`…)撞车
        // 代号 > 0 时写成 `l3u1`:`l` 后紧跟数字,与代号 0 的 `lu1` 不会混淆
        if self.replay {
            format!("{prefix}{}", self.seq)
        } else if self.id_gen == 0 {
            format!("l{prefix}{}", self.seq)
        } else {
            format!("l{}{prefix}{}", self.id_gen, self.seq)
        }
    }

    /// 用户发出的提示:claude 不会回显,由界面侧自己追加到时间线。
    /// 返回:追加条目事件 + 条目与 claude 消息 `uuid` 的对应(文件回退用)。
    pub fn user_prompt(&mut self, text: &str, images: &[String], uuid: &str) -> Vec<AgentEvent> {
        let id = self.next_id("u");
        vec![
            AgentEvent::Append { parent: None, item: TimelineItem::User { id: id.clone(), text: text.into(), images: images.to_vec(), files: Vec::new() } },
            AgentEvent::UserUuid { id, uuid: uuid.into() },
        ]
    }

    /// 取走挂起的请求(应答时调用)。
    pub fn take_pending(&mut self, id: &str) -> Option<Pending> {
        self.pending.remove(id)
    }

    /// 是否仍有等待用户的请求。
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// 取出全部挂起请求。
    pub fn drain_pending(&mut self) -> Vec<(String, Pending)> {
        self.pending.drain().collect()
    }

    /// 处理一行输出(控制请求与取消请求除外,它们由 `control_request` / `cancel_request` 处理)。
    pub fn handle(&mut self, v: &Value) -> Vec<AgentEvent> {
        match str_of(v, "type") {
            "system" => self.system(v),
            "stream_event" => self.stream_event(v),
            "assistant" => self.assistant(v),
            "user" => self.user(v),
            "result" => self.result(v),
            "rate_limit_event" => self.rate_limit(v),
            other => {
                // 历史 jsonl 里本来就有很多非事件记录(摘要、快照等),只在实时会话里记
                if !self.replay {
                    crate::compat::note_unknown("type", other);
                }
                Vec::new()
            }
        }
    }

    fn parent_of(&self, v: &Value) -> Option<String> {
        v.get("parent_tool_use_id")
            .and_then(Value::as_str)
            .filter(|p| self.subagents.contains(*p))
            .map(String::from)
    }

    // ---------- system ----------

    fn system(&mut self, v: &Value) -> Vec<AgentEvent> {
        match str_of(v, "subtype") {
            "init" => {
                if let Some(id) = v.get("session_id").and_then(Value::as_str) {
                    self.session_id = id.into();
                }
                if self.cwd.is_empty() {
                    self.cwd = str_of(v, "cwd").into();
                }
                mcp_servers(v).map(|l| vec![AgentEvent::McpServers(l)]).unwrap_or_default()
            }
            "compact_boundary" => vec![notice(NoticeLevel::Info, "已压缩上下文")],
            "status" if str_of(v, "status") == "compacting" => {
                vec![notice(NoticeLevel::Info, "正在压缩上下文…")]
            }
            // 权限模式每次变化都会带上当前模式(批准计划、set_permission_mode 之后)
            "status" => match v.get("permissionMode").and_then(Value::as_str).and_then(PermissionMode::from_cli) {
                Some(m) => vec![AgentEvent::ModeChanged(m)],
                None => Vec::new(),
            },
            "task_started" => vec![AgentEvent::TaskStarted(self.task_from(v))],
            "task_progress" => {
                let usage = &v["usage"];
                vec![AgentEvent::TaskProgress {
                    task_id: str_of(v, "task_id").into(),
                    tokens: usage.get("total_tokens").and_then(Value::as_u64),
                    tool_uses: usage.get("tool_uses").and_then(Value::as_u64).map(|n| n as u32),
                    last_tool: v.get("last_tool_name").and_then(Value::as_str).map(String::from),
                    summary: v.get("summary").and_then(Value::as_str).map(String::from),
                }]
            }
            "task_updated" => {
                let patch = &v["patch"];
                vec![AgentEvent::TaskUpdated {
                    task_id: str_of(v, "task_id").into(),
                    status: patch.get("status").and_then(Value::as_str).map(TaskStatus::from_cli),
                    description: patch.get("description").and_then(Value::as_str).map(String::from),
                    output_file: None,
                }]
            }
            "task_notification" => vec![AgentEvent::TaskNotification {
                task_id: str_of(v, "task_id").into(),
                status: TaskStatus::from_cli(str_of(v, "status")),
                summary: str_of(v, "summary").into(),
                output_file: v.get("output_file").and_then(Value::as_str).map(String::from),
            }],
            "background_tasks_changed" => {
                let list = v.get("tasks").and_then(Value::as_array).map(|a| a.iter().map(|t| self.task_from(t)).collect()).unwrap_or_default();
                vec![AgentEvent::TasksChanged(list)]
            }
            // 钩子:只有失败值得打扰用户(开始与进度太碎)
            "hook_response" if str_of(v, "outcome") != "success" && !str_of(v, "outcome").is_empty() => {
                let name = str_of(v, "hook_name");
                let detail = ["stderr", "output", "stdout"].iter().map(|k| str_of(v, k).trim()).find(|t| !t.is_empty()).unwrap_or_default();
                let mut text = format!("钩子 {name} 执行失败");
                if let Some(code) = v.get("exit_code").and_then(Value::as_i64) {
                    text.push_str(&format!("(退出码 {code})"));
                }
                if !detail.is_empty() {
                    text.push_str(&format!(":{}", truncate(detail, 300)));
                }
                vec![notice(NoticeLevel::Warning, &text)]
            }
            "informational" => {
                let level = match str_of(v, "level") {
                    "error" => NoticeLevel::Error,
                    "warning" => NoticeLevel::Warning,
                    _ => NoticeLevel::Info,
                };
                let text = str_of(v, "content").trim();
                if text.is_empty() { Vec::new() } else { vec![notice(level, text)] }
            }
            "model_fallback" | "model_refusal_fallback" | "model_refusal_no_fallback" => {
                let text = str_of(v, "content").trim();
                let (from, to) = (str_of(v, "original_model"), str_of(v, "fallback_model"));
                let text = if !text.is_empty() {
                    text.to_string()
                } else if !to.is_empty() {
                    format!("模型 {from} 暂不可用,已回退到 {to}")
                } else {
                    format!("模型 {from} 拒绝了这次请求")
                };
                vec![notice(NoticeLevel::Warning, &text)]
            }
            "api_retry" => {
                let attempt = v.get("attempt").and_then(Value::as_u64).unwrap_or(1);
                let max = v.get("max_retries").and_then(Value::as_u64).unwrap_or(0);
                let text = if max > 0 {
                    format!("请求失败,正在重试({attempt}/{max})")
                } else {
                    format!("请求失败,正在重试(第 {attempt} 次)")
                };
                vec![notice(NoticeLevel::Warning, &text)]
            }
            other => {
                if !self.replay {
                    crate::compat::note_unknown("system", other);
                }
                Vec::new()
            }
        }
    }

    /// 把 `task_started` 或快照里的一项解析成后台任务。
    fn task_from(&self, v: &Value) -> BackgroundTask {
        let tool_use = v.get("tool_use_id").and_then(Value::as_str).filter(|t| !t.is_empty());
        let mut kind = TaskKind::from_task_type(str_of(v, "task_type"));
        // Monitor 在协议里也是 local_bash,靠发起它的工具名区分
        if tool_use.and_then(|id| self.tools.get(id)).is_some_and(|t| t.name == "Monitor") {
            kind = TaskKind::Monitor;
        }
        let first = |s: &str| s.lines().next().unwrap_or_default().trim().chars().take(120).collect::<String>();
        let description = [str_of(v, "description"), str_of(v, "prompt"), str_of(v, "subagent_type")]
            .into_iter()
            .find(|t| !t.trim().is_empty())
            .map(first)
            .unwrap_or_default();
        let mut task = BackgroundTask::new(str_of(v, "task_id"), kind, &description);
        task.tool_use_id = tool_use.map(String::from);
        task.run_id = v.get("run_id").and_then(Value::as_str).map(String::from);
        task.ambient = v.get("ambient").and_then(Value::as_bool).unwrap_or(false);
        task.owned_by_subagent = v.get("owned_by_subagent").and_then(Value::as_bool).unwrap_or(false);
        task.backgrounded = v.get("is_backgrounded").and_then(Value::as_bool).unwrap_or(true);
        task.output_file = v.get("output_file").and_then(Value::as_str).filter(|f| !f.is_empty()).map(String::from);
        task
    }

    // ---------- 流式 ----------

    fn stream_event(&mut self, v: &Value) -> Vec<AgentEvent> {
        let parent = self.parent_of(v);
        let ev = &v["event"];
        let msg_id = v
            .get("api_message_id")
            .and_then(Value::as_str)
            .map(String::from)
            .unwrap_or_else(|| self.cur_msg.clone());
        let index = ev.get("index").and_then(Value::as_u64).unwrap_or(0);
        match str_of(ev, "type") {
            "message_start" => {
                self.cur_msg = ev["message"]["id"].as_str().unwrap_or_default().into();
                // 每次调用模型的开头就带着当前上下文的 token 数:回合进行中(长工具链)也让圆环实时跟着涨
                self.live_context(&ev["message"]["usage"], parent.is_none())
            }
            // 一条消息结束时的累计用量(含这条的输出)
            "message_delta" => self.live_context(&ev["usage"], parent.is_none()),
            "content_block_start" => {
                // ExitPlanMode:计划正文开始生成就让卡片出现在正文里(文字随 input_json_delta 补全),
                // 不必等整段生成完、权限请求到达才出现。回放历史没有流事件,不走这里
                let block = &ev["content_block"];
                if str_of(block, "type") == "tool_use" && str_of(block, "name") == "ExitPlanMode" && !str_of(block, "id").is_empty() {
                    let id = str_of(block, "id").to_string();
                    self.plan_blocks.insert((msg_id, index), PlanBlock { id: id.clone(), json: String::new(), sent: 0 });
                    self.plans_streamed.insert(id.clone());
                    return vec![AgentEvent::Append {
                        parent,
                        item: TimelineItem::PlanApproval(PlanApproval { id, markdown: String::new(), approved: None, streaming: true }),
                    }];
                }
                if !matches!(str_of(block, "type"), "text" | "thinking") {
                    return Vec::new();
                }
                let id = format!("{msg_id}:{index}");
                self.open_blocks.insert((msg_id, index), OpenBlock { id, parent, created: false });
                Vec::new()
            }
            "content_block_delta" => {
                let delta = &ev["delta"];
                if str_of(delta, "type") == "input_json_delta" {
                    let Some(block) = self.plan_blocks.get_mut(&(msg_id, index)) else { return Vec::new() };
                    block.json.push_str(str_of(delta, "partial_json"));
                    let Some(markdown) = partial_json_string(&block.json, "plan").filter(|m| m.len() > block.sent) else { return Vec::new() };
                    block.sent = markdown.len();
                    return vec![AgentEvent::PlanUpdated { id: block.id.clone(), markdown, streaming: true }];
                }
                let (text, kind) = match str_of(delta, "type") {
                    "text_delta" => (str_of(delta, "text"), BlockKind::Text),
                    "thinking_delta" => (str_of(delta, "thinking"), BlockKind::Thinking),
                    _ => return Vec::new(),
                };
                if text.is_empty() {
                    return Vec::new();
                }
                let Some(block) = self.open_blocks.get_mut(&(msg_id.clone(), index)) else {
                    return Vec::new();
                };
                if !block.created {
                    block.created = true;
                    self.streamed.entry(msg_id).or_default().push(Streamed { kind, claimed: false });
                }
                let (parent, id) = (block.parent.clone(), block.id.clone());
                let delta = text.to_string();
                vec![match kind {
                    BlockKind::Text => AgentEvent::TextDelta { parent, id, delta },
                    BlockKind::Thinking => AgentEvent::ThinkingDelta { parent, id, delta },
                }]
            }
            "content_block_stop" if self.plan_blocks.remove(&(msg_id.clone(), index)).is_some() => Vec::new(),
            "content_block_stop" => match self.open_blocks.remove(&(msg_id, index)) {
                Some(b) if b.created => vec![AgentEvent::ItemDone { parent: b.parent, id: b.id }],
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    /// 完整消息里的文本/思考块是否已被流式增量展示过;命中则认领并返回 true。
    fn claim_streamed(&mut self, msg_id: &str, kind: BlockKind) -> bool {
        if let Some(list) = self.streamed.get_mut(msg_id)
            && let Some(s) = list.iter_mut().find(|s| s.kind == kind && !s.claimed)
        {
            s.claimed = true;
            return true;
        }
        false
    }

    // ---------- assistant ----------

    fn assistant(&mut self, v: &Value) -> Vec<AgentEvent> {
        let parent = self.parent_of(v);
        let message = &v["message"];
        let msg_id = str_of(message, "id").to_string();
        let mut out = Vec::new();
        let Some(blocks) = message.get("content").and_then(Value::as_array) else {
            return out;
        };
        for block in blocks {
            match str_of(block, "type") {
                "text" => {
                    let text = str_of(block, "text");
                    if text.trim().is_empty() || self.claim_streamed(&msg_id, BlockKind::Text) {
                        continue;
                    }
                    // API 错误:claude 先发一条合成的 assistant 消息,回合结束的 `result` 又带同样的文字。
                    // 实时流里只留 `result` 那条错误提示(见 `result`),这里记下并跳过;回放历史没有 `result`,照常显示
                    if !self.replay && is_api_error_message(v, text) {
                        self.api_error = Some(text.to_string());
                        continue;
                    }
                    let id = self.next_id("a");
                    out.push(AgentEvent::Append {
                        parent: parent.clone(),
                        item: TimelineItem::Assistant { id, text: text.into(), streaming: false },
                    });
                }
                "thinking" => {
                    let text = str_of(block, "thinking");
                    if self.claim_streamed(&msg_id, BlockKind::Thinking) || text.trim().is_empty() {
                        continue;
                    }
                    let id = self.next_id("th");
                    out.push(AgentEvent::Append {
                        parent: parent.clone(),
                        item: TimelineItem::Thinking { id, text: text.into(), streaming: false },
                    });
                }
                "tool_use" => out.extend(self.tool_use(block, parent.clone())),
                _ => {}
            }
        }
        out
    }

    fn tool_use(&mut self, block: &Value, parent: Option<String>) -> Vec<AgentEvent> {
        let id = str_of(block, "id").to_string();
        let name = str_of(block, "name").to_string();
        let input = block.get("input").cloned().unwrap_or(Value::Null);
        self.tools.insert(
            id.clone(),
            ToolInfo {
                name: name.clone(),
                input: input.clone(),
                started: (!self.replay).then(Instant::now),
                parent: parent.clone(),
            },
        );
        match name.as_str() {
            "TodoWrite" => vec![AgentEvent::Todos { id, items: parse_todos(&input) }],
            "AskUserQuestion" => Vec::new(),
            "ExitPlanMode" => {
                if self.replay {
                    // 历史里同一个 tool_use 会先后落几份快照(后面的可能是空输入),空正文的不显示;
                    // 批准结果由随后的 tool_result 决定(没有结果 = 还没批准)
                    let markdown = str_of(&input, "plan").to_string();
                    if markdown.trim().is_empty() {
                        Vec::new()
                    } else {
                        vec![AgentEvent::Append {
                            parent,
                            item: TimelineItem::PlanApproval(PlanApproval { id, markdown, approved: None, streaming: false }),
                        }]
                    }
                } else {
                    Vec::new()
                }
            }
            "Task" | "Agent" => {
                self.subagents.insert(id.clone());
                let agent_type = input.get("subagent_type").and_then(Value::as_str).unwrap_or("general-purpose");
                // 回放历史没有时间戳可用,不计时(started_ms 置空,界面不显示用时)
                let mut card = SubagentCard::new(&id, agent_type, str_of(&input, "description"), SubagentStatus::Running);
                if self.replay {
                    card.started_ms = None;
                    card.last_activity_ms = None;
                }
                card.background = input.get("run_in_background").and_then(Value::as_bool).unwrap_or(false);
                if card.background {
                    card.status = SubagentStatus::Background;
                }
                vec![AgentEvent::Append { parent, item: TimelineItem::Subagent(card) }]
            }
            _ => {
                let tool = build_tool(&id, &name, &input, &self.cwd);
                vec![AgentEvent::Append { parent, item: TimelineItem::Tool(tool) }]
            }
        }
    }

    // ---------- user(工具结果 / 回放中的用户输入) ----------

    fn user(&mut self, v: &Value) -> Vec<AgentEvent> {
        let parent = self.parent_of(v);
        let content = &v["message"]["content"];
        let mut out = Vec::new();

        if let Some(blocks) = content.as_array() {
            let mut texts = Vec::new();
            let mut images = Vec::new();
            for block in blocks {
                match str_of(block, "type") {
                    // 回放时用户消息里的图片:落盘后按路径展示
                    "image" if self.replay => images.extend(crate::image_cache::store(block)),
                    // 实时输出是 tool_use_result,历史 jsonl 里是驼峰的 toolUseResult
                    "tool_result" => out.extend(self.tool_result(
                        block,
                        v.get("tool_use_result").or_else(|| v.get("toolUseResult")),
                        parent.clone(),
                    )),
                    "text" => texts.push(str_of(block, "text")),
                    _ => {}
                }
            }
            if self.replay && (!texts.is_empty() || !images.is_empty()) {
                out.extend(self.replay_user(&texts.join("\n"), images, v));
            }
        } else if let Some(text) = content.as_str()
            && self.replay
        {
            out.extend(self.replay_user(text, Vec::new(), v));
        }
        out
    }

    /// 回放时把真实用户输入还原成条目;过滤掉元信息与斜杠命令产生的内部文本。
    fn replay_user(&mut self, text: &str, images: Vec<String>, v: &Value) -> Vec<AgentEvent> {
        // 后台任务完成时 claude 把通知当作「用户消息」注入;它不是用户输入,转成任务通知
        if text.trim_start().starts_with("<task-notification>") {
            return self.replay_task_notification(text);
        }
        let internal = v.get("isMeta").and_then(Value::as_bool).unwrap_or(false)
            || v.get("isSidechain").and_then(Value::as_bool).unwrap_or(false)
            || text.starts_with("<command-")
            || text.starts_with("<local-command")
            || text.starts_with("Caveat:")
            || text.starts_with("<system-reminder>")
            // 中断时 claude 写入的标记(不带 isMeta),不是用户输入
            || text.starts_with("[Request interrupted by user")
            || (text.trim().is_empty() && images.is_empty());
        if internal {
            return Vec::new();
        }
        let id = self.next_id("u");
        let mut out = vec![AgentEvent::Append {
            parent: None,
            item: TimelineItem::User { id: id.clone(), text: text.into(), images, files: Vec::new() },
        }];
        // jsonl 里这条记录的 uuid 就是文件检查点的键
        if let Some(uuid) = v.get("uuid").and_then(Value::as_str) {
            out.push(AgentEvent::UserUuid { id, uuid: uuid.into() });
        }
        out
    }

    /// 回放时解析 `<task-notification>` 注入消息。
    fn replay_task_notification(&mut self, text: &str) -> Vec<AgentEvent> {
        let task_id = tag_text(text, "task-id");
        if task_id.is_empty() {
            return Vec::new();
        }
        vec![AgentEvent::TaskNotification {
            task_id,
            status: TaskStatus::from_cli(&tag_text(text, "status")),
            summary: tag_text(text, "summary"),
            output_file: Some(tag_text(text, "output-file")).filter(|f| !f.is_empty()),
        }]
    }

    /// 子 Agent 的 `tool_result`:后台启动不能当作完成;前台结束带上统计与结果摘要。
    fn subagent_result(&mut self, id: &str, block: &Value, tur: Option<&Value>, is_error: bool, info: Option<ToolInfo>) -> Vec<AgentEvent> {
        let (text, _) = result_text(&block["content"]);
        let launched = tur.is_some_and(|t| t.get("isAsync").and_then(Value::as_bool).unwrap_or(false) || str_of(t, "status") == "async_launched")
            || text.starts_with("Async agent launched");
        if launched && !is_error {
            let mut out = vec![AgentEvent::SubagentStatus { id: id.into(), status: SubagentStatus::Background }];
            let agent_id = tur.map(|t| str_of(t, "agentId")).filter(|a| !a.is_empty()).map(String::from).or_else(|| {
                text.split("agentId:").nth(1).and_then(|r| r.split_whitespace().next()).map(String::from)
            });
            let output_file = tur.map(|t| str_of(t, "outputFile")).filter(|f| !f.is_empty()).map(String::from);
            if let Some(task_id) = agent_id {
                if self.replay {
                    let input = info.as_ref().map(|i| i.input.clone()).unwrap_or(Value::Null);
                    let desc = [str_of(&input, "description"), str_of(&input, "prompt")].into_iter().find(|d| !d.is_empty()).unwrap_or("后台子代理");
                    let mut task = BackgroundTask::new(&task_id, TaskKind::Agent, desc.lines().next().unwrap_or_default()).with_tool_use(id);
                    task.output_file = output_file;
                    out.push(AgentEvent::TaskStarted(task));
                } else {
                    out.push(AgentEvent::TaskUpdated { task_id, status: None, description: None, output_file });
                }
            }
            return out;
        }
        let summary = match tur.and_then(|t| t.get("content")) {
            Some(c) => result_text(c).0,
            None => text,
        };
        let summary: String = summary.trim().chars().take(400).collect();
        let tokens = tur.and_then(|t| t.get("totalTokens")).and_then(Value::as_u64).unwrap_or(0);
        let tool_count = tur.and_then(|t| t.get("totalToolUseCount")).and_then(Value::as_u64).unwrap_or(0) as u32;
        let duration_ms = tur.and_then(|t| t.get("totalDurationMs")).and_then(Value::as_u64);
        let status = if is_error { SubagentStatus::Failed } else { SubagentStatus::Completed };
        vec![AgentEvent::SubagentFinished { id: id.into(), status, summary, tokens, tool_count, duration_ms }]
    }

    fn tool_result(&mut self, block: &Value, tur: Option<&Value>, parent: Option<String>) -> Vec<AgentEvent> {
        let id = str_of(block, "tool_use_id").to_string();
        let is_error = block.get("is_error").and_then(Value::as_bool).unwrap_or(false);
        let info = self.tools.remove(&id);

        if self.subagents.contains(&id) {
            return self.subagent_result(&id, block, tur, is_error, info);
        }
        let Some(info) = info else {
            return Vec::new();
        };
        if self.replay && info.name == "ExitPlanMode" {
            // 工具结果是用户的决定:报错 = 没批准(「用户没有批准该计划…」),否则批准
            return vec![AgentEvent::PlanResolved { id, approved: !is_error }];
        }
        if matches!(info.name.as_str(), "TodoWrite" | "AskUserQuestion" | "ExitPlanMode") {
            return Vec::new();
        }

        let (text, images) = result_text(&block["content"]);
        let mut patch = ToolPatch {
            status: Some(if is_error { ToolStatus::Failed } else { ToolStatus::Completed }),
            duration_ms: info.started.map(|s| s.elapsed().as_millis() as u64),
            ..Default::default()
        };
        let kind = ToolKind::from_tool_name(&info.name);

        match kind {
            // 后台 Bash:启动结果只是「已在后台运行」,输出在任务面板里看,不当作终端输出
            ToolKind::Execute if info.name == "Bash" && !is_error && background_task_id(&info.input, tur, &text).is_some() => {
                let task_id = background_task_id(&info.input, tur, &text).unwrap_or_default();
                patch.task_id = Some(task_id.clone());
                let output_file = background_output_file(&text);
                let mut out = Vec::new();
                if self.replay {
                    // 回放的历史没有 system/task_started,这里补一个任务;实时模式由 system 消息创建
                    let mut task = BackgroundTask::new(&task_id, TaskKind::Shell, &build_tool(&id, &info.name, &info.input, &self.cwd).title).with_tool_use(&id);
                    task.output_file = output_file;
                    out.push(AgentEvent::TaskStarted(task));
                } else {
                    out.push(AgentEvent::TaskUpdated { task_id, status: None, description: None, output_file });
                }
                out.push(AgentEvent::ToolUpdate { parent: parent.or(info.parent), id, patch });
                return out;
            }
            ToolKind::Execute if info.name == "Bash" => {
                let command = str_of(&info.input, "command").to_string();
                let output = match tur.filter(|t| t.get("stdout").is_some()) {
                    Some(t) => {
                        let (out, err) = (str_of(t, "stdout"), str_of(t, "stderr"));
                        if err.is_empty() { out.to_string() } else { format!("{out}\n{err}") }
                    }
                    None => text.clone(),
                };
                let exit_code = if is_error { Some(parse_exit_code(&text).unwrap_or(1)) } else { Some(0) };
                patch.append_content.push(ToolContent::Terminal { command, output, exit_code });
            }
            ToolKind::Read if !is_error => {
                if let Some(n) = tur.and_then(|t| t["file"]["numLines"].as_u64()) {
                    patch.title = Some(format!("{} · {n} 行", build_tool(&id, &info.name, &info.input, &self.cwd).title));
                }
            }
            ToolKind::Edit | ToolKind::Write if !is_error => {}
            _ => {
                if !text.trim().is_empty() {
                    patch.append_content.push(ToolContent::Text(truncate(&text, 20_000)));
                }
            }
        }
        if is_error && !matches!(kind, ToolKind::Execute) && !text.trim().is_empty() && patch.append_content.is_empty() {
            patch.append_content.push(ToolContent::Text(truncate(&text, 4_000)));
        }
        for (n, path) in images.into_iter().enumerate() {
            // 读图片文件时标签用文件名,其余用「图片 N」
            let label = match (&kind, &path) {
                (ToolKind::Read, Some(_)) => str_of(&info.input, "file_path").rsplit('/').next().unwrap_or("图片").to_string(),
                _ => format!("图片 {}", n + 1),
            };
            patch.append_content.push(ToolContent::Image { label, path });
        }
        vec![AgentEvent::ToolUpdate { parent: parent.or(info.parent), id, patch }]
    }

    /// 回合进行中用某次 API 调用的用量刷新「当前上下文占用」:只认主对话(子 Agent 有自己的上下文),
    /// 数没变或没有输入侧用量时不发事件(免得每个流事件都重绘)。
    fn live_context(&mut self, usage: &Value, main: bool) -> Vec<AgentEvent> {
        let used = context_tokens(usage);
        if !main || used == 0 || used == self.usage.context_used {
            return Vec::new();
        }
        self.usage.context_used = used;
        vec![AgentEvent::Usage(self.usage.clone())]
    }

    // ---------- result / 限额 ----------

    fn result(&mut self, v: &Value) -> Vec<AgentEvent> {
        let mut out = Vec::new();

        if let Some(cost) = v.get("total_cost_usd").and_then(Value::as_f64) {
            self.usage.cost_usd = cost;
        }
        if let Some(size) = v
            .get("modelUsage")
            .and_then(Value::as_object)
            .and_then(|m| m.values().filter_map(|u| u.get("contextWindow").and_then(Value::as_u64)).max())
        {
            self.usage.context_size = size;
        }
        let usage = &v["usage"];
        // 取最后一次迭代的用量作为当前上下文占用;没有迭代信息时用整体用量
        let last = usage.get("iterations").and_then(Value::as_array).and_then(|a| a.last()).unwrap_or(usage);
        let used: u64 = ["input_tokens", "cache_read_input_tokens", "cache_creation_input_tokens", "output_tokens"]
            .iter()
            .map(|k| last.get(*k).and_then(Value::as_u64).unwrap_or(0))
            .sum();
        if used > 0 {
            self.usage.context_used = used;
        }
        let cache = |u: &Value| {
            let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
            CacheTokens { read: n("cache_read_input_tokens"), written: n("cache_creation_input_tokens"), uncached: n("input_tokens") }
        };
        let (last_cache, turn_cache) = (cache(last), cache(usage));
        if last_cache.total() > 0 {
            self.usage.cache_last = last_cache;
        }
        let total = &mut self.usage.cache_total;
        (total.read, total.written, total.uncached) = (total.read + turn_cache.read, total.written + turn_cache.written, total.uncached + turn_cache.uncached);
        out.push(AgentEvent::Usage(self.usage.clone()));
        let stats = turn_stats(v);
        if !stats.is_empty() {
            out.push(AgentEvent::TurnStats { id: None, stats });
        }

        let is_error = v.get("is_error").and_then(Value::as_bool).unwrap_or(false);
        // 合成的 API 错误消息被跳过了,而这次 `result` 又没标错误:把错误文字补成提示,不能丢
        if let Some(text) = self.api_error.take().filter(|_| !is_error) {
            out.push(notice(NoticeLevel::Error, &text));
        }
        let reason = if std::mem::take(&mut self.interrupted) {
            StopReason::Cancelled
        } else if is_error {
            let text = match v.get("result").and_then(Value::as_str).filter(|t| !t.is_empty()) {
                Some(t) => t.to_string(),
                None => match v.get("errors").and_then(Value::as_array) {
                    Some(errs) if !errs.is_empty() => {
                        errs.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n")
                    }
                    _ => format!("回合失败({})", str_of(v, "subtype")),
                },
            };
            out.push(notice(NoticeLevel::Error, &text));
            StopReason::Error
        } else {
            match str_of(v, "stop_reason") {
                "refusal" => StopReason::Refusal,
                "max_tokens" => StopReason::MaxTokens,
                _ => StopReason::EndTurn,
            }
        };
        // 回合结束后,流式状态不应再有残留
        self.open_blocks.clear();
        self.streamed.clear();
        // 计划卡生成到一半回合就结束了(被中断 / 出错):没有等到权限请求,按未批准收尾,免得留下点不动的 Build 按钮
        self.plan_blocks.clear();
        for id in std::mem::take(&mut self.plans_streamed) {
            out.push(AgentEvent::PlanResolved { id, approved: false });
        }
        out.push(AgentEvent::TurnEnded(reason));
        out
    }

    /// 开始查询账户限额:返回带「查询中」状态的用量快照。
    pub fn usage_checking(&mut self) -> AgentEvent {
        self.usage.limits_state = LimitsState::Checking;
        AgentEvent::Usage(self.usage.clone())
    }

    /// 处理 `mcp_status` 的应答:每个服务器的状态、连接失败的原因与工具列表(含插件与 claude.ai 连接器带来的)。
    /// 格式不符或失败返回 `None`。
    pub fn mcp_status_event(&mut self, body: &Value, ok: bool) -> Option<AgentEvent> {
        let list = body.get("mcpServers")?.as_array().filter(|_| ok)?;
        Some(AgentEvent::McpServers(
            list.iter()
                .filter_map(|s| {
                    let name = s.get("name").and_then(Value::as_str)?.to_string();
                    let tools = s.get("tools").and_then(Value::as_array).map(|a| a.iter().filter_map(|t| t.get("name").and_then(Value::as_str)).map(|t| bare_tool(t, &name).to_string()).collect()).unwrap_or_default();
                    Some(McpServerStatus { conn: McpConn::parse(str_of(s, "status")), error: s.get("error").and_then(Value::as_str).and_then(short_error), name, tools })
                })
                .collect(),
        ))
    }

    /// 处理 `get_context_usage` 的应答:按类别的明细与总量;失败或格式不符时返回 `None`(旧版 claude 可能不支持)。
    pub fn context_breakdown(&mut self, body: &Value, ok: bool) -> Option<AgentEvent> {
        let cats = body.get("categories")?.as_array().filter(|_| ok)?;
        let list: Vec<ContextCategory> = cats
            .iter()
            .filter_map(|c| {
                let kind = match str_of(c, "kind") {
                    "free" => ContextKind::Free,
                    "buffer" => ContextKind::Buffer,
                    "deferred" => ContextKind::Deferred,
                    _ => ContextKind::Used,
                };
                Some(ContextCategory { name: c.get("name")?.as_str()?.to_string(), tokens: c.get("tokens")?.as_u64()?, kind })
            })
            .collect();
        if list.is_empty() {
            return None;
        }
        if let Some(max) = body.get("maxTokens").and_then(Value::as_u64).filter(|m| *m > 0) {
            self.usage.context_size = max;
        }
        if let Some(total) = body.get("totalTokens").and_then(Value::as_u64) {
            self.usage.context_used = total;
        }
        self.usage.context_categories = list;
        // 各 MCP 工具的占用(按服务器汇总用):服务器名字段已是工具名里的写法
        self.usage.mcp_tools = body
            .get("mcpTools")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|t| {
                        let server = t.get("serverName").and_then(Value::as_str)?.to_string();
                        let full = t.get("name").and_then(Value::as_str)?;
                        let tool = full.strip_prefix(&format!("mcp__{server}__")).unwrap_or(full).to_string();
                        Some(McpToolUsage { server, tool, tokens: t.get("tokens").and_then(Value::as_u64).unwrap_or(0), loaded: t.get("isLoaded").and_then(Value::as_bool).unwrap_or(true) })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(AgentEvent::Usage(self.usage.clone()))
    }

    /// 处理 `get_usage` 的应答:`rate_limits.five_hour / seven_day` 的 `utilization` 已是百分比。
    /// 失败或当前账户没有限额时置为「不可用」。
    pub fn account_usage(&mut self, body: &Value, ok: bool) -> AgentEvent {
        let limits = &body["rate_limits"];
        let pct = |k: &str| limits[k].get("utilization").and_then(Value::as_f64).map(|u| u as f32);
        let (five, weekly) = (pct("five_hour"), pct("seven_day"));
        let resets = |k: &str| limits[k].get("resets_at").and_then(Value::as_str).and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map(|t| t.timestamp());
        if ok && (five.is_some() || weekly.is_some()) {
            self.usage.five_hour_resets = resets("five_hour").or(self.usage.five_hour_resets);
            self.usage.weekly_resets = resets("seven_day").or(self.usage.weekly_resets);
            self.usage.five_hour_pct = five.or(self.usage.five_hour_pct);
            self.usage.weekly_pct = weekly.or(self.usage.weekly_pct);
            self.usage.limits_state = LimitsState::Idle;
            self.usage.limits_at = Some(now_unix());
        } else {
            self.usage.limits_state = LimitsState::Unavailable;
        }
        AgentEvent::Usage(self.usage.clone())
    }

    fn rate_limit(&mut self, v: &Value) -> Vec<AgentEvent> {
        let info = &v["rate_limit_info"];
        let windows = &info["unifiedWindows"];
        let pct = |k: &str| windows[k].get("utilization").and_then(Value::as_f64).map(|u| (u * 100.0) as f32);
        if let Some(p) = pct("five_hour") {
            self.usage.five_hour_pct = Some(p);
            self.usage.limits_at = Some(now_unix());
        }
        if let Some(p) = pct("seven_day") {
            self.usage.weekly_pct = Some(p);
            self.usage.limits_at = Some(now_unix());
        }
        let mut out = vec![AgentEvent::Usage(self.usage.clone())];
        let status = str_of(info, "status");
        if !status.is_empty() && !matches!(status, "allowed" | "allowed_warning") {
            let which = match str_of(info, "rateLimitType") {
                "five_hour" => "5 小时",
                "seven_day" | "seven_day_opus" | "seven_day_sonnet" => "每周",
                _ => "",
            };
            out.push(notice(NoticeLevel::Warning, &format!("已触及{which}使用限额,请稍后再试")));
        }
        out
    }

    // ---------- 控制请求 ----------

    /// 处理 claude 发来的控制请求。返回要追加的事件,以及需要立刻回复的应答
    /// (只有不支持的请求才会立刻回复;权限类请求等用户决定后由调用方应答)。
    pub fn control_request(&mut self, v: &Value) -> (Vec<AgentEvent>, Option<Value>) {
        let request_id = str_of(v, "request_id").to_string();
        let req = &v["request"];
        match str_of(req, "subtype") {
            "can_use_tool" => (self.can_use_tool(&request_id, req), None),
            "elicitation" => {
                // MCP 服务向用户索取输入:暂不支持在对话里填写,自动拒绝,但要让用户知道发生了什么
                let server = ["mcp_server_name", "server_name", "serverName"].iter().map(|k| str_of(req, k)).find(|s| !s.is_empty()).unwrap_or("MCP 服务");
                let message = str_of(req, "message");
                let text = if message.is_empty() { format!("{server} 请求输入,已自动拒绝(暂不支持在对话里填写)") } else { format!("{server} 请求输入「{message}」,已自动拒绝(暂不支持在对话里填写)") };
                (vec![notice(NoticeLevel::Info, &text)], Some(crate::protocol::success_response(&request_id, serde_json::json!({"action": "decline"}))))
            }
            "hook_callback" => (Vec::new(), Some(crate::protocol::success_response(&request_id, serde_json::json!({})))),
            other => (Vec::new(), Some(crate::protocol::error_response(&request_id, &format!("不支持的控制请求:{other}")))),
        }
    }

    fn can_use_tool(&mut self, request_id: &str, req: &Value) -> Vec<AgentEvent> {
        let tool_name = str_of(req, "tool_name").to_string();
        let input = req.get("input").cloned().unwrap_or(Value::Null);
        let tool_use_id = req.get("tool_use_id").and_then(Value::as_str).map(String::from);
        let suggestions = req.get("permission_suggestions").and_then(Value::as_array).cloned().unwrap_or_default();
        let item_id = tool_use_id.clone().unwrap_or_else(|| request_id.to_string());
        let parent = tool_use_id
            .as_ref()
            .and_then(|id| self.tools.get(id))
            .and_then(|t| t.parent.clone());

        let kind = match tool_name.as_str() {
            "AskUserQuestion" => PendingKind::Question,
            "ExitPlanMode" => PendingKind::Plan,
            _ => PendingKind::Permission,
        };
        let pending = Pending { request_id: request_id.into(), tool_use_id, tool_name: tool_name.clone(), input: input.clone(), suggestions, kind };

        let event = match kind {
            PendingKind::Question => AgentEvent::Append {
                parent,
                item: TimelineItem::Question(QuestionRequest {
                    id: item_id.clone(),
                    questions: parse_questions(&input),
                    answers: None,
                }),
            },
            // 计划已经以流式卡片出现过:定稿正文(此时才有 Build 按钮),不重复追加
            PendingKind::Plan if self.plans_streamed.remove(&item_id) => {
                AgentEvent::PlanUpdated { id: item_id.clone(), markdown: str_of(&input, "plan").into(), streaming: false }
            }
            PendingKind::Plan => AgentEvent::Append {
                parent,
                item: TimelineItem::PlanApproval(PlanApproval {
                    id: item_id.clone(),
                    markdown: str_of(&input, "plan").into(),
                    approved: None,
                    streaming: false,
                }),
            },
            PendingKind::Permission => {
                let tool = build_tool(&item_id, &tool_name, &input, &self.cwd);
                AgentEvent::PromoteToPermission {
                    parent,
                    request: PermissionRequest {
                        id: item_id.clone(),
                        tool,
                        options: permission_options(&pending),
                        resolved: None,
                    },
                }
            }
        };
        self.pending.insert(item_id, pending);
        vec![event]
    }

    /// claude 撤销了某个控制请求(例如回合被中断):把对应卡片标记为已拒绝。
    pub fn cancel_request(&mut self, request_id: &str) -> Vec<AgentEvent> {
        let key = self.pending.iter().find(|(_, p)| p.request_id == request_id).map(|(k, _)| k.clone());
        let Some(key) = key else { return Vec::new() };
        let Some(p) = self.pending.remove(&key) else { return Vec::new() };
        match p.kind {
            PendingKind::Permission => vec![AgentEvent::PermissionResolved { id: key, option: "reject".into() }],
            PendingKind::Question => vec![AgentEvent::QuestionAnswered { id: key, answers: Vec::new() }],
            PendingKind::Plan => vec![AgentEvent::PlanResolved { id: key, approved: false }],
        }
    }
}

// ---------- 辅助函数 ----------

fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// 取 `<tag>…</tag>` 之间的文本。
fn tag_text(text: &str, tag: &str) -> String {
    let (open, close) = (format!("<{tag}>"), format!("</{tag}>"));
    let Some(start) = text.find(&open).map(|i| i + open.len()) else { return String::new() };
    let end = text[start..].find(&close).map(|i| start + i).unwrap_or(text.len());
    text[start..end].trim().to_string()
}

/// 后台 Bash 的任务 id:优先 `toolUseResult.backgroundTaskId`,其次解析「running in background with ID: xxx」。
fn background_task_id(input: &Value, tur: Option<&Value>, text: &str) -> Option<String> {
    if let Some(id) = tur.and_then(|t| t.get("backgroundTaskId")).and_then(Value::as_str).filter(|s| !s.is_empty()) {
        return Some(id.into());
    }
    if input.get("run_in_background").and_then(Value::as_bool).unwrap_or(false) || text.contains("running in background with ID:") {
        let id = text.split("with ID:").nth(1)?.trim().split(|c: char| c.is_whitespace() || c == '.').next()?;
        return Some(id.trim_end_matches('.').to_string()).filter(|s| !s.is_empty());
    }
    None
}

/// 解析「Output is being written to: <path>.」里的输出文件路径。
fn background_output_file(text: &str) -> Option<String> {
    let rest = text.split("written to:").nth(1)?.trim();
    let end = rest.find(".output").map(|i| i + ".output".len())?;
    Some(rest[..end].trim().to_string())
}

fn notice(level: NoticeLevel, text: &str) -> AgentEvent {
    AgentEvent::Notice { level, text: text.into() }
}

/// 权限卡的选项。「总是允许」的文案取决于 claude 给出的建议。
pub fn permission_options(pending: &Pending) -> Vec<PermissionOption> {
    let always_label = if pending.suggestions.iter().any(|s| {
        s.get("type").and_then(Value::as_str) == Some("setMode") && s.get("mode").and_then(Value::as_str) == Some("acceptEdits")
    }) {
        "允许并自动接受编辑"
    } else if pending.suggestions.iter().any(|s| s.get("type").and_then(Value::as_str) == Some("addRules")) {
        "总是允许"
    } else {
        "总是允许此类操作"
    };
    vec![
        PermissionOption { id: "allow_once".into(), label: "允许一次".into(), kind: PermissionOptionKind::AllowOnce },
        PermissionOption { id: "allow_always".into(), label: always_label.into(), kind: PermissionOptionKind::AllowAlways },
        PermissionOption { id: "reject".into(), label: "拒绝".into(), kind: PermissionOptionKind::RejectOnce },
    ]
}

fn parse_todos(input: &Value) -> Vec<TodoItem> {
    input
        .get("todos")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|t| TodoItem {
                    content: str_of(t, "content").into(),
                    status: match str_of(t, "status") {
                        "in_progress" => TodoStatus::InProgress,
                        "completed" => TodoStatus::Completed,
                        _ => TodoStatus::Pending,
                    },
                    active_form: Some(str_of(t, "activeForm")).filter(|s| !s.is_empty()).map(str::to_string),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_questions(input: &Value) -> Vec<Question> {
    input
        .get("questions")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|q| Question {
                    header: str_of(q, "header").into(),
                    question: str_of(q, "question").into(),
                    multi_select: q.get("multiSelect").and_then(Value::as_bool).unwrap_or(false),
                    options: q
                        .get("options")
                        .and_then(Value::as_array)
                        .map(|o| {
                            o.iter()
                                .map(|o| QuestionOption { label: str_of(o, "label").into(), description: str_of(o, "description").into() })
                                .collect()
                        })
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 工具结果内容 → (文本, 图片本地路径列表;落盘失败的为 `None`)。
/// 从 `result` 事件取出本回合的消耗统计。
///
/// `usage` 是本回合所有模型调用的合计;费用用 `model_usage_since_last_result`(单回合,
/// `total_cost_usd` 是会话累计值,不能直接用);没有费用(或为 0,如第三方服务商)时为 `None`。
fn turn_stats(v: &Value) -> TurnStats {
    let u = &v["usage"];
    let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
    let cost: f64 = v.get("model_usage_since_last_result").and_then(Value::as_object).map_or(0., |m| m.values().filter_map(|x| x.get("costUSD").and_then(Value::as_f64)).sum());
    // 主模型:本回合输出最多的那个(子 Agent 可能用了别的模型)
    let model = v.get("model_usage_since_last_result").and_then(Value::as_object).and_then(|m| {
        m.iter().max_by_key(|(_, x)| x.get("outputTokens").and_then(Value::as_u64).unwrap_or(0)).map(|(k, x)| x.get("canonicalModel").and_then(Value::as_str).unwrap_or(k).to_string())
    });
    TurnStats {
        model,
        input: n("input_tokens"),
        output: n("output_tokens"),
        thinking: u.pointer("/output_tokens_details/thinking_tokens").and_then(Value::as_u64).unwrap_or(0),
        cache_read: n("cache_read_input_tokens"),
        cache_write: n("cache_creation_input_tokens"),
        cost_usd: (cost > 0.).then_some(cost),
        api_ms: v.get("duration_api_ms").and_then(Value::as_u64),
        ttft_ms: v.get("ttft_ms").and_then(Value::as_u64),
        calls: v.get("num_turns").and_then(Value::as_u64).map(|n| n as u32),
    }
}

fn result_text(content: &Value) -> (String, Vec<Option<String>>) {
    match content {
        Value::String(s) => (s.clone(), Vec::new()),
        Value::Array(blocks) => {
            let mut texts = Vec::new();
            let mut images = Vec::new();
            for b in blocks {
                match str_of(b, "type") {
                    "text" => texts.push(str_of(b, "text").to_string()),
                    "image" => images.push(crate::image_cache::store(b)),
                    _ => {}
                }
            }
            (texts.join("\n"), images)
        }
        _ => (String::new(), Vec::new()),
    }
}

fn parse_exit_code(text: &str) -> Option<i32> {
    let rest = text.strip_prefix("Exit code ")?;
    rest.split(|c: char| !c.is_ascii_digit() && c != '-').next()?.parse().ok()
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.into();
    }
    let cut: String = text.chars().take(max).collect();
    format!("{cut}\n…(已截断)")
}

/// 路径显示:位于工作目录内的显示相对路径,否则保留末尾三段。
pub fn short_path(path: &str, cwd: &str) -> String {
    if !cwd.is_empty()
        && let Some(rest) = path.strip_prefix(cwd)
    {
        let rest = rest.trim_start_matches('/');
        if !rest.is_empty() {
            return rest.into();
        }
    }
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() <= 3 { path.into() } else { format!("…/{}", parts[parts.len() - 3..].join("/")) }
}

/// 由工具名与入参构造工具卡。
pub fn build_tool(id: &str, name: &str, input: &Value, cwd: &str) -> ToolCall {
    let path = ["file_path", "notebook_path", "path"].iter().find_map(|k| input.get(*k).and_then(Value::as_str));
    let shown = path.map(|p| short_path(p, cwd)).unwrap_or_default();
    let first_line = |s: &str| {
        let line = s.lines().next().unwrap_or_default().trim();
        if line.chars().count() > 80 { format!("{}…", line.chars().take(80).collect::<String>()) } else { line.to_string() }
    };
    let title = match name {
        "Read" | "NotebookRead" => format!("读取 {shown}"),
        "Edit" | "MultiEdit" => format!("编辑 {shown}"),
        "Write" => format!("写入 {shown}"),
        "NotebookEdit" => format!("编辑笔记本 {shown}"),
        "Bash" => {
            let desc = str_of(input, "description");
            if desc.is_empty() { first_line(str_of(input, "command")) } else { desc.into() }
        }
        "BashOutput" | "TaskOutput" => "读取后台输出".into(),
        "KillShell" | "KillBash" | "TaskStop" => "结束后台任务".into(),
        "Monitor" => {
            let desc = str_of(input, "description");
            format!("监听 {}", if desc.is_empty() { first_line(str_of(input, "command")) } else { desc.into() })
        }
        "Glob" => format!("查找 {}", str_of(input, "pattern")),
        "Grep" => format!("搜索 {}", str_of(input, "pattern")),
        "LS" => format!("列出 {shown}"),
        "WebFetch" => format!("抓取 {}", str_of(input, "url")),
        "WebSearch" => format!("搜索网页:{}", str_of(input, "query")),
        "ToolSearch" => format!("查找工具:{}", str_of(input, "query")),
        "EnterPlanMode" => "进入计划模式".into(),
        n if n.starts_with("mcp__") => {
            let mut parts = n.splitn(3, "__").skip(1);
            let server = parts.next().unwrap_or_default();
            let tool = parts.next().unwrap_or_default();
            format!("{server} · {tool}")
        }
        n => n.to_string(),
    };

    let mut tool = ToolCall::new(id, name, title.trim()).with_input(input.clone());
    // 读取输出 / 结束任务的工具关联到被操作的任务
    if matches!(name, "BashOutput" | "KillShell" | "KillBash" | "TaskOutput" | "TaskStop") {
        tool.task_id = ["task_id", "bash_id", "shell_id"].iter().find_map(|k| input.get(*k).and_then(Value::as_str)).map(String::from);
    }
    if let Some(p) = path {
        tool.locations.push(p.into());
    }
    match name {
        "Edit" => {
            if let Some(p) = path {
                tool.content.push(ToolContent::Diff {
                    path: p.into(),
                    old: str_of(input, "old_string").into(),
                    new: str_of(input, "new_string").into(),
                });
            }
        }
        "MultiEdit" => {
            if let (Some(p), Some(edits)) = (path, input.get("edits").and_then(Value::as_array)) {
                for e in edits {
                    tool.content.push(ToolContent::Diff {
                        path: p.into(),
                        old: str_of(e, "old_string").into(),
                        new: str_of(e, "new_string").into(),
                    });
                }
            }
        }
        "Write" => {
            if let Some(p) = path {
                tool.content.push(ToolContent::Diff { path: p.into(), old: String::new(), new: str_of(input, "content").into() });
            }
        }
        _ => {}
    }
    tool
}

#[cfg(test)]
mod tests {
    #[::core::prelude::v1::test]
    fn result_带出回合消耗统计() {
        let mut t = Translator::new("/x");
        let ev = t.handle(&json!({"type":"result","subtype":"success","is_error":false,"result":"ok","duration_api_ms":4233,"ttft_ms":2916,"num_turns":2,"total_cost_usd":9.9,
            "usage":{"input_tokens":4,"output_tokens":230,"output_tokens_details":{"thinking_tokens":149},"cache_read_input_tokens":35613,"cache_creation_input_tokens":19732},
            "model_usage_since_last_result":{"claude-haiku-5-5":{"costUSD":0.0044,"outputTokens":230}}}));
        let stats = ev.iter().find_map(|e| if let AgentEvent::TurnStats { stats, .. } = e { Some(stats.clone()) } else { None }).expect("有 TurnStats");
        assert_eq!((stats.input, stats.output, stats.thinking, stats.cache_read, stats.cache_write), (4, 230, 149, 35613, 19732));
        // 费用取单回合的 model_usage_since_last_result,不是会话累计的 total_cost_usd
        assert_eq!(stats.cost_usd, Some(0.0044));
        assert_eq!(stats.model.as_deref(), Some("claude-haiku-5-5"));
        assert_eq!((stats.api_ms, stats.ttft_ms, stats.calls), (Some(4233), Some(2916), Some(2)));
        // TurnStats 在 TurnEnded 之前
        let (a, b) = (ev.iter().position(|e| matches!(e, AgentEvent::TurnStats { .. })), ev.iter().position(|e| matches!(e, AgentEvent::TurnEnded(_))));
        assert!(a < b);
    }

    use super::*;
    use serde_json::json;

    #[test]
    fn 上下文明细与缓存() {
        let mut t = Translator::new("/private/tmp/loom-probe");
        let body = json!({"categories": [
            {"name": "System prompt", "tokens": 3000, "kind": "used"},
            {"name": "Messages", "tokens": 9000, "kind": "used"},
            {"name": "Autocompact buffer", "tokens": 33000, "kind": "buffer"},
            {"name": "Free space", "tokens": 155000, "kind": "free"}], "totalTokens": 12000, "maxTokens": 200000});
        let Some(AgentEvent::Usage(u)) = t.context_breakdown(&body, true) else { panic!() };
        assert_eq!((u.context_used, u.context_size, u.context_categories.len()), (12000, 200000, 4));
        assert_eq!(u.context_categories[2].kind, ContextKind::Buffer);
        assert!(t.context_breakdown(&json!({}), true).is_none());
        let cache = CacheTokens { read: 90, written: 5, uncached: 5 };
        assert_eq!(cache.hit_percent(), 90);
        assert_eq!(CacheTokens::default().hit_percent(), 0);
    }

    #[test]
    fn get_usage应答写入限额与状态() {
        let mut t = Translator::new("/private/tmp/loom-probe");
        let AgentEvent::Usage(u) = t.usage_checking() else { panic!() };
        assert_eq!(u.limits_state, LimitsState::Checking);
        let body = json!({"rate_limits": {"five_hour": {"utilization": 42.0}, "seven_day": {"utilization": 7.5}}});
        let AgentEvent::Usage(u) = t.account_usage(&body, true) else { panic!() };
        assert_eq!((u.five_hour_pct, u.weekly_pct, u.limits_state), (Some(42.0), Some(7.5), LimitsState::Idle));
        let AgentEvent::Usage(u) = t.account_usage(&json!({"rate_limits_available": false}), true) else { panic!() };
        assert_eq!(u.limits_state, LimitsState::Unavailable);
    }

    /// 把 jsonl 夹具逐行喂给翻译器。can_use_tool 的应答在测试里省略。
    fn feed(name: &str) -> (Translator, Vec<AgentEvent>) {
        let path = format!("{}/tests/fixtures/{name}.jsonl", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(path).expect("夹具存在");
        let mut t = Translator::new("/private/tmp/loom-probe");
        let mut events = Vec::new();
        for line in text.lines() {
            let v: Value = serde_json::from_str(line).unwrap();
            if str_of(&v, "type") == "control_request" {
                events.extend(t.control_request(&v).0);
            } else {
                events.extend(t.handle(&v));
            }
        }
        (t, events)
    }

    fn reduce(events: Vec<AgentEvent>) -> SessionState {
        let mut s = SessionState::default();
        for e in events {
            s.apply(e);
        }
        s
    }

    #[test]
    fn 简单对话去重并结束() {
        let (_, events) = feed("simple");
        let s = reduce(events);
        let texts: Vec<_> = s
            .items
            .iter()
            .filter_map(|i| match i {
                TimelineItem::Assistant { text, streaming, .. } => Some((text.clone(), *streaming)),
                _ => None,
            })
            .collect();
        assert_eq!(texts, vec![("ok".to_string(), false)], "流式与完整消息只应出现一次");
        assert_eq!(s.last_stop, Some(StopReason::EndTurn));
        assert!(s.usage.context_size >= 1_000_000);
        assert!(s.usage.cost_usd > 0.0);
        assert!(s.usage.five_hour_pct.is_some());
    }

    #[test]
    fn 工具调用与结果() {
        let (_, events) = feed("tools");
        let s = reduce(events);
        let tools: Vec<&ToolCall> = s.items.iter().filter_map(|i| if let TimelineItem::Tool(t) = i { Some(t) } else { None }).collect();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "Read");
        assert_eq!(tools[0].title, "读取 note.txt · 2 行");
        assert_eq!(tools[0].status, ToolStatus::Completed);
        assert_eq!(tools[1].name, "Bash");
        assert!(matches!(tools[1].content[0], ToolContent::Terminal { exit_code: Some(0), .. }));
        assert!(tools.iter().all(|t| t.duration_ms.is_some()));
    }

    #[test]
    fn 权限请求把工具卡升级() {
        let (t, events) = feed("permission");
        let s = reduce(events);
        let perms: Vec<&PermissionRequest> =
            s.items.iter().filter_map(|i| if let TimelineItem::Permission(p) = i { Some(p) } else { None }).collect();
        assert_eq!(perms.len(), 2, "Write 与 Edit 各一张权限卡");
        assert_eq!(perms[0].tool.name, "Write");
        assert!(matches!(&perms[0].tool.content[0], ToolContent::Diff { old, new, .. } if old.is_empty() && new == "hello"));
        assert_eq!(perms[1].tool.name, "Edit");
        assert_eq!(perms[0].options.len(), 3);
        assert_eq!(perms[0].options[1].label, "允许并自动接受编辑");
        // 夹具里没有应答,所以请求仍挂起;工具结果到达后工具已完成
        assert!(t.has_pending());
        assert!(perms.iter().all(|p| p.tool.status == ToolStatus::Completed));
        // 没有残留的普通工具卡(都被升级了)
        assert!(!s.items.iter().any(|i| matches!(i, TimelineItem::Tool(_))));
    }

    #[test]
    fn 子_agent_与待办与提问() {
        let mut t = Translator::new("/w");
        let mut events = Vec::new();
        let lines = [
            json!({"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"task1","name":"Task","input":{"subagent_type":"Explore","description":"查找入口"}}]}}),
            json!({"type":"assistant","parent_tool_use_id":"task1","message":{"id":"m2","content":[{"type":"tool_use","id":"g1","name":"Grep","input":{"pattern":"fn main"}}]}}),
            json!({"type":"user","parent_tool_use_id":"task1","message":{"content":[{"type":"tool_result","tool_use_id":"g1","content":"src/main.rs"}]}}),
            json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"task1","content":"完成"}]}}),
            json!({"type":"assistant","message":{"id":"m3","content":[{"type":"tool_use","id":"td","name":"TodoWrite","input":{"todos":[{"content":"写代码","status":"in_progress","activeForm":"写代码中"},{"content":"测试","status":"pending"}]}}]}}),
            json!({"type":"assistant","message":{"id":"m4","content":[{"type":"tool_use","id":"q1","name":"AskUserQuestion","input":{"questions":[{"question":"用哪个?","header":"库","multiSelect":false,"options":[{"label":"A","description":"a"},{"label":"B","description":"b"}]}]}}]}}),
        ];
        for l in &lines {
            events.extend(t.handle(l));
        }
        let (ev, reply) = t.control_request(&json!({"type":"control_request","request_id":"rq","request":{"subtype":"can_use_tool","tool_name":"AskUserQuestion","tool_use_id":"q1","input":{"questions":[{"question":"用哪个?","header":"库","multiSelect":false,"options":[{"label":"A","description":"a"},{"label":"B","description":"b"}]}]}}}));
        assert!(reply.is_none());
        events.extend(ev);
        let s = reduce(events);

        let TimelineItem::Subagent(card) = &s.items[0] else { panic!("应是子 Agent 卡") };
        assert_eq!(card.agent_type, "Explore");
        assert_eq!(card.status, SubagentStatus::Completed);
        assert_eq!(card.children.len(), 1, "子 Agent 的工具归入嵌套时间线");
        assert!(matches!(&card.children[0], TimelineItem::Tool(t) if t.status == ToolStatus::Completed));
        assert!(matches!(&s.items[1], TimelineItem::Todos { items, .. } if items.len() == 2 && items[0].status == TodoStatus::InProgress && items[0].active_form.as_deref() == Some("写代码中") && items[1].active_form.is_none()));
        let TimelineItem::Question(q) = &s.items[2] else { panic!("应是提问卡") };
        assert_eq!(q.questions[0].options.len(), 2);
        assert!(t.take_pending("q1").is_some());
    }

    #[test]
    fn 计划卡与取消请求() {
        let mut t = Translator::new("/w");
        let (ev, _) = t.control_request(&json!({"request_id":"r9","request":{"subtype":"can_use_tool","tool_name":"ExitPlanMode","tool_use_id":"p1","input":{"plan":"# 计划"},"permission_suggestions":[{"type":"setMode","mode":"acceptEdits","destination":"session"}]}}));
        assert!(matches!(&ev[0], AgentEvent::Append { item: TimelineItem::PlanApproval(p), .. } if p.markdown == "# 计划"));
        assert_eq!(suggested_mode(&t.take_pending("p1").unwrap()), Some(PermissionMode::AcceptEdits));
        let (_, _) = t.control_request(&json!({"request_id":"r10","request":{"subtype":"can_use_tool","tool_name":"Bash","tool_use_id":"b1","input":{"command":"ls"}}}));
        let cancelled = t.cancel_request("r10");
        assert!(matches!(&cancelled[0], AgentEvent::PermissionResolved { id, option } if id == "b1" && option == "reject"));
        assert!(!t.has_pending());
    }

    #[test]
    fn api_错误只显示一份() {
        let mut t = Translator::new("/w");
        let synthetic = json!({"type":"assistant","isApiErrorMessage":true,"message":{"id":"m1","model":"<synthetic>","content":[{"type":"text","text":"API Error: 422 boom"}]}});
        assert!(t.handle(&synthetic).is_empty(), "合成的错误消息不应再作为助手回复显示");
        let ev = t.handle(&json!({"type":"result","subtype":"success","is_error":true,"result":"API Error: 422 boom"}));
        assert_eq!(ev.iter().filter(|e| matches!(e, AgentEvent::Notice { level: NoticeLevel::Error, .. })).count(), 1);
        // 合成消息之后的 result 没标错误:文字不能丢
        let mut t = Translator::new("/w");
        t.handle(&synthetic);
        let ev = t.handle(&json!({"type":"result","subtype":"success","is_error":false,"result":""}));
        assert!(ev.iter().any(|e| matches!(e, AgentEvent::Notice { text, .. } if text.contains("422"))));
        // 回放历史没有 result,合成消息照常显示
        let mut t = Translator::new("/w");
        t.replay = true;
        assert!(!t.handle(&synthetic).is_empty());
    }

    #[test]
    fn 中断后回合以取消结束() {
        let mut t = Translator::new("/w");
        t.interrupted = true;
        let ev = t.handle(&json!({"type":"result","subtype":"error_during_execution","is_error":true,"result":""}));
        assert!(matches!(ev.last(), Some(AgentEvent::TurnEnded(StopReason::Cancelled))));
        // 之后的错误才是真错误,并附带提示
        let ev = t.handle(&json!({"type":"result","subtype":"success","is_error":true,"result":"Invalid API key"}));
        assert!(matches!(&ev[1], AgentEvent::Notice { level: NoticeLevel::Error, text } if text == "Invalid API key"));
        assert!(matches!(ev.last(), Some(AgentEvent::TurnEnded(StopReason::Error))));
    }

    #[test]
    fn 未流式的完整消息直接追加() {
        let mut t = Translator::new("/w");
        let ev = t.handle(&json!({"type":"assistant","message":{"id":"x","content":[{"type":"text","text":"你好"},{"type":"thinking","thinking":""}]}}));
        assert_eq!(ev.len(), 1, "空思考块被忽略");
        assert!(matches!(&ev[0], AgentEvent::Append { item: TimelineItem::Assistant { text, streaming: false, .. }, .. } if text == "你好"));
    }

    #[test]
    fn bash_失败解析退出码() {
        let mut t = Translator::new("/w");
        t.handle(&json!({"type":"assistant","message":{"id":"m","content":[{"type":"tool_use","id":"b","name":"Bash","input":{"command":"false"}}]}}));
        let ev = t.handle(&json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"b","is_error":true,"content":"Exit code 2\nboom"}]}}));
        let AgentEvent::ToolUpdate { patch, .. } = &ev[0] else { panic!() };
        assert_eq!(patch.status, Some(ToolStatus::Failed));
        assert!(matches!(&patch.append_content[0], ToolContent::Terminal { exit_code: Some(2), .. }));
    }

    #[test]
    fn 不支持的控制请求立即回复() {
        let mut t = Translator::new("/w");
        let (_, reply) = t.control_request(&json!({"request_id":"z","request":{"subtype":"mystery"}}));
        assert_eq!(reply.unwrap()["response"]["subtype"], "error");
    }

    #[test]
    fn 路径与标题() {
        assert_eq!(short_path("/w/src/a.rs", "/w"), "src/a.rs");
        assert_eq!(short_path("/x/y/z/w/a.rs", "/w"), "…/z/w/a.rs");
        let t = build_tool("1", "mcp__github__get_pr", &json!({}), "/w");
        assert_eq!(t.title, "github · get_pr");
        assert_eq!(t.kind, ToolKind::Mcp);
    }

    fn run(t: &mut Translator, lines: &[Value]) -> SessionState {
        let mut events = Vec::new();
        for l in lines {
            events.extend(t.handle(l));
        }
        reduce(events)
    }

    #[test]
    fn mcp_输入请求自动拒绝并提示() {
        let mut t = Translator::new("/w");
        let (events, reply) = t.control_request(&json!({"type":"control_request","request_id":"r1","request":{"subtype":"elicitation","mcp_server_name":"github","message":"个人访问令牌"}}));
        assert!(reply.is_some(), "必须立刻应答,否则 claude 会一直等");
        assert!(matches!(&events[0], AgentEvent::Notice { text, .. } if text.contains("github") && text.contains("个人访问令牌")));
    }

    #[test]
    fn 后台子代理启动后不会被误标为已完成() {
        let mut t = Translator::new("/w");
        let s = run(
            &mut t,
            &[
                json!({"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"ag1","name":"Agent","input":{"subagent_type":"Explore","description":"盘点设置页","run_in_background":true}}]}}),
                json!({"type":"system","subtype":"background_tasks_changed","tasks":[{"task_id":"a9d5","task_type":"local_agent","description":"盘点设置页","tool_use_id":"ag1","is_backgrounded":true}]}),
                json!({"type":"system","subtype":"task_started","task_id":"a9d5","tool_use_id":"ag1","subagent_type":"Explore","is_backgrounded":true,"spawn_depth":1,"task_type":"local_agent","prompt":"盘点设置页"}),
                json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"ag1","content":"Async agent launched successfully.\nagentId: a9d5 (internal ID)"}]},"tool_use_result":{"isAsync":true,"status":"async_launched","agentId":"a9d5","outputFile":"/tmp/x/a9d5.output"}}),
                json!({"type":"system","subtype":"task_progress","task_id":"a9d5","usage":{"total_tokens":52470,"tool_uses":4,"duration_ms":4314},"last_tool_name":"Grep","summary":"正在搜索设置项"}),
            ],
        );
        let TimelineItem::Subagent(card) = &s.items[0] else { panic!("应是子 Agent 卡") };
        assert_eq!(card.status, SubagentStatus::Background, "启动结果不是完成");
        assert!(card.background);
        assert_eq!((card.tokens, card.tool_count, card.last_tool.as_str()), (52470, 4, "Grep"));
        assert_eq!(card.summary, "正在搜索设置项");
        assert_eq!(s.running_task_count(), 1);
        assert_eq!(s.task("a9d5").and_then(|t| t.output_file.clone()).as_deref(), Some("/tmp/x/a9d5.output"));
    }

    #[test]
    fn 后台子代理通知收尾并写入卡片摘要() {
        let mut t = Translator::new("/w");
        let s = run(
            &mut t,
            &[
                json!({"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"ag1","name":"Agent","input":{"subagent_type":"Explore","description":"盘点","run_in_background":true}}]}}),
                json!({"type":"system","subtype":"task_started","task_id":"a1","tool_use_id":"ag1","is_backgrounded":true,"task_type":"local_agent"}),
                json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"ag1","content":"Async agent launched"}]},"tool_use_result":{"isAsync":true,"agentId":"a1"}}),
                json!({"type":"system","subtype":"task_updated","task_id":"a1","patch":{"status":"completed","end_time":1}}),
                json!({"type":"system","subtype":"task_notification","task_id":"a1","tool_use_id":"ag1","status":"completed","output_file":"/tmp/a1.output","summary":"盘点完成,共 12 页"}),
                json!({"type":"system","subtype":"background_tasks_changed","tasks":[]}),
            ],
        );
        let TimelineItem::Subagent(card) = &s.items[0] else { panic!() };
        assert_eq!(card.status, SubagentStatus::Completed);
        assert_eq!(s.running_task_count(), 0);
        // 子 Agent 的结果写在自己的卡片上,不再额外插通知行
        assert_eq!(card.summary, "盘点完成,共 12 页");
        assert!(!s.items.iter().any(|i| matches!(i, TimelineItem::TaskNote { .. })));
    }

    #[test]
    fn 后台_bash_关联任务且不显示终端输出() {
        let mut t = Translator::new("/w");
        let s = run(
            &mut t,
            &[
                json!({"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"cargo build","description":"编译 release","run_in_background":true}}]}}),
                json!({"type":"system","subtype":"task_started","task_id":"bhmx","tool_use_id":"b1","description":"编译 release","is_backgrounded":true,"task_type":"local_bash"}),
                json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"b1","content":"Command running in background with ID: bhmx. Output is being written to: /private/tmp/claude/tasks/bhmx.output. You will be notified"}]},"tool_use_result":{"stdout":"","stderr":"","backgroundTaskId":"bhmx"}}),
            ],
        );
        let TimelineItem::Tool(tool) = &s.items[0] else { panic!() };
        assert_eq!(tool.task_id.as_deref(), Some("bhmx"));
        assert_eq!(tool.status, ToolStatus::Completed);
        assert!(tool.content.is_empty(), "启动结果不当作终端输出");
        let task = s.task("bhmx").unwrap();
        assert_eq!(task.kind, TaskKind::Shell);
        assert_eq!(task.output_file.as_deref(), Some("/private/tmp/claude/tasks/bhmx.output"));
        assert!(task.counts_as_running());
    }

    #[test]
    fn 子代理私有任务与环境任务不计入后台() {
        let mut t = Translator::new("/w");
        let s = run(
            &mut t,
            &[
                json!({"type":"system","subtype":"task_started","task_id":"f1","owned_by_subagent":true,"is_backgrounded":false,"task_type":"local_bash"}),
                json!({"type":"system","subtype":"task_started","task_id":"amb","ambient":true,"is_backgrounded":true,"task_type":"local_bash"}),
                json!({"type":"system","subtype":"task_started","task_id":"real","is_backgrounded":true,"task_type":"local_bash","description":"dev server"}),
            ],
        );
        assert_eq!(s.running_task_count(), 1);
        assert_eq!(s.running_tasks().next().unwrap().task_id, "real");
    }

    #[test]
    fn 前台子代理结束带上统计与摘要() {
        let mut t = Translator::new("/w");
        let s = run(
            &mut t,
            &[
                json!({"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"ag","name":"Agent","input":{"subagent_type":"Plan","description":"设计方案"}}]}}),
                json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"ag","content":[{"type":"text","text":"方案如下"}]}]},"tool_use_result":{"status":"completed","totalDurationMs":6039,"totalTokens":18149,"totalToolUseCount":7,"content":[{"type":"text","text":"方案如下:先拆模块"}]}}),
            ],
        );
        let TimelineItem::Subagent(card) = &s.items[0] else { panic!() };
        assert_eq!(card.status, SubagentStatus::Completed);
        assert_eq!((card.tokens, card.tool_count), (18149, 7));
        assert_eq!(card.summary, "方案如下:先拆模块");
        assert!(card.elapsed_ms(0).is_some_and(|e| e >= 6039));
    }

    #[test]
    fn 回放里的任务通知不再显示成用户消息() {
        let notice = "<task-notification>\n<task-id>bhmx</task-id>\n<tool-use-id>b1</tool-use-id>\n<output-file>/tmp/bhmx.output</output-file>\n<status>completed</status>\n<summary>Background command \"编译\" completed (exit code 0)</summary>\n</task-notification>";
        let mut t = Translator::replaying("/w");
        let s = run(
            &mut t,
            &[
                json!({"type":"user","message":{"role":"user","content":"帮我编译"}}),
                json!({"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"cargo build","description":"编译","run_in_background":true}}]}}),
                json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"b1","content":"Command running in background with ID: bhmx. Output is being written to: /tmp/bhmx.output. x"}]},"toolUseResult":{"stdout":"","stderr":"","backgroundTaskId":"bhmx"}}),
                json!({"type":"user","message":{"role":"user","content":notice},"origin":{"kind":"task-notification"}}),
            ],
        );
        let users = s.items.iter().filter(|i| matches!(i, TimelineItem::User { .. })).count();
        assert_eq!(users, 1, "通知不是用户输入");
        assert!(s.items.iter().any(|i| matches!(i, TimelineItem::TaskNote { status: TaskStatus::Completed, .. })));
        assert_eq!(s.running_task_count(), 0);
    }

    #[test]
    fn 取消回合只中断前台不影响后台任务() {
        let mut t = Translator::new("/w");
        t.interrupted = true;
        let mut events = Vec::new();
        for l in [
            json!({"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"fg","name":"Agent","input":{"description":"前台"}}]}}),
            json!({"type":"assistant","message":{"id":"m2","content":[{"type":"tool_use","id":"bg","name":"Agent","input":{"description":"后台","run_in_background":true}}]}}),
            json!({"type":"system","subtype":"task_started","task_id":"t-bg","tool_use_id":"bg","is_backgrounded":true,"task_type":"local_agent"}),
            json!({"type":"result","subtype":"error_during_execution","is_error":true,"result":""}),
        ] {
            events.extend(t.handle(&l));
        }
        let s = reduce(events);
        let status = |id: &str| match s.items.iter().find(|i| i.id() == id) {
            Some(TimelineItem::Subagent(c)) => c.status,
            _ => panic!(),
        };
        assert_eq!(status("fg"), SubagentStatus::Stopped);
        assert_eq!(status("bg"), SubagentStatus::Background);
    }

    #[test]
    fn 回放跳过中断标记但保留真实输入() {
        let mut t = Translator::replaying("/w");
        for marker in ["[Request interrupted by user]", "[Request interrupted by user for tool use]"] {
            let ev = t.handle(&json!({"type":"user","isSidechain":false,"message":{"role":"user","content":[{"type":"text","text":marker}]}}));
            assert!(ev.is_empty(), "{marker} 不应变成用户气泡");
        }
        let ev = t.handle(&json!({"type":"user","message":{"role":"user","content":[{"type":"text","text":"继续"}]}}));
        assert!(matches!(&ev[0], AgentEvent::Append { item: TimelineItem::User { text, .. }, .. } if text == "继续"));
    }

    #[test]
    fn 钩子失败与模型回退变成提示() {
        let mut t = Translator::new("/w");
        let ev = t.handle(&json!({"type":"system","subtype":"hook_response","hook_name":"Stop","outcome":"error","exit_code":2,"stderr":"lint 未通过"}));
        assert!(matches!(&ev[0], AgentEvent::Notice { level: NoticeLevel::Warning, text } if text.contains("Stop") && text.contains("lint 未通过") && text.contains("退出码 2")));
        assert!(t.handle(&json!({"type":"system","subtype":"hook_response","hook_name":"Stop","outcome":"success"})).is_empty());
        let ev = t.handle(&json!({"type":"system","subtype":"model_fallback","original_model":"opus","fallback_model":"sonnet","content":""}));
        assert!(matches!(&ev[0], AgentEvent::Notice { text, .. } if text.contains("sonnet")));
        let ev = t.handle(&json!({"type":"system","subtype":"status","status":null,"permissionMode":"plan"}));
        assert!(matches!(ev[0], AgentEvent::ModeChanged(PermissionMode::Plan)));
    }

    #[test]
    fn 监听工具按发起工具名识别() {
        let mut t = Translator::new("/w");
        let s = run(
            &mut t,
            &[
                json!({"type":"assistant","message":{"id":"m","content":[{"type":"tool_use","id":"mon","name":"Monitor","input":{"command":"tail -f log","description":"监听日志"}}]}}),
                json!({"type":"system","subtype":"task_started","task_id":"m1","tool_use_id":"mon","is_backgrounded":true,"task_type":"local_bash","description":"监听日志"}),
            ],
        );
        assert_eq!(s.task("m1").unwrap().kind, TaskKind::Monitor);
        assert!(matches!(&s.items[0], TimelineItem::Tool(t) if t.title == "监听 监听日志" && t.task_id.as_deref() == Some("m1")));
    }

    #[test]
    fn 初始化声明逐任务停止() {
        let v = crate::protocol::initialize("req_init");
        assert_eq!(v["request"]["perTaskStopAffordance"], true);
        let v = crate::protocol::stop_task("r", "t1");
        assert_eq!((v["request"]["subtype"].as_str(), v["request"]["task_id"].as_str()), (Some("stop_task"), Some("t1")));
    }

    #[test]
    fn 增量json里取出计划字段() {
        assert_eq!(partial_json_string("{\"plan\": \"# 标", "plan").as_deref(), Some("# 标"));
        assert_eq!(partial_json_string("{\"pla", "plan"), None);
        assert_eq!(partial_json_string("{\"plan\":\"a\\nb\\\"c\\\\d\",\"x\":1}", "plan").as_deref(), Some("a\nb\"c\\d"));
        // 半截转义留到下次
        assert_eq!(partial_json_string("{\"plan\":\"ab\\", "plan").as_deref(), Some("ab"));
        assert_eq!(partial_json_string("{\"plan\":\"ab\\u4e", "plan").as_deref(), Some("ab"));
        assert_eq!(partial_json_string("{\"plan\":\"ab\\u4e2d文\"}", "plan").as_deref(), Some("ab中文"));
        // 代理对(emoji)拼成一个字符;只到高位就等下次
        assert_eq!(partial_json_string("{\"plan\":\"\\ud83d\\ude00!", "plan").as_deref(), Some("😀!"));
        assert_eq!(partial_json_string("{\"plan\":\"x\\ud83d", "plan").as_deref(), Some("x"));
        // 值里恰好出现 "plan" 字样不会被当成键
        assert_eq!(partial_json_string("{\"note\":\"plan: x\",\"plan\":\"真\"}", "plan").as_deref(), Some("真"));
    }

    fn plan_stream(t: &mut Translator, ev: Value) -> Vec<AgentEvent> {
        t.handle(&json!({"type":"stream_event","event":ev,"api_message_id":"m1"}))
    }

    #[test]
    fn 计划卡随流式生成出现并在权限请求时定稿() {
        let mut t = Translator::new("/p");
        let started = plan_stream(&mut t, json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"p1","name":"ExitPlanMode","input":{}}}));
        assert!(matches!(&started[..], [AgentEvent::Append { item: TimelineItem::PlanApproval(p), .. }] if p.id == "p1" && p.streaming && p.approved.is_none() && p.markdown.is_empty()));
        let d1 = plan_stream(&mut t, json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"plan\":\"# 方"}}));
        assert!(matches!(&d1[..], [AgentEvent::PlanUpdated { id, markdown, streaming: true }] if id == "p1" && markdown == "# 方"));
        // 没有新增正文的片段不发事件
        assert!(plan_stream(&mut t, json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":""}})).is_empty());
        plan_stream(&mut t, json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"案\"}"}}));
        assert!(plan_stream(&mut t, json!({"type":"content_block_stop","index":1})).is_empty());
        // 权限请求到达:定稿同一张卡,不再追加第二张
        let (ev, _) = t.control_request(&json!({"request_id":"r1","request":{"subtype":"can_use_tool","tool_name":"ExitPlanMode","tool_use_id":"p1","input":{"plan":"# 方案"}}}));
        assert!(matches!(&ev[..], [AgentEvent::PlanUpdated { id, markdown, streaming: false }] if id == "p1" && markdown == "# 方案"));
    }

    #[test]
    fn init_带出_mcp_服务器状态与工具数() {
        let mut t = Translator::new("/p");
        let ev = t.handle(&json!({"type":"system","subtype":"init","session_id":"s1","cwd":"/p",
            "mcp_servers":[{"name":"github","status":"connected"},{"name":"db","status":"failed"},{"name":"x","status":"needs-auth"}],
            "tools":["Bash","mcp__github__get_pr","mcp__github__list","mcp__db__query"]}));
        let [AgentEvent::McpServers(l)] = &ev[..] else { panic!("{ev:?}") };
        assert_eq!(l.iter().map(|s| (s.name.as_str(), s.conn, s.tools.clone())).collect::<Vec<_>>(), vec![("github", McpConn::Connected, vec!["get_pr".to_string(), "list".to_string()]), ("db", McpConn::Failed, vec!["query".to_string()]), ("x", McpConn::NeedsAuth, vec![])]);
        // 旧版没有 mcp_servers 字段:不发事件
        assert!(t.handle(&json!({"type":"system","subtype":"init","session_id":"s1"})).is_empty());
    }

    #[test]
    fn mcp_status_应答带出状态_错误与工具() {
        let mut t = Translator::new("/p");
        let body = json!({"mcpServers":[
            {"name":"claude.ai Docs","status":"connected","tools":[{"name":"read","annotations":{}},{"name":"update"}]},
            {"name":"gw","status":"failed","error":"\n  [{\"code\":1}]\nmore","tools":[]},
            {"name":"off","status":"disabled"}]});
        let Some(AgentEvent::McpServers(l)) = t.mcp_status_event(&body, true) else { panic!() };
        assert_eq!((l[0].conn, l[0].tools.clone()), (McpConn::Connected, vec!["read".to_string(), "update".to_string()]));
        assert_eq!((l[1].conn, l[1].error.as_deref()), (McpConn::Failed, Some("[{\"code\":1}]")));
        assert_eq!((l[2].conn, l[2].tools.len()), (McpConn::Disabled, 0));
        assert!(t.mcp_status_event(&body, false).is_none());
    }

    #[test]
    fn 上下文明细带出各_mcp_工具占用() {
        let mut t = Translator::new("/p");
        let body = json!({"categories":[{"name":"MCP tools","tokens":300,"kind":"used"}],"maxTokens":1000,"totalTokens":300,
            "mcpTools":[{"name":"mcp__claude_ai_Docs__read","serverName":"claude_ai_Docs","tokens":120,"isLoaded":true},{"name":"mcp__gw__q","serverName":"gw","tokens":900,"isLoaded":false}]});
        let Some(AgentEvent::Usage(u)) = t.context_breakdown(&body, true) else { panic!() };
        assert_eq!(u.mcp_tools, vec![
            McpToolUsage { server: "claude_ai_Docs".into(), tool: "read".into(), tokens: 120, loaded: true },
            McpToolUsage { server: "gw".into(), tool: "q".into(), tokens: 900, loaded: false },
        ]);
    }

    #[test]
    fn 回合进行中每次调用模型都刷新上下文占用() {
        let mut t = Translator::new("/p");
        let start = |t: &mut Translator, input: u64, cache: u64| plan_stream(t, json!({"type":"message_start","message":{"id":"m1","usage":{"input_tokens":input,"cache_read_input_tokens":cache,"cache_creation_input_tokens":0,"output_tokens":1}}}));
        let ev = start(&mut t, 10, 5_000);
        assert!(matches!(&ev[..], [AgentEvent::Usage(u)] if u.context_used == 5_011));
        // 数没变不重复发;工具链里下一次调用上下文变大了就更新
        assert!(start(&mut t, 10, 5_000).is_empty());
        let ev = start(&mut t, 10, 9_000);
        assert!(matches!(&ev[..], [AgentEvent::Usage(u)] if u.context_used == 9_011));
        // 没有输入侧用量(只有输出)的 message_delta 不动它
        assert!(plan_stream(&mut t, json!({"type":"message_delta","usage":{"output_tokens":200}})).is_empty());
        // 带完整用量的 message_delta 把输出算进去
        let ev = plan_stream(&mut t, json!({"type":"message_delta","usage":{"input_tokens":10,"cache_read_input_tokens":9000,"output_tokens":200}}));
        assert!(matches!(&ev[..], [AgentEvent::Usage(u)] if u.context_used == 9_210));
    }

    #[test]
    fn 计划生成一半回合结束按未批准收尾() {
        let mut t = Translator::new("/p");
        plan_stream(&mut t, json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"p2","name":"ExitPlanMode","input":{}}}));
        let ev = t.handle(&json!({"type":"result","subtype":"error_during_execution","is_error":true,"errors":["boom"]}));
        assert!(ev.iter().any(|e| matches!(e, AgentEvent::PlanResolved { id, approved: false } if id == "p2")));
    }

    #[test]
    fn 回放计划按工具结果决定批准与否() {
        let lines = [
            json!({"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"a","name":"ExitPlanMode","input":{"plan":"# 一"}}]}}),
            json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"a","is_error":true,"content":"用户没有批准该计划"}]}}),
            // 同一个 tool_use 之后又落了一份空输入的快照:不显示
            json!({"type":"assistant","message":{"id":"m2","content":[{"type":"tool_use","id":"a","name":"ExitPlanMode","input":{}}]}}),
            json!({"type":"assistant","message":{"id":"m3","content":[{"type":"tool_use","id":"b","name":"ExitPlanMode","input":{"plan":"# 二"}}]}}),
            json!({"type":"assistant","message":{"id":"m4","content":[{"type":"tool_use","id":"c","name":"ExitPlanMode","input":{"plan":"# 三"}}]}}),
            json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"b","content":"User has approved your plan"}]}}),
        ];
        let text = lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n");
        let mut state = SessionState::default();
        for ev in crate::history::replay_lines(text.lines(), "/p") {
            state.apply(ev);
        }
        let plans: Vec<(&str, Option<bool>)> = state.items.iter().filter_map(|i| match i {
            TimelineItem::PlanApproval(p) => Some((p.markdown.as_str(), p.approved)),
            _ => None,
        }).collect();
        // 一:被拒绝;二:批准;三:没有结果(还没批准)
        assert_eq!(plans, vec![("# 一", Some(false)), ("# 二", Some(true)), ("# 三", None)]);
    }

}

/// 工具全名去掉 `mcp__<服务器>__` 前缀。
fn bare_tool<'a>(full: &'a str, server: &str) -> &'a str {
    full.strip_prefix(&format!("mcp__{}__", mcp_norm_name(server))).unwrap_or(full)
}

/// 从 `system init` 取各 MCP 服务器的状态与工具:`mcp_servers` 给状态,`tools` 里的 `mcp__<服务器>__<工具>` 归到各服务器。
/// 没有 `mcp_servers` 字段(旧版 claude)返回 `None`。
fn mcp_servers(init: &Value) -> Option<Vec<McpServerStatus>> {
    let servers = init.get("mcp_servers")?.as_array()?;
    let tools: Vec<&str> = init.get("tools").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    Some(
        servers
            .iter()
            .filter_map(|s| {
                let name = s.get("name").and_then(Value::as_str)?.to_string();
                let prefix = format!("mcp__{}__", mcp_norm_name(&name));
                let list: Vec<String> = tools.iter().filter_map(|t| t.strip_prefix(&prefix)).map(String::from).collect();
                Some(McpServerStatus { conn: McpConn::parse(str_of(s, "status")), name, tools: list, error: None })
            })
            .collect(),
    )
}

/// 取错误文字的第一行并截断(连接失败的原因有时是一大段 JSON)。
fn short_error(e: &str) -> Option<String> {
    let line = e.lines().map(str::trim).find(|l| !l.is_empty())?;
    Some(truncate(line, 120))
}

/// 一次 API 调用占用的上下文:输入 + 缓存读 + 缓存写 + 输出(都为 0 时返回 0,表示这份用量里没有输入侧数据)。
fn context_tokens(usage: &Value) -> u64 {
    let n = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
    if n("input_tokens") + n("cache_read_input_tokens") + n("cache_creation_input_tokens") == 0 {
        return 0;
    }
    n("input_tokens") + n("cache_read_input_tokens") + n("cache_creation_input_tokens") + n("output_tokens")
}

/// 当前 Unix 秒。
fn now_unix() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}
