//! claude 的忙闲信号:`system session_state_changed`(需要 `CLAUDE_CODE_EMIT_SESSION_STATE_EVENTS=1`)
//! 与 `command_lifecycle`(每条用户消息的 queued / started / completed / cancelled)。
//!
//! 已对真实 claude 2.1.x 实测:
//! - `session_state_changed` 只在忙闲切换时发出;一次提示是 `running` → … → `result` → `idle`;
//! - 中断时还没被吸收的引导消息不会被取消,claude 会紧接着另开回合处理它,期间一直是 `running`;
//! - 回合结束仍以 `result` 为主信号,这里的 `idle` 只是兜底(没收到 `result` 就空闲了),
//!   `running` 则让 claude 自行开启的回合(后台任务通知等)比第一条流式输出更早进入运行态。
//!
//! @author ixxxxoooo

use serde_json::Value;
use std::collections::HashSet;

/// 一行输出对回合状态的影响。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// 与回合状态无关
    None,
    /// claude 开始忙了
    Busy,
    /// claude 真正空闲了(没有本端发出、尚未结束的消息)
    Idle,
}

/// 忙闲跟踪器。
#[derive(Debug, Default)]
pub struct Liveness {
    /// 本端写入 stdin、claude 还没报告 completed / cancelled 的消息 uuid
    inflight: HashSet<String>,
}

impl Liveness {
    /// 记录一条刚写入的用户消息。
    ///
    /// 参数 `uuid`:消息的 `uuid` 字段;为空时忽略。
    pub fn sent(&mut self, uuid: &str) {
        if !uuid.is_empty() {
            self.inflight.insert(uuid.to_string());
        }
    }

    /// 处理一行输出。
    ///
    /// 参数 `v`:claude 输出的一条 JSON 消息。
    /// 返回:这条消息对回合状态的影响。`idle` 只有在没有未结束的本端消息时才算数,
    /// 以免上一回合迟到的 `idle` 把刚发出的新消息的回合误判为结束。
    pub fn on_message(&mut self, v: &Value) -> Signal {
        let str_of = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default();
        match str_of("type") {
            "command_lifecycle" => {
                if matches!(str_of("state"), "completed" | "cancelled") {
                    self.inflight.remove(str_of("command_uuid"));
                }
                Signal::None
            }
            "system" if str_of("subtype") == "session_state_changed" => match str_of("state") {
                "idle" if self.inflight.is_empty() => Signal::Idle,
                "idle" => Signal::None,
                _ => Signal::Busy,
            },
            _ => Signal::None,
        }
    }

    /// 未结束的本端消息数(诊断用)。
    pub fn pending(&self) -> usize {
        self.inflight.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn state(s: &str) -> Value {
        json!({"type":"system","subtype":"session_state_changed","state":s})
    }

    fn life(id: &str, s: &str) -> Value {
        json!({"type":"command_lifecycle","command_uuid":id,"state":s})
    }

    #[test]
    fn 正常一轮_运行到空闲() {
        let mut l = Liveness::default();
        l.sent("u1");
        assert_eq!(l.on_message(&state("running")), Signal::Busy);
        assert_eq!(l.on_message(&life("u1", "started")), Signal::None);
        assert_eq!(l.on_message(&life("u1", "completed")), Signal::None);
        assert_eq!(l.on_message(&state("idle")), Signal::Idle);
        assert_eq!(l.pending(), 0);
    }

    #[test]
    fn 迟到的空闲不结束新消息的回合() {
        let mut l = Liveness::default();
        l.sent("u1");
        l.on_message(&life("u1", "completed"));
        // 上一回合的 result 之后界面立刻发出了排队消息,随后才收到上一回合的 idle
        l.sent("u2");
        assert_eq!(l.on_message(&state("idle")), Signal::None);
        l.on_message(&life("u2", "cancelled"));
        assert_eq!(l.on_message(&state("idle")), Signal::Idle);
    }

    #[test]
    fn 无关消息与空_uuid() {
        let mut l = Liveness::default();
        l.sent("");
        assert_eq!(l.pending(), 0);
        assert_eq!(l.on_message(&json!({"type":"assistant"})), Signal::None);
        assert_eq!(l.on_message(&json!({"type":"system","subtype":"init"})), Signal::None);
    }
}
