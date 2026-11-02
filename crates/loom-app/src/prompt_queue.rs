//! 运行中排队的消息(对标 Cursor 的「排队 / 立即引导」):纯数据与策略,与界面无关。
//!
//! - 默认:回合进行中发新消息 = 排队,本轮正常结束后按顺序自动发送;
//! - 「引导」:把某条排队消息立刻以 `priority: next` 塞进当前回合(`take` 取出后由调用方发送);
//! - 「停止并发送」:把某条挪到队首并中断当前回合,中断结束后自动发它;
//! - 回合被中断 / 出错时队列**暂停**(不再自动发送),避免用户中止后又被排队消息接着跑,
//!   由用户点「继续」或逐条处理;回合正常结束则恢复自动发送。
//!
//! @author ixxxxoooo

use loom_core::StopReason;

/// 一条排队的消息(带图片与文件引用)。
#[derive(Debug, Clone, PartialEq)]
pub struct QueuedPrompt {
    /// 队内唯一编号(界面按它定位按钮)
    pub id: u64,
    pub text: String,
    pub images: Vec<String>,
    pub files: Vec<String>,
}

/// 回合结束后的处理结果。
#[derive(Debug, Clone, PartialEq)]
pub enum Next {
    /// 队列为空或已暂停:什么都不发
    Idle,
    /// 该发送这一条
    Send(QueuedPrompt),
}

/// 排队队列与暂停状态。
#[derive(Debug, Default)]
pub struct PromptQueue {
    items: Vec<QueuedPrompt>,
    next_id: u64,
    /// 暂停自动发送(上一轮被中断或出错)
    paused: bool,
    /// 「停止并发送」进行中:下一次中断结束后仍要发队首
    resume_on_cancel: bool,
}

impl PromptQueue {
    /// 排到队尾,返回编号。
    pub fn push(&mut self, text: String, images: Vec<String>, files: Vec<String>) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.items.push(QueuedPrompt { id, text, images, files });
        id
    }

    pub fn items(&self) -> &[QueuedPrompt] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    /// 取走指定的一条(引导 / 删除 / 编辑用)。队列空了就清掉暂停标记。
    pub fn take(&mut self, id: u64) -> Option<QueuedPrompt> {
        let ix = self.items.iter().position(|q| q.id == id)?;
        let item = self.items.remove(ix);
        self.settle();
        Some(item)
    }

    /// 取走全部(撤回用),按排队顺序。
    pub fn take_all(&mut self) -> Vec<QueuedPrompt> {
        self.paused = false;
        self.resume_on_cancel = false;
        std::mem::take(&mut self.items)
    }

    /// 把某条挪到队首并登记「中断后继续发」,返回是否找到。
    pub fn promote_for_stop(&mut self, id: u64) -> bool {
        let Some(ix) = self.items.iter().position(|q| q.id == id) else { return false };
        let item = self.items.remove(ix);
        self.items.insert(0, item);
        self.resume_on_cancel = true;
        true
    }

    /// 用户点「继续」:解除暂停并取队首(没有则 `None`)。
    pub fn resume(&mut self) -> Option<QueuedPrompt> {
        self.paused = false;
        if self.items.is_empty() { None } else { Some(self.items.remove(0)) }
    }

    /// 回合结束:决定下一步。正常结束 → 发队首并取消暂停;中断 / 出错 → 暂停(「停止并发送」除外)。
    pub fn on_turn_end(&mut self, reason: &StopReason) -> Next {
        if self.items.is_empty() {
            self.paused = false;
            self.resume_on_cancel = false;
            return Next::Idle;
        }
        let interrupted = matches!(reason, StopReason::Cancelled | StopReason::Error);
        if interrupted && !self.resume_on_cancel {
            self.paused = true;
            return Next::Idle;
        }
        self.resume_on_cancel = false;
        self.paused = false;
        Next::Send(self.items.remove(0))
    }

    fn settle(&mut self) {
        if self.items.is_empty() {
            self.paused = false;
            self.resume_on_cancel = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(texts: &[&str]) -> PromptQueue {
        let mut q = PromptQueue::default();
        for t in texts {
            q.push((*t).into(), vec![], vec![]);
        }
        q
    }

    #[test]
    fn 正常结束按顺序逐条发送() {
        let mut q = q(&["a", "b"]);
        assert!(matches!(q.on_turn_end(&StopReason::EndTurn), Next::Send(p) if p.text == "a"));
        assert!(matches!(q.on_turn_end(&StopReason::EndTurn), Next::Send(p) if p.text == "b"));
        assert_eq!(q.on_turn_end(&StopReason::EndTurn), Next::Idle);
    }

    #[test]
    fn 中断或出错后暂停不自动发送() {
        let mut q = q(&["a"]);
        assert_eq!(q.on_turn_end(&StopReason::Cancelled), Next::Idle);
        assert!(q.paused());
        assert_eq!(q.len(), 1);
        let mut e = self::q(&["a"]);
        assert_eq!(e.on_turn_end(&StopReason::Error), Next::Idle);
        assert!(e.paused());
    }

    #[test]
    fn 暂停后可继续_且正常结束会恢复自动发送() {
        let mut q = q(&["a", "b"]);
        q.on_turn_end(&StopReason::Cancelled);
        assert_eq!(q.resume().map(|p| p.text), Some("a".into()));
        assert!(!q.paused());
        assert!(matches!(q.on_turn_end(&StopReason::EndTurn), Next::Send(p) if p.text == "b"));
    }

    #[test]
    fn 停止并发送_中断后仍发队首() {
        let mut q = q(&["a", "b", "c"]);
        let id = q.items()[2].id;
        assert!(q.promote_for_stop(id));
        assert!(matches!(q.on_turn_end(&StopReason::Cancelled), Next::Send(p) if p.text == "c"));
        // 标记只管一次:之后再被中断就暂停
        assert_eq!(q.on_turn_end(&StopReason::Cancelled), Next::Idle);
        assert!(q.paused());
    }

    #[test]
    fn 取走指定条目与清空() {
        let mut q = q(&["a", "b"]);
        let id = q.items()[0].id;
        assert_eq!(q.take(id).map(|p| p.text), Some("a".into()));
        assert_eq!(q.take(id), None);
        q.on_turn_end(&StopReason::Cancelled);
        assert!(q.paused());
        let rest = q.take_all();
        assert_eq!(rest.len(), 1);
        assert!(!q.paused() && q.is_empty());
    }

    #[test]
    fn 取光后暂停标记随之清除() {
        let mut q = q(&["a"]);
        q.on_turn_end(&StopReason::Error);
        let id = q.items()[0].id;
        q.take(id);
        assert!(!q.paused());
    }
}
