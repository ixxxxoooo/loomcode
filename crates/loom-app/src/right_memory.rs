//! 右栏状态的记忆:把已打开的标签、选中的标签、预览的文件写进 / 读出 [`LastState`]。
//!
//! @author ixxxxoooo

use crate::right_panel::RightPanel;
use crate::workbench::RightTab;
use loom_config::LastState;

impl RightTab {
    /// 由键(见 [`RightTab::key`])还原;未知键返回 `None`。
    pub fn from_key(key: &str) -> Option<RightTab> {
        RightTab::ALL.into_iter().find(|t| t.key() == key)
    }
}

impl RightPanel {
    /// 把右栏当前状态写进 `last`(会话 id 与对话栏标签不在这里管)。
    pub fn write_last(&self, last: &mut LastState) {
        last.right_tabs = Some(self.open.iter().map(|t| t.key().to_string()).collect());
        last.right_tab = Some(self.tab.key().to_string());
        // 预览的文件现在由编辑器的标签记忆(`state.json` 的 `ide`)保存
        last.files_current = None;
        last.tree_collapsed = self.tree_collapsed;
        let (hidden, collapsed) = self.sections.to_keys();
        last.explorer_hidden = hidden;
        last.explorer_collapsed = Some(collapsed);
    }

    /// 按上次的状态恢复标签与树的收起状态;打开的文件由编辑器的标签记忆恢复。
    pub fn restore_last(&mut self, last: &LastState) {
        if let Some(keys) = &last.right_tabs {
            self.open = keys.iter().filter_map(|k| RightTab::from_key(k)).collect();
        }
        let wanted = last.right_tab.as_deref().and_then(RightTab::from_key).filter(|t| self.open.contains(t));
        if let Some(t) = wanted.or_else(|| self.open.first().copied()) {
            self.tab = t;
        }
        self.tree_collapsed = last.tree_collapsed;
        self.sections = crate::explorer_sections::SectionState::from_keys(&last.explorer_hidden, last.explorer_collapsed.as_deref());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 键往返() {
        for t in RightTab::ALL {
            assert_eq!(RightTab::from_key(t.key()), Some(t));
        }
        assert_eq!(RightTab::from_key("nope"), None);
    }
}
