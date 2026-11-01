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
    usage: Usage,
    /// 用户是否请求过中断(用于把回合结束原因标成「已取消」)
    pub interrupted: bool,
    seq: u64,
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
            usage: Usage::default(),
            interrupted: false,
            seq: 0,
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
        let live = if self.replay { "" } else { "l" };
        format!("{live}{prefix}{}", self.seq)
    }

    /// 用户发出的提示:claude 不会回显,由界面侧自己追加到时间线。
    pub fn user_prompt(&mut self, text: &str, images: &[String]) -> AgentEvent {
        let id = self.next_id("u");
        AgentEvent::Append { parent: None, item: TimelineItem::User { id, text: text.into(), images: images.to_vec(), files: Vec::new() } }
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
            _ => Vec::new(),
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
                Vec::new()
            }
            "compact_boundary" => vec![notice(NoticeLevel::Info, "已压缩上下文")],
            "status" if str_of(v, "status") == "compacting" => {
                vec![notice(NoticeLevel::Info, "正在压缩上下文…")]
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
            _ => Vec::new(),
        }
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
                Vec::new()
            }
            "content_block_start" => {
                if !matches!(str_of(&ev["content_block"], "type"), "text" | "thinking") {
                    return Vec::new();
                }
                let id = format!("{msg_id}:{index}");
                self.open_blocks.insert((msg_id, index), OpenBlock { id, parent, created: false });
                Vec::new()
            }
            "content_block_delta" => {
                let delta = &ev["delta"];
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
                    let markdown = str_of(&input, "plan").to_string();
                    vec![AgentEvent::Append {
                        parent,
                        item: TimelineItem::PlanApproval(PlanApproval { id, markdown, approved: Some(true) }),
                    }]
                } else {
                    Vec::new()
                }
            }
            "Task" | "Agent" => {
                self.subagents.insert(id.clone());
                let agent_type = input.get("subagent_type").and_then(Value::as_str).unwrap_or("general-purpose");
                let card = SubagentCard {
                    id,
                    agent_type: agent_type.into(),
                    description: str_of(&input, "description").into(),
                    status: SubagentStatus::Running,
                    children: Vec::new(),
                };
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
            for block in blocks {
                match str_of(block, "type") {
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
            if self.replay && !texts.is_empty() {
                out.extend(self.replay_user(&texts.join("\n"), v));
            }
        } else if let Some(text) = content.as_str()
            && self.replay
        {
            out.extend(self.replay_user(text, v));
        }
        out
    }

    /// 回放时把真实用户输入还原成条目;过滤掉元信息与斜杠命令产生的内部文本。
    fn replay_user(&mut self, text: &str, v: &Value) -> Vec<AgentEvent> {
        let internal = v.get("isMeta").and_then(Value::as_bool).unwrap_or(false)
            || v.get("isSidechain").and_then(Value::as_bool).unwrap_or(false)
            || text.starts_with("<command-")
            || text.starts_with("<local-command")
            || text.starts_with("Caveat:")
            || text.starts_with("<system-reminder>")
            || text.trim().is_empty();
        if internal {
            return Vec::new();
        }
        let id = self.next_id("u");
        vec![AgentEvent::Append {
            parent: None,
            item: TimelineItem::User { id, text: text.into(), images: Vec::new(), files: Vec::new() },
        }]
    }

    fn tool_result(&mut self, block: &Value, tur: Option<&Value>, parent: Option<String>) -> Vec<AgentEvent> {
        let id = str_of(block, "tool_use_id").to_string();
        let is_error = block.get("is_error").and_then(Value::as_bool).unwrap_or(false);
        let info = self.tools.remove(&id);

        if self.subagents.contains(&id) {
            let status = if is_error { SubagentStatus::Failed } else { SubagentStatus::Completed };
            return vec![AgentEvent::SubagentStatus { id, status }];
        }
        let Some(info) = info else {
            return Vec::new();
        };
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
        for _ in 0..images {
            patch.append_content.push(ToolContent::Image { label: "图片".into(), path: None });
        }
        vec![AgentEvent::ToolUpdate { parent: parent.or(info.parent), id, patch }]
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
        out.push(AgentEvent::Usage(self.usage.clone()));

        let is_error = v.get("is_error").and_then(Value::as_bool).unwrap_or(false);
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
        out.push(AgentEvent::TurnEnded(reason));
        out
    }

    fn rate_limit(&mut self, v: &Value) -> Vec<AgentEvent> {
        let info = &v["rate_limit_info"];
        let windows = &info["unifiedWindows"];
        let pct = |k: &str| windows[k].get("utilization").and_then(Value::as_f64).map(|u| (u * 100.0) as f32);
        if let Some(p) = pct("five_hour") {
            self.usage.five_hour_pct = Some(p);
        }
        if let Some(p) = pct("seven_day") {
            self.usage.weekly_pct = Some(p);
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
            "elicitation" => (Vec::new(), Some(crate::protocol::success_response(&request_id, serde_json::json!({"action": "decline"})))),
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
            PendingKind::Plan => AgentEvent::Append {
                parent,
                item: TimelineItem::PlanApproval(PlanApproval {
                    id: item_id.clone(),
                    markdown: str_of(&input, "plan").into(),
                    approved: None,
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

/// 工具结果内容 → (文本, 图片数)。
fn result_text(content: &Value) -> (String, usize) {
    match content {
        Value::String(s) => (s.clone(), 0),
        Value::Array(blocks) => {
            let mut texts = Vec::new();
            let mut images = 0;
            for b in blocks {
                match str_of(b, "type") {
                    "text" => texts.push(str_of(b, "text").to_string()),
                    "image" => images += 1,
                    _ => {}
                }
            }
            (texts.join("\n"), images)
        }
        _ => (String::new(), 0),
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
        "BashOutput" => "读取后台输出".into(),
        "KillShell" | "KillBash" => "结束后台任务".into(),
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
    use super::*;
    use serde_json::json;

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
        assert!(matches!(&s.items[1], TimelineItem::Todos { items, .. } if items.len() == 2 && items[0].status == TodoStatus::InProgress));
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
}
