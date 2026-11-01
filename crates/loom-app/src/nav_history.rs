//! 会话访问历史:顶部的前进 / 后退箭头用。纯逻辑,不依赖界面。
//!
//! @author ixxxxoooo

/// 访问过的会话 id 序列和当前位置。
#[derive(Debug, Default)]
pub struct NavHistory {
    entries: Vec<String>,
    /// 当前位置(指向 `entries` 里的下标);没有记录时为 0
    cursor: usize,
}

/// 历史最多保留多少条,超出丢弃最早的。
const MAX_ENTRIES: usize = 100;

impl NavHistory {
    /// 记录一次主动打开的会话:丢掉当前位置之后的「前进」记录;和当前位置相同则不重复记录。
    ///
    /// 参数:`id` 会话 id。
    pub fn visit(&mut self, id: &str) {
        if self.entries.get(self.cursor).is_some_and(|c| c == id) {
            return;
        }
        if !self.entries.is_empty() {
            self.entries.truncate(self.cursor + 1);
        }
        self.entries.push(id.to_string());
        if self.entries.len() > MAX_ENTRIES {
            self.entries.remove(0);
        }
        self.cursor = self.entries.len() - 1;
    }

    /// 能否后退。
    pub fn can_back(&self) -> bool {
        self.cursor > 0
    }

    /// 能否前进。
    pub fn can_forward(&self) -> bool {
        self.cursor + 1 < self.entries.len()
    }

    /// 后退一步,返回要打开的会话 id。
    pub fn back(&mut self) -> Option<String> {
        if !self.can_back() {
            return None;
        }
        self.cursor -= 1;
        self.entries.get(self.cursor).cloned()
    }

    /// 前进一步,返回要打开的会话 id。
    pub fn forward(&mut self) -> Option<String> {
        if !self.can_forward() {
            return None;
        }
        self.cursor += 1;
        self.entries.get(self.cursor).cloned()
    }

    /// 会话被删除后,把它从历史里去掉并保持当前位置指向同一条记录。
    ///
    /// 参数:`id` 被删除的会话 id。
    pub fn remove(&mut self, id: &str) {
        let before_cursor = self.entries.iter().take(self.cursor).filter(|e| *e == id).count();
        self.entries.retain(|e| e != id);
        self.cursor = self.cursor.saturating_sub(before_cursor).min(self.entries.len().saturating_sub(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 后退前进() {
        let mut h = NavHistory::default();
        h.visit("a");
        h.visit("b");
        h.visit("c");
        assert!(h.can_back() && !h.can_forward());
        assert_eq!(h.back().as_deref(), Some("b"));
        assert_eq!(h.back().as_deref(), Some("a"));
        assert_eq!(h.back(), None);
        assert_eq!(h.forward().as_deref(), Some("b"));
    }

    #[::core::prelude::v1::test]
    fn 后退后新打开会丢掉前进记录() {
        let mut h = NavHistory::default();
        for id in ["a", "b", "c"] {
            h.visit(id);
        }
        h.back();
        h.visit("d");
        assert!(!h.can_forward());
        assert_eq!(h.back().as_deref(), Some("b"));
    }

    #[::core::prelude::v1::test]
    fn 重复打开同一个不记录() {
        let mut h = NavHistory::default();
        h.visit("a");
        h.visit("a");
        assert!(!h.can_back());
    }

    #[::core::prelude::v1::test]
    fn 删除会话后位置不错乱() {
        let mut h = NavHistory::default();
        for id in ["a", "b", "c"] {
            h.visit(id);
        }
        h.remove("a");
        assert_eq!(h.back().as_deref(), Some("b"));
        h.remove("c");
        assert!(!h.can_forward());
    }
}
