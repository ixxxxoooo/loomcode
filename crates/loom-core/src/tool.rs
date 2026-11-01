//! 工具调用模型。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 工具的大类,决定图标与卡片样式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolKind {
    Read,
    Edit,
    Write,
    Delete,
    Move,
    Search,
    Execute,
    Think,
    Fetch,
    Browser,
    Mcp,
    /// 子 Agent(Task / Agent 工具)
    Subagent,
    Todo,
    SwitchMode,
    Other,
}

impl ToolKind {
    /// 按 Claude Code 的工具名推断大类。
    pub fn from_tool_name(name: &str) -> Self {
        if name.starts_with("mcp__") {
            return if name.contains("browser") || name.contains("playwright") {
                Self::Browser
            } else {
                Self::Mcp
            };
        }
        match name {
            "Read" | "NotebookRead" => Self::Read,
            "Edit" | "MultiEdit" | "NotebookEdit" => Self::Edit,
            "Write" => Self::Write,
            "Glob" | "Grep" | "LS" | "ToolSearch" | "LSP" => Self::Search,
            "Bash" | "BashOutput" | "KillShell" | "KillBash" | "Await" | "Monitor" | "TaskOutput" | "TaskStop" => Self::Execute,
            "WebFetch" | "WebSearch" => Self::Fetch,
            "Task" | "Agent" => Self::Subagent,
            "TodoWrite" => Self::Todo,
            "ExitPlanMode" | "EnterPlanMode" => Self::SwitchMode,
            "Think" => Self::Think,
            _ => Self::Other,
        }
    }
}

/// 工具执行状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolStatus {
    Pending,
    Running,
    Completed,
    Failed,
    /// 用户拒绝了权限请求
    Denied,
    /// 回合被中断时仍在运行的工具
    Interrupted,
}

impl ToolStatus {
    /// 是否已结束(成功、失败、被拒绝或被中断)。
    pub fn is_finished(self) -> bool {
        !matches!(self, Self::Pending | Self::Running)
    }

    /// 是否属于需要提示用户注意的未成功结果(失败 / 被拒绝 / 被中断)。
    pub fn is_unsuccessful(self) -> bool {
        matches!(self, Self::Failed | Self::Denied | Self::Interrupted)
    }
}

/// 工具产出的内容块。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ToolContent {
    /// 纯文本或带 ANSI 转义的输出
    Text(String),
    /// 文件改动
    Diff { path: String, old: String, new: String },
    /// 终端命令与输出
    Terminal { command: String, output: String, exit_code: Option<i32> },
    /// 图片(演示中用占位说明,真实模式存文件路径)
    Image { label: String, path: Option<String> },
}

/// 一次工具调用。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    /// Claude Code 原始工具名,如 `Bash`、`mcp__github__get_pr`
    pub name: String,
    pub kind: ToolKind,
    /// 一行摘要,如 `读取 src/main.rs`
    pub title: String,
    pub status: ToolStatus,
    /// 原始入参
    pub input: Value,
    pub content: Vec<ToolContent>,
    /// 涉及的文件路径(用于「跟随」与跳转)
    pub locations: Vec<String>,
    /// 耗时毫秒(结束后填充)
    pub duration_ms: Option<u64>,
    /// 后台任务 id:后台 Bash 或 Monitor 启动后,用它关联 `SessionState.tasks`
    #[serde(default)]
    pub task_id: Option<String>,
}

impl ToolCall {
    /// 创建一个运行中的工具调用,大类由工具名推断。
    pub fn new(id: &str, name: &str, title: &str) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind: ToolKind::from_tool_name(name),
            title: title.into(),
            status: ToolStatus::Running,
            input: Value::Null,
            content: Vec::new(),
            locations: Vec::new(),
            duration_ms: None,
            task_id: None,
        }
    }

    /// 链式设置入参。
    pub fn with_input(mut self, input: Value) -> Self {
        self.input = input;
        self
    }

    /// 链式追加内容块。
    pub fn with_content(mut self, content: ToolContent) -> Self {
        self.content.push(content);
        self
    }

    /// 链式追加涉及的文件。
    pub fn with_location(mut self, path: &str) -> Self {
        self.locations.push(path.into());
        self
    }
}

/// 工具调用的增量更新,字段为 `None` 表示不变。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolPatch {
    pub title: Option<String>,
    pub status: Option<ToolStatus>,
    pub append_content: Vec<ToolContent>,
    pub duration_ms: Option<u64>,
    pub task_id: Option<String>,
}

impl ToolCall {
    /// 应用增量更新。
    pub fn apply(&mut self, patch: ToolPatch) {
        if let Some(t) = patch.title {
            self.title = t;
        }
        if let Some(s) = patch.status {
            self.status = s;
        }
        self.content.extend(patch.append_content);
        if patch.duration_ms.is_some() {
            self.duration_ms = patch.duration_ms;
        }
        if patch.task_id.is_some() {
            self.task_id = patch.task_id;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 工具名推断大类() {
        assert_eq!(ToolKind::from_tool_name("Bash"), ToolKind::Execute);
        assert_eq!(ToolKind::from_tool_name("mcp__github__list"), ToolKind::Mcp);
        assert_eq!(ToolKind::from_tool_name("mcp__playwright__click"), ToolKind::Browser);
        assert_eq!(ToolKind::from_tool_name("Agent"), ToolKind::Subagent);
    }

    #[test]
    fn 增量更新生效() {
        let mut t = ToolCall::new("1", "Bash", "运行测试");
        t.apply(ToolPatch {
            status: Some(ToolStatus::Completed),
            append_content: vec![ToolContent::Text("ok".into())],
            duration_ms: Some(12),
            ..Default::default()
        });
        assert!(t.status.is_finished());
        assert_eq!(t.content.len(), 1);
        assert_eq!(t.duration_ms, Some(12));
    }
}
