//! 会话的实时状态摘要:侧栏状态点、后台任务数、IDE 对话标签的运行圆点只依赖这几个量。
//!
//! 为什么单独缓存:gpui 会记录渲染期间读过的实体,之后这些实体一 `notify` 就让整个窗口重绘。
//! 侧栏若在渲染里直接读每个已打开会话的 `SessionModel`,任何后台会话的流式输出、每秒计时
//! 都会让整个工作台重绘。改为由 `Workbench` 在 observe 回调里算出摘要并缓存,
//! 渲染只读缓存;非激活会话只有摘要变了才通知重绘。
//!
//! @author ixxxxoooo

use loom_core::{Phase, SessionState};
use loom_core::SessionBadge;

/// 一个已打开会话的实时状态摘要。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LiveSig {
    /// 回合进行中
    pub turn: bool,
    /// 有等待用户操作的条目(权限、提问、计划)
    pub awaiting: bool,
    /// 仍在运行的后台任务数
    pub tasks: usize,
}

impl LiveSig {
    /// 从会话状态算出摘要。
    ///
    /// 参数:`st` 会话状态。返回:摘要。
    pub fn of(st: &SessionState) -> Self {
        Self { turn: st.phase == Phase::Running, awaiting: st.awaiting_user(), tasks: st.running_task_count() }
    }

    /// 侧栏状态点:等待用户 > 运行中(回合结束但后台任务还在跑也算)> 未读 > 空闲。
    ///
    /// 参数:`unread` 是否未读。返回:状态点。
    pub fn badge(self, unread: bool) -> SessionBadge {
        if self.awaiting {
            SessionBadge::Waiting
        } else if self.turn || self.tasks > 0 {
            SessionBadge::Running
        } else if unread {
            SessionBadge::Unread
        } else {
            SessionBadge::Idle
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 状态点优先级() {
        let s = |turn, awaiting, tasks| LiveSig { turn, awaiting, tasks };
        assert_eq!(s(true, true, 0).badge(true), SessionBadge::Waiting);
        assert_eq!(s(true, false, 0).badge(true), SessionBadge::Running);
        assert_eq!(s(false, false, 2).badge(false), SessionBadge::Running);
        assert_eq!(s(false, false, 0).badge(true), SessionBadge::Unread);
        assert_eq!(s(false, false, 0).badge(false), SessionBadge::Idle);
    }

    #[::core::prelude::v1::test]
    fn 新会话摘要为空闲() {
        assert_eq!(LiveSig::of(&SessionState::default()), LiveSig::default());
    }
}
