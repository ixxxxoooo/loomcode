//! IDE 标签页模型:参考 Zed `Pane` 的做法,与界面无关,可单独测试。
//!
//! - 新标签插在当前标签右侧(而不是末尾),已打开的文件只激活不重复。
//! - 预览标签:单击文件树打开的是预览(斜体),再打开别的预览会原位替换它;
//!   编辑、保存、双击标签或从树里双击都会把它固定下来。
//! - 关闭当前标签后回到最近用过的标签(激活历史),而不是简单取邻居。
//!
//! @author ixxxxoooo

use std::path::{Path, PathBuf};

/// 标签页集合:只记录路径与顺序,文档内容由视图持有。
#[derive(Default)]
pub struct TabModel {
    /// 从左到右的标签
    order: Vec<PathBuf>,
    /// 当前标签
    active: Option<PathBuf>,
    /// 预览标签(至多一个)
    preview: Option<PathBuf>,
    /// 激活历史,最近的在末尾
    history: Vec<PathBuf>,
    /// 固定的标签(总是排在最左边,「关闭其他 / 右侧」不会关它们)
    pinned: Vec<PathBuf>,
}

/// 打开文件的结果:告诉视图要不要建新文档、哪个预览被顶掉了。
pub struct OpenOutcome {
    /// 被替换掉的预览标签(其文档应丢弃)
    pub replaced: Option<PathBuf>,
}

impl TabModel {
    /// 打开(激活)一个文件。`preview` 为真时作为预览标签;已存在的标签不会降级。
    pub fn open(&mut self, path: &Path, preview: bool) -> OpenOutcome {
        let existed = self.order.iter().any(|p| p == path);
        let mut replaced = None;
        if !existed {
            if preview && let Some(old) = self.preview.take() {
                // 原位替换旧预览
                if let Some(ix) = self.order.iter().position(|p| *p == old) {
                    self.order[ix] = path.to_path_buf();
                    self.history.retain(|p| *p != old);
                    replaced = Some(old);
                }
            }
            if replaced.is_none() {
                let at = self.active.as_ref().and_then(|a| self.order.iter().position(|p| p == a)).map_or(0, |i| i + 1);
                self.order.insert(at.min(self.order.len()), path.to_path_buf());
            }
            self.preview = preview.then(|| path.to_path_buf());
        } else if !preview && self.preview.as_deref() == Some(path) {
            // 对预览标签再次「正式打开」= 固定
            self.preview = None;
        }
        self.activate(path);
        OpenOutcome { replaced }
    }

    /// 激活已有标签并记入历史。
    pub fn activate(&mut self, path: &Path) {
        if !self.order.iter().any(|p| p == path) {
            return;
        }
        self.history.retain(|p| p != path);
        self.history.push(path.to_path_buf());
        self.active = Some(path.to_path_buf());
    }

    /// 固定预览标签(编辑 / 保存 / 双击时调用)。
    pub fn pin(&mut self, path: &Path) {
        if self.preview.as_deref() == Some(path) {
            self.preview = None;
        }
    }

    /// 关闭标签;返回关闭后应激活的标签(若关的是当前标签)。
    pub fn close(&mut self, path: &Path) -> Option<PathBuf> {
        let ix = self.order.iter().position(|p| p == path)?;
        self.order.remove(ix);
        self.pinned.retain(|p| p != path);
        self.history.retain(|p| p != path);
        if self.preview.as_deref() == Some(path) {
            self.preview = None;
        }
        if self.active.as_deref() == Some(path) {
            // 先回最近用过的,没有历史就取右邻 / 左邻
            let next = self.history.last().cloned().or_else(|| self.order.get(ix).or_else(|| self.order.last()).cloned());
            self.active = None;
            if let Some(n) = &next {
                self.activate(n);
            }
            return next;
        }
        None
    }

    /// 除 `keep` 之外的标签(用于「关闭其他」)。
    pub fn others(&self, keep: &Path) -> Vec<PathBuf> {
        self.order.iter().filter(|p| *p != keep && !self.is_pinned(p)).cloned().collect()
    }

    /// `path` 右侧的标签(用于「关闭右侧」)。
    pub fn right_of(&self, path: &Path) -> Vec<PathBuf> {
        match self.order.iter().position(|p| p == path) {
            Some(ix) => self.order[ix + 1..].iter().filter(|p| !self.is_pinned(p)).cloned().collect(),
            None => Vec::new(),
        }
    }

    /// 相邻切换:`step` 为 +1 / -1,首尾循环。
    pub fn cycle(&mut self, step: isize) -> Option<PathBuf> {
        let cur = self.active.as_ref().and_then(|a| self.order.iter().position(|p| p == a))? as isize;
        let n = self.order.len() as isize;
        let next = self.order[(cur + step).rem_euclid(n) as usize].clone();
        self.activate(&next);
        Some(next)
    }

    /// 切换「固定」:固定的标签移到最左侧固定区的末尾,取消固定则移到固定区之后。
    pub fn toggle_pin(&mut self, path: &Path) {
        let Some(ix) = self.order.iter().position(|p| p == path) else { return };
        let item = self.order.remove(ix);
        if let Some(pix) = self.pinned.iter().position(|p| p == path) {
            self.pinned.remove(pix);
        } else {
            self.pinned.push(item.clone());
            self.preview = self.preview.take().filter(|p| p != path);
        }
        self.order.insert(self.pinned.len() - usize::from(self.is_pinned(path)), item);
    }

    /// 是否为固定标签。
    pub fn is_pinned(&self, path: &Path) -> bool {
        self.pinned.iter().any(|p| p == path)
    }

    /// 把 `path` 拖到 `target` 的位置(向右拖落在目标之后,向左拖落在目标之前);固定与非固定不互相越界。
    pub fn move_to(&mut self, path: &Path, target: &Path) {
        if path == target || self.is_pinned(path) != self.is_pinned(target) {
            return;
        }
        let (Some(from), Some(to)) = (self.order.iter().position(|p| p == path), self.order.iter().position(|p| p == target)) else { return };
        let item = self.order.remove(from);
        self.order.insert(to, item);
        if self.is_pinned(path) {
            self.pinned = self.order.iter().filter(|p| self.pinned.contains(p)).cloned().collect();
        }
    }

    /// 所有标签。
    pub fn tabs(&self) -> &[PathBuf] {
        &self.order
    }

    /// 当前标签。
    pub fn active(&self) -> Option<&Path> {
        self.active.as_deref()
    }

    /// 是否为预览标签。
    pub fn is_preview(&self, path: &Path) -> bool {
        self.preview.as_deref() == Some(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn 预览标签被下一个预览原位替换() {
        let mut m = TabModel::default();
        m.open(&p("a"), true);
        m.open(&p("b"), false);
        let out = m.open(&p("c"), true);
        assert!(out.replaced.is_none());
        let out = m.open(&p("d"), true);
        assert_eq!(out.replaced, Some(p("c")));
        assert_eq!(m.tabs(), [p("a"), p("b"), p("d")]);
    }

    #[test]
    fn 固定后不再被替换() {
        let mut m = TabModel::default();
        m.open(&p("a"), true);
        m.pin(&p("a"));
        let out = m.open(&p("b"), true);
        assert!(out.replaced.is_none());
        assert_eq!(m.tabs().len(), 2);
    }

    #[test]
    fn 新标签插在当前标签右侧() {
        let mut m = TabModel::default();
        m.open(&p("a"), false);
        m.open(&p("b"), false);
        m.activate(&p("a"));
        m.open(&p("c"), false);
        assert_eq!(m.tabs(), [p("a"), p("c"), p("b")]);
    }

    #[test]
    fn 关闭当前标签回到最近使用的() {
        let mut m = TabModel::default();
        for n in ["a", "b", "c"] {
            m.open(&p(n), false);
        }
        m.activate(&p("a"));
        m.activate(&p("c"));
        assert_eq!(m.close(&p("c")), Some(p("a")));
        assert_eq!(m.active(), Some(p("a").as_path()));
    }

    #[test]
    fn 其他与右侧() {
        let mut m = TabModel::default();
        for n in ["a", "b", "c"] {
            m.open(&p(n), false);
        }
        assert_eq!(m.others(&p("b")), [p("a"), p("c")]);
        assert_eq!(m.right_of(&p("a")), [p("b"), p("c")]);
    }

    #[test]
    fn 固定与拖动() {
        let mut m = TabModel::default();
        for n in ["a", "b", "c", "d"] {
            m.open(&p(n), false);
        }
        m.toggle_pin(&p("c"));
        assert_eq!(m.tabs(), [p("c"), p("a"), p("b"), p("d")]);
        assert_eq!(m.others(&p("a")), [p("b"), p("d")]);
        m.move_to(&p("d"), &p("a"));
        assert_eq!(m.tabs(), [p("c"), p("d"), p("a"), p("b")]);
        // 固定与非固定之间不能互拖
        m.move_to(&p("b"), &p("c"));
        assert_eq!(m.tabs()[0], p("c"));
        m.toggle_pin(&p("c"));
        assert!(!m.is_pinned(&p("c")));
    }

    #[test]
    fn 循环切换() {
        let mut m = TabModel::default();
        m.open(&p("a"), false);
        m.open(&p("b"), false);
        assert_eq!(m.cycle(1), Some(p("a")));
        assert_eq!(m.cycle(-1), Some(p("b")));
    }
}
