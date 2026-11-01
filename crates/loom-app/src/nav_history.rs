//! 访问历史:顶部的前进 / 后退箭头用。纯逻辑,不依赖界面。
//! 泛型:Agent 窗口记会话 id(`NavHistory<String>`),IDE 窗口记 (文件, 行)。
//!
//! @author ixxxxoooo

/// 访问过的条目序列和当前位置。
#[derive(Debug)]
pub struct NavHistory<T> {
    entries: Vec<T>,
    /// 当前位置(指向 `entries` 里的下标);没有记录时为 0
    cursor: usize,
}

/// 历史最多保留多少条,超出丢弃最早的。
const MAX_ENTRIES: usize = 100;

impl<T> Default for NavHistory<T> {
    fn default() -> Self {
        Self { entries: Vec::new(), cursor: 0 }
    }
}

impl<T: Clone + PartialEq> NavHistory<T> {
    /// 记录一次主动打开的条目:丢掉当前位置之后的「前进」记录;和当前位置相同则不重复记录。
    ///
    /// 参数:`entry` 条目。
    pub fn visit(&mut self, entry: T) {
        if self.entries.get(self.cursor).is_some_and(|c| *c == entry) {
            return;
        }
        if !self.entries.is_empty() {
            self.entries.truncate(self.cursor + 1);
        }
        self.entries.push(entry);
        if self.entries.len() > MAX_ENTRIES {
            self.entries.remove(0);
        }
        self.cursor = self.entries.len() - 1;
    }

    /// 把当前位置的条目换成新的(没有记录时等同 `visit`)。
    pub fn replace_current(&mut self, entry: T) {
        match self.entries.get_mut(self.cursor) {
            Some(e) => *e = entry,
            None => self.visit(entry),
        }
    }

    /// 能否后退。
    pub fn can_back(&self) -> bool {
        self.cursor > 0
    }

    /// 能否前进。
    pub fn can_forward(&self) -> bool {
        self.cursor + 1 < self.entries.len()
    }

    /// 后退一步,返回要打开的条目。
    pub fn back(&mut self) -> Option<T> {
        if !self.can_back() {
            return None;
        }
        self.cursor -= 1;
        self.entries.get(self.cursor).cloned()
    }

    /// 前进一步,返回要打开的条目。
    pub fn forward(&mut self) -> Option<T> {
        if !self.can_forward() {
            return None;
        }
        self.cursor += 1;
        self.entries.get(self.cursor).cloned()
    }

    /// 条目失效(会话被删除、文件被关闭)后,把它从历史里去掉并保持当前位置指向同一条记录。
    ///
    /// 参数:`entry` 要去掉的条目。
    pub fn remove(&mut self, entry: &T) {
        let before_cursor = self.entries.iter().take(self.cursor).filter(|e| *e == entry).count();
        self.entries.retain(|e| e != entry);
        self.cursor = self.cursor.saturating_sub(before_cursor).min(self.entries.len().saturating_sub(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 后退前进() {
        let mut h = NavHistory::<&str>::default();
        h.visit("a");
        h.visit("b");
        h.visit("c");
        assert!(h.can_back() && !h.can_forward());
        assert_eq!(h.back(), Some("b"));
        assert_eq!(h.back(), Some("a"));
        assert_eq!(h.back(), None);
        assert_eq!(h.forward(), Some("b"));
    }

    #[::core::prelude::v1::test]
    fn 后退后新打开会丢掉前进记录() {
        let mut h = NavHistory::<&str>::default();
        for id in ["a", "b", "c"] {
            h.visit(id);
        }
        h.back();
        h.visit("d");
        assert!(!h.can_forward());
        assert_eq!(h.back(), Some("b"));
    }

    #[::core::prelude::v1::test]
    fn 泛型条目可以是文件加行号() {
        let mut h = NavHistory::<(String, u32)>::default();
        h.visit(("a.rs".into(), 1));
        h.visit(("a.rs".into(), 40));
        h.visit(("b.rs".into(), 3));
        assert_eq!(h.back(), Some(("a.rs".into(), 40)));
        assert_eq!(h.back(), Some(("a.rs".into(), 1)));
        assert_eq!(h.forward(), Some(("a.rs".into(), 40)));
    }

    #[::core::prelude::v1::test]
    fn 重复打开同一个不记录() {
        let mut h = NavHistory::<&str>::default();
        h.visit("a");
        h.visit("a");
        assert!(!h.can_back());
    }

    #[::core::prelude::v1::test]
    fn 删除会话后位置不错乱() {
        let mut h = NavHistory::<&str>::default();
        for id in ["a", "b", "c"] {
            h.visit(id);
        }
        h.remove(&"a");
        assert_eq!(h.back(), Some("b"));
        h.remove(&"c");
        assert!(!h.can_forward());
    }
}
