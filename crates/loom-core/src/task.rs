//! 后台任务模型:后台 Bash、Monitor、后台子 Agent、workflow 等。
//!
//! 对应 claude stream-json 的 `system/task_*` 与 `background_tasks_changed` 消息。
//! 任务放在 `SessionState.tasks`,不进时间线;时间线上只有完成时的 `TimelineItem::TaskNote` 通知行。

use serde::{Deserialize, Serialize};

/// 任务类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TaskKind {
    /// 后台 Shell 命令(`run_in_background` 的 Bash)
    Shell,
    /// 后台子 Agent
    Agent,
    /// Monitor 持续监听
    Monitor,
    /// 动态工作流
    Workflow,
    #[default]
    Other,
}

impl TaskKind {
    /// 按 claude 的 `task_type` 字符串映射。`Monitor` 在协议里也是 `local_bash`,需要结合描述区分,这里只做基础映射。
    pub fn from_task_type(t: &str) -> Self {
        match t {
            "local_bash" | "bash" | "shell" => Self::Shell,
            "local_agent" | "agent" | "remote_agent" => Self::Agent,
            "monitor" => Self::Monitor,
            "local_workflow" | "workflow" => Self::Workflow,
            _ => Self::Other,
        }
    }

    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            Self::Shell => "后台命令",
            Self::Agent => "后台子代理",
            Self::Monitor => "监听",
            Self::Workflow => "工作流",
            Self::Other => "后台任务",
        }
    }
}

/// 任务状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TaskStatus {
    #[default]
    Running,
    Completed,
    Failed,
    /// 被用户或系统停止(killed / stopped)
    Stopped,
}

impl TaskStatus {
    /// 从协议字符串映射(`completed` / `failed` / `stopped` / `killed` / `cancelled` / `running`)。
    pub fn from_cli(s: &str) -> Self {
        match s {
            "completed" | "success" => Self::Completed,
            "failed" | "error" => Self::Failed,
            "stopped" | "killed" | "cancelled" => Self::Stopped,
            _ => Self::Running,
        }
    }

    /// 是否已结束。
    pub fn is_finished(self) -> bool {
        self != Self::Running
    }

    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "运行中",
            Self::Completed => "已完成",
            Self::Failed => "已失败",
            Self::Stopped => "已停止",
        }
    }
}

/// 一个后台任务。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct BackgroundTask {
    pub task_id: String,
    /// 同一个 `task_id` 重新运行(如恢复子 Agent)时,每次运行有新的 `run_id`
    pub run_id: Option<String>,
    /// 启动它的工具调用 id,用来关联时间线上的工具卡或子 Agent 卡
    pub tool_use_id: Option<String>,
    pub kind: TaskKind,
    pub description: String,
    pub status: TaskStatus,
    /// 开始时间(unix 毫秒)
    pub started_ms: u64,
    /// 结束时间(unix 毫秒)
    pub ended_ms: Option<u64>,
    pub tokens: u64,
    pub tool_uses: u32,
    /// 最近一次工具名
    pub last_tool: String,
    /// 最近一次有进展的时间(unix 毫秒),用来判断是否疑似停滞
    pub last_activity_ms: u64,
    /// 进度或结果摘要
    pub summary: String,
    /// 输出文件(后台 Shell 的实时输出落在这里)
    pub output_file: Option<String>,
    /// 最近输出的尾部(最多 `OUTPUT_CAP` 字节);真实模式下为空时由界面读取 `output_file`
    #[serde(default)]
    pub output: String,
    /// 为 true 表示不属于会话本身的工作(Claude Code 自己的内部任务、实时监听器),不计入活动指示
    pub ambient: bool,
    /// 为 true 表示是子 Agent 自己的前台工具,不算后台工作
    pub owned_by_subagent: bool,
    /// 是否在后台运行(前台子 Agent 也会有 `task_started`,但不是后台)
    pub backgrounded: bool,
}

impl BackgroundTask {
    /// 新建一个正在运行的后台任务。
    pub fn new(task_id: &str, kind: TaskKind, description: &str) -> Self {
        let now = now_ms();
        Self {
            task_id: task_id.into(),
            kind,
            description: description.into(),
            started_ms: now,
            last_activity_ms: now,
            backgrounded: true,
            ..Default::default()
        }
    }

    /// 链式设置启动它的工具调用 id。
    pub fn with_tool_use(mut self, id: &str) -> Self {
        self.tool_use_id = Some(id.into());
        self
    }

    /// 链式设置输出文件。
    pub fn with_output(mut self, path: &str) -> Self {
        self.output_file = Some(path.into());
        self
    }

    /// 是否计入「后台正在运行」:在跑、确实在后台,且不是环境任务或子 Agent 私有。
    pub fn counts_as_running(&self) -> bool {
        self.status == TaskStatus::Running && self.backgrounded && !self.ambient && !self.owned_by_subagent
    }

    /// 是否应该出现在任务列表里(环境任务与子 Agent 私有任务不展示)。
    pub fn visible(&self) -> bool {
        !self.ambient && !self.owned_by_subagent
    }

    /// 追加输出,超出上限时丢弃最早的部分(按字符边界截断)。
    pub fn push_output(&mut self, text: &str) {
        self.output.push_str(text);
        if self.output.len() > OUTPUT_CAP {
            let mut cut = self.output.len() - OUTPUT_CAP;
            while !self.output.is_char_boundary(cut) {
                cut += 1;
            }
            self.output.drain(..cut);
        }
        self.last_activity_ms = now_ms();
    }

    /// 已运行时长(毫秒):结束的用结束时间,运行中的用 `now`。
    pub fn elapsed_ms(&self, now: u64) -> u64 {
        self.ended_ms.unwrap_or(now).saturating_sub(self.started_ms)
    }

    /// 距上次活动已过去的毫秒数。
    pub fn idle_ms(&self, now: u64) -> u64 {
        now.saturating_sub(self.last_activity_ms)
    }
}

/// 任务输出缓冲上限(字节)。
pub const OUTPUT_CAP: usize = 16 * 1024;

/// 读日志文件时最多取末尾多少字节(比内存里的 `OUTPUT_CAP` 大得多,长日志才看得到近况)。
pub const LOG_CAP: usize = 96 * 1024;

/// 读到的日志尾部。
pub struct LogTail {
    /// 整理后的文本(已去掉回车覆盖的进度行,截断时从整行开始)
    pub text: String,
    /// 文件比读取上限大,前面的部分没显示
    pub truncated: bool,
    /// 文件总字节数
    pub total: u64,
}

/// 读取输出文件的尾部(最多 `cap` 字节);文件不存在或读失败返回 `None`。
///
/// 截断时丢掉不完整的第一行;每行只保留最后一个 `\r` 之后的内容(进度条反复覆盖同一行),`\r\n` 当普通换行。
pub fn read_log(path: &str, cap: usize) -> Option<LogTail> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let total = f.metadata().ok()?.len();
    let truncated = total > cap as u64;
    f.seek(SeekFrom::Start(total.saturating_sub(cap as u64))).ok()?;
    let mut buf = Vec::new();
    f.take(cap as u64).read_to_end(&mut buf).ok()?;
    let mut text = String::from_utf8_lossy(&buf).into_owned();
    if truncated {
        if let Some(i) = text.find('\n') {
            text.drain(..=i);
        }
    }
    Some(LogTail { text: tidy_log(&text), truncated, total })
}

/// 整理终端输出:`\r\n` 归一,行内的回车覆盖只留最后一段。
pub fn tidy_log(text: &str) -> String {
    let text = text.replace("\r\n", "\n");
    if !text.contains('\r') {
        return text;
    }
    text.split('\n').map(|l| l.rsplit('\r').find(|x| !x.is_empty()).unwrap_or("")).collect::<Vec<_>>().join("\n")
}

/// 疑似停滞的阈值:运行中且超过这么久没有任何进展(毫秒)。
pub const STALL_MS: u64 = 60_000;

/// 当前 unix 毫秒。
pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// 把毫秒格式化成「12 秒」「3 分 05 秒」「1 小时 02 分」。
pub fn format_elapsed(ms: u64) -> String {
    let s = ms / 1000;
    if s < 60 {
        format!("{s} 秒")
    } else if s < 3600 {
        format!("{} 分 {:02} 秒", s / 60, s % 60)
    } else {
        format!("{} 小时 {:02} 分", s / 3600, s % 3600 / 60)
    }
}

/// 把 token 数格式化成「820」「8.2k」「1.3M」。
pub fn format_tokens(n: u64) -> String {
    if n < 1000 {
        n.to_string()
    } else if n < 1_000_000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 计数规则排除环境任务与子代理私有任务() {
        let mut t = BackgroundTask::new("a", TaskKind::Shell, "x");
        assert!(t.counts_as_running());
        t.ambient = true;
        assert!(!t.counts_as_running());
        t.ambient = false;
        t.owned_by_subagent = true;
        assert!(!t.counts_as_running());
        t.owned_by_subagent = false;
        t.backgrounded = false;
        assert!(!t.counts_as_running());
    }

    #[test]
    fn 时长与数量格式化() {
        assert_eq!(format_elapsed(12_000), "12 秒");
        assert_eq!(format_elapsed(185_000), "3 分 05 秒");
        assert_eq!(format_elapsed(3_720_000), "1 小时 02 分");
        assert_eq!(format_tokens(820), "820");
        assert_eq!(format_tokens(8200), "8.2k");
    }

    #[test]
    fn 状态映射() {
        assert_eq!(TaskStatus::from_cli("killed"), TaskStatus::Stopped);
        assert_eq!(TaskStatus::from_cli("completed"), TaskStatus::Completed);
        assert_eq!(TaskKind::from_task_type("local_agent"), TaskKind::Agent);
    }
}
