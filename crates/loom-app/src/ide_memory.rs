//! IDE 的位置历史与项目记忆:前进 / 后退(文件 + 行)、最近文件、切换项目时保存与恢复标签。
//!
//! @author ixxxxoooo

use crate::ide::IdeView;
use gpui_kit::*;
use loom_config::{IdeMemory, IdeTabMemory};
use std::path::{Path, PathBuf};

impl IdeView {
    /// 某文件当前光标所在行(1 起始);没有文档(图片、尚未打开)时为 1。
    pub(crate) fn cursor_line(&self, path: &Path, cx: &App) -> u32 {
        self.docs.get(path).map_or(1, |d| d.editor.read(cx).cursor_position().line + 1)
    }

    /// 打开 / 激活文件:记入最近文件与位置历史。
    pub(crate) fn record_open(&mut self, path: &Path, line: u32) {
        if self.nav_quiet || crate::ide_diff::is_diff(path) || crate::ide_untitled::is_untitled(path) {
            return;
        }
        let key = path.to_string_lossy().to_string();
        self.recent.retain(|p| *p != key);
        self.recent.insert(0, key);
        self.recent.truncate(loom_config::ui_state::MAX_RECENT);
        self.nav.visit((path.to_path_buf(), line));
        self.fresh_open = true;
    }

    /// 在文件里跳行:刚打开文件的那一跳替换历史里的「第 1 行」,其余新增一条。
    pub(crate) fn record_jump(&mut self, path: &Path, line: u32) {
        if self.nav_quiet {
            return;
        }
        if std::mem::take(&mut self.fresh_open) {
            self.nav.replace_current((path.to_path_buf(), line));
        } else {
            self.nav.visit((path.to_path_buf(), line));
        }
    }

    /// 按位置历史后退 / 前进:打开(或激活)那个文件并跳到记录的行。
    ///
    /// 返回:实际打开的文件(宿主据此在文件树里定位);没有可去的位置返回 `None`。
    pub fn navigate(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) -> Option<PathBuf> {
        loop {
            let (path, line) = if forward { self.nav.forward() } else { self.nav.back() }?;
            if !path.exists() {
                self.nav.remove(&(path, line));
                continue;
            }
            self.nav_quiet = true;
            let preview = !self.docs.contains_key(&path) && !self.images.contains(&path) && !self.sheets.contains_key(&path);
            self.open_inner(path.clone(), preview, true, window, cx);
            self.goto(line, 1, window, cx);
            self.nav_quiet = false;
            return Some(path);
        }
    }

    /// 当前标签状态 → 记忆(展开的目录由宿主从文件树补上)。
    pub fn snapshot(&self) -> IdeMemory {
        IdeMemory {
            tabs: self
                .tabs
                .tabs()
                .iter()
                .filter(|p| !crate::ide_diff::is_diff(p) && !crate::ide_untitled::is_untitled(p))
                .map(|p| IdeTabMemory { path: p.to_string_lossy().to_string(), preview: self.tabs.is_preview(p), pinned: self.tabs.is_pinned(p) })
                .collect(),
            active: self.tabs.active().filter(|p| !crate::ide_diff::is_diff(p) && !crate::ide_untitled::is_untitled(p)).map(|p| p.to_string_lossy().to_string()),
            recent: self.recent.clone(),
            expanded: Vec::new(),
        }
    }

    /// 清空所有标签与历史(切换项目前调用)。
    pub fn clear(&mut self) {
        self.docs.clear();
        self.images.clear();
        self.sheets.clear();
        self.sheet_subs.clear();
        self.diffs.clear();
        self.tabs = Default::default();
        self.note = None;
        self.nav = Default::default();
        self.fresh_open = false;
        self.recent.clear();
    }

    /// 处理切换项目后挂起的记忆:按原顺序重新打开标签,不抢焦点、不记历史。渲染时调用。
    pub(crate) fn restore_pending(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mem) = self.pending_restore.take() else { return };
        self.nav_quiet = true;
        for t in &mem.tabs {
            let p = PathBuf::from(&t.path);
            self.open_inner(p.clone(), t.preview, false, window, cx);
            if t.pinned {
                self.tabs.toggle_pin(&p);
            }
        }
        if let Some(active) = mem.active.as_deref() {
            self.tabs.activate(Path::new(active));
        }
        self.recent = mem.recent;
        self.nav_quiet = false;
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use crate::nav_history::NavHistory;
    use std::path::PathBuf;

    #[::core::prelude::v1::test]
    fn 打开后立刻跳行只留一条() {
        // 模拟 record_open + record_jump 的历史语义:同一次打开里的跳行替换第 1 行那条
        let mut h = NavHistory::<(PathBuf, u32)>::default();
        h.visit((PathBuf::from("a.rs"), 1));
        h.replace_current((PathBuf::from("a.rs"), 42));
        h.visit((PathBuf::from("b.rs"), 1));
        assert_eq!(h.back(), Some((PathBuf::from("a.rs"), 42)));
        assert_eq!(h.back(), None);
    }
}
