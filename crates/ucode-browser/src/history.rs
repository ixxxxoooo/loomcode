//! 前进 / 后退历史。

/// 浏览历史:`visit` 会丢弃当前位置之后的「前进」记录。
#[derive(Debug, Clone, Default)]
pub struct History {
    entries: Vec<String>,
    index: usize,
}

impl History {
    pub fn current(&self) -> Option<&str> {
        self.entries.get(self.index).map(String::as_str)
    }

    pub fn visit(&mut self, url: &str) {
        if self.current() == Some(url) {
            return;
        }
        if !self.entries.is_empty() {
            self.entries.truncate(self.index + 1);
        }
        self.entries.push(url.to_string());
        self.index = self.entries.len() - 1;
    }

    pub fn can_back(&self) -> bool {
        self.index > 0
    }

    pub fn can_forward(&self) -> bool {
        self.index + 1 < self.entries.len()
    }

    pub fn back(&mut self) -> Option<&str> {
        if self.can_back() {
            self.index -= 1;
        }
        self.current()
    }

    pub fn forward(&mut self) -> Option<&str> {
        if self.can_forward() {
            self.index += 1;
        }
        self.current()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 前进后退与分支截断() {
        let mut h = History::default();
        assert!(h.current().is_none() && !h.can_back());
        h.visit("a");
        h.visit("b");
        h.visit("c");
        assert_eq!(h.back(), Some("b"));
        assert_eq!(h.back(), Some("a"));
        assert_eq!(h.back(), Some("a"), "到头后保持不动");
        assert_eq!(h.forward(), Some("b"));
        h.visit("d");
        assert!(!h.can_forward(), "新访问丢弃前进记录");
        assert_eq!(h.len(), 3);
        h.visit("d");
        assert_eq!(h.len(), 3, "重复访问当前页不入栈");
    }
}
