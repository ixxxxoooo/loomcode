//! 界面状态(`~/.ucode/state.json`):会话置顶 / 归档 / 自定义名、项目置顶 / 隐藏。
//!
//! 会话本体仍由 Claude Code 保存在 `~/.claude/projects`,这里只记用户自己的选择。
//!
//! @author ixxxxoooo

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use ucode_core::SessionMeta;

/// 用户对侧栏做的选择。所有字段都有默认值,旧文件缺字段时不会报错。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UiState {
    /// 置顶的会话 id
    pub pinned: BTreeSet<String>,
    /// 归档的会话 id
    pub archived: BTreeSet<String>,
    /// 会话 id → 自定义名称
    pub names: BTreeMap<String, String>,
    /// 置顶的项目路径
    pub pinned_projects: BTreeSet<String>,
    /// 从侧栏移除(隐藏)的项目路径
    pub hidden_projects: BTreeSet<String>,
}

impl UiState {
    /// 默认存放位置 `~/.ucode/state.json`。
    pub fn default_path() -> Option<PathBuf> {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".ucode").join("state.json"))
    }

    /// 读取;文件不存在或损坏时返回默认值。
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    /// 写入(自动创建目录);先写临时文件再改名,避免写一半崩溃。
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self).map_err(std::io::Error::other)?)?;
        std::fs::rename(tmp, path)
    }

    /// 把置顶 / 归档 / 自定义名合并进会话列表,置顶的会话稳定排到最前。
    ///
    /// 参数:`sessions` 待合并的会话列表(原地修改)。
    pub fn apply(&self, sessions: &mut [SessionMeta]) {
        for s in sessions.iter_mut() {
            s.pinned = self.pinned.contains(&s.id);
            s.archived = self.archived.contains(&s.id);
            if let Some(name) = self.names.get(&s.id).filter(|n| !n.trim().is_empty()) {
                s.title = name.clone();
            }
        }
        sessions.sort_by_key(|s| !s.pinned);
    }

    /// 切换会话置顶,返回切换后的状态。
    pub fn toggle_pin(&mut self, id: &str) -> bool {
        toggle(&mut self.pinned, id)
    }

    /// 设置会话归档状态。归档会同时取消置顶。
    pub fn set_archived(&mut self, id: &str, archived: bool) {
        if archived {
            self.archived.insert(id.to_string());
            self.pinned.remove(id);
        } else {
            self.archived.remove(id);
        }
    }

    /// 重命名会话;空白名称表示恢复原标题。
    pub fn rename(&mut self, id: &str, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            self.names.remove(id);
        } else {
            self.names.insert(id.to_string(), name.to_string());
        }
    }

    /// 会话被删除后清理它留下的所有记录。
    pub fn forget_session(&mut self, id: &str) {
        self.pinned.remove(id);
        self.archived.remove(id);
        self.names.remove(id);
    }

    /// 切换项目置顶,返回切换后的状态。
    pub fn toggle_pin_project(&mut self, path: &str) -> bool {
        toggle(&mut self.pinned_projects, path)
    }
}

fn toggle(set: &mut BTreeSet<String>, key: &str) -> bool {
    if set.remove(key) {
        false
    } else {
        set.insert(key.to_string());
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ucode_core::SessionBadge;

    fn meta(id: &str, title: &str) -> SessionMeta {
        SessionMeta { id: id.into(), title: title.into(), updated: String::new(), pinned: false, archived: false, badge: SessionBadge::Idle }
    }

    #[test]
    fn 合并后置顶在前且改名生效() {
        let mut st = UiState::default();
        st.toggle_pin("b");
        st.rename("c", " 我的名字 ");
        st.set_archived("a", true);
        let mut list = vec![meta("a", "A"), meta("b", "B"), meta("c", "C")];
        st.apply(&mut list);
        assert_eq!(list.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["b", "a", "c"]);
        assert!(list[1].archived);
        assert_eq!(list[2].title, "我的名字");
    }

    #[test]
    fn 归档会取消置顶_空名恢复原标题() {
        let mut st = UiState::default();
        st.toggle_pin("a");
        st.set_archived("a", true);
        assert!(!st.pinned.contains("a"));
        st.rename("a", "x");
        st.rename("a", "  ");
        assert!(st.names.is_empty());
    }

    #[test]
    fn 删除会话清理记录() {
        let mut st = UiState::default();
        st.toggle_pin("a");
        st.rename("a", "x");
        st.set_archived("a", true);
        st.forget_session("a");
        assert_eq!(st, UiState::default());
    }

    #[test]
    fn 保存后可读回() {
        let dir = std::env::temp_dir().join(format!("ucode-state-{}", std::process::id()));
        let path = dir.join("state.json");
        let mut st = UiState::default();
        st.toggle_pin("a");
        st.toggle_pin_project("/p");
        st.save(&path).unwrap();
        assert_eq!(UiState::load(&path), st);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn 损坏文件返回默认值() {
        let dir = std::env::temp_dir().join(format!("ucode-state-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.json");
        std::fs::write(&path, "{坏的").unwrap();
        assert_eq!(UiState::load(&path), UiState::default());
        let _ = std::fs::remove_dir_all(dir);
    }
}
