//! 自动化:按计划在项目里无人值守地运行一段提示词(底层是 `claude -p`)。
//!
//! - `schedule`:计划与下次运行时间的纯函数;
//! - `store`:自动化列表与运行记录的落盘;
//! - `runner`:阻塞式地执行一次并产出运行记录。

mod runner;
mod schedule;
mod store;

pub use runner::{DEFAULT_TIMEOUT, RunOptions, run_once};
pub use schedule::*;
pub use store::AutomationStore;

use serde::{Deserialize, Serialize};

/// 一条自动化。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Automation {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub project_path: String,
    pub schedule: Schedule,
    pub enabled: bool,
    /// 权限模式的 CLI 取值;无人值守时不能询问,默认 `acceptEdits`
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// 创建时间(Unix 秒)
    pub created_at: i64,
    /// 最近一次开始运行的时间(Unix 秒)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<i64>,
}

fn default_mode() -> String {
    "acceptEdits".into()
}

impl Automation {
    /// 下次运行时间(Unix 秒,本地时区)。已停用返回 `None`。
    pub fn next_run_ts(&self) -> Option<i64> {
        if !self.enabled {
            return None;
        }
        let base = self.last_run.unwrap_or(self.created_at);
        Some(to_ts(self.schedule.next_after(to_naive(base))))
    }

    /// 在 `now`(Unix 秒)是否到期。
    pub fn is_due(&self, now: i64) -> bool {
        self.next_run_ts().is_some_and(|t| t <= now)
    }
}

/// 一次运行的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Succeeded,
    Failed,
}

/// 运行记录。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunRecord {
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub status: RunStatus,
    /// 助手最终回复(截断)或错误说明
    pub summary: String,
    #[serde(default)]
    pub cost_usd: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// 错误。
#[derive(Debug, thiserror::Error)]
pub enum AutomationError {
    #[error("读写失败:{0}")]
    Io(String),
    #[error("格式错误:{0}")]
    Format(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn sample(id: &str) -> Automation {
        Automation {
            id: id.into(),
            name: "每日巡检".into(),
            prompt: "检查今天新增的 TODO".into(),
            project_path: "/tmp".into(),
            schedule: Schedule::Daily { hour: 9, minute: 0 },
            enabled: true,
            mode: "acceptEdits".into(),
            model: None,
            created_at: 0,
            last_run: None,
        }
    }

    #[test]
    fn 停用的自动化永不到期() {
        let mut a = sample("a");
        a.enabled = false;
        assert_eq!(a.next_run_ts(), None);
        assert!(!a.is_due(i64::MAX));
    }

    #[test]
    fn 启用后到期判断() {
        let mut a = sample("a");
        a.created_at = to_ts(naive(2026, 10, 22, 10, 0));
        let next = a.next_run_ts().unwrap();
        assert_eq!(next, to_ts(naive(2026, 10, 23, 9, 0)), "已过今天 9 点,下次是明天");
        assert!(!a.is_due(next - 1));
        assert!(a.is_due(next));
        a.last_run = Some(next);
        assert_eq!(a.next_run_ts().unwrap(), to_ts(naive(2026, 10, 24, 9, 0)));
    }
}
