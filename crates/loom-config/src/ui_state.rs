//! 界面状态(`~/.loomcode/state.json`):会话置顶 / 归档 / 自定义名、项目置顶 / 隐藏。
//!
//! 会话本体仍由 Claude Code 保存在 `~/.claude/projects`,这里只记用户自己的选择。
//!
//! @author ixxxxoooo

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use loom_core::{Project, SessionMeta, WorkspaceDef};

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
    /// 项目路径 → 自定义显示名(只改显示,不改目录)
    pub project_names: BTreeMap<String, String>,
    /// 多根工作区
    pub workspaces: Vec<WorkspaceDef>,
}

impl UiState {
    /// 默认存放位置 `~/.loomcode/state.json`。
    pub fn default_path() -> Option<PathBuf> {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".loomcode").join("state.json"))
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

    /// 重命名项目的显示名;空白表示恢复目录名。
    pub fn rename_project(&mut self, path: &str, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            self.project_names.remove(path);
        } else {
            self.project_names.insert(path.to_string(), name.to_string());
        }
    }

    /// 从侧栏移除项目(只是隐藏,不动磁盘上的文件),同时取消置顶。
    pub fn hide_project(&mut self, path: &str) {
        self.hidden_projects.insert(path.to_string());
        self.pinned_projects.remove(path);
    }

    /// 重新显示被移除的项目(再次添加同一目录时调用)。
    pub fn unhide_project(&mut self, path: &str) {
        self.hidden_projects.remove(path);
    }

    /// 把自定义项目名合并进项目列表,置顶项目稳定排到最前。
    ///
    /// 参数:`projects` 待合并的项目列表(原地修改)。
    pub fn apply_projects(&self, projects: &mut [Project]) {
        for p in projects.iter_mut() {
            if let Some(name) = self.project_names.get(&p.path) {
                p.name = name.clone();
            }
        }
        projects.sort_by_key(|p| !self.pinned_projects.contains(&p.path));
    }

    /// 新增或更新一个工作区(按 id 匹配)。
    pub fn upsert_workspace(&mut self, def: WorkspaceDef) {
        match self.workspaces.iter_mut().find(|w| w.id == def.id) {
            Some(old) => *old = def,
            None => self.workspaces.push(def),
        }
    }

    /// 按工作区专属目录查找工作区。
    pub fn workspace_by_dir(&self, dir: &str) -> Option<&WorkspaceDef> {
        self.workspaces.iter().find(|w| w.dir == dir)
    }

    /// 把工作区合并进项目列表:已有会话历史的项目改用工作区名称,还没有历史的补一个空项目。
    ///
    /// 参数:`projects` 项目列表(原地修改,新增的项目排在最前)。
    pub fn apply_workspaces(&self, projects: &mut Vec<Project>) {
        for w in &self.workspaces {
            match projects.iter_mut().find(|p| p.path == w.dir) {
                Some(p) => p.name = w.name.clone(),
                None => projects.insert(0, Project { name: w.name.clone(), path: w.dir.clone(), branch: None, sessions: Vec::new(), expanded: false }),
            }
        }
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
    use loom_core::SessionBadge;

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
    fn 项目置顶改名与隐藏() {
        let mk = |n: &str, path: &str| Project { name: n.into(), path: path.into(), branch: None, sessions: vec![], expanded: false };
        let mut st = UiState::default();
        st.toggle_pin_project("/b");
        st.rename_project("/c", " 自定义 ");
        let mut list = vec![mk("a", "/a"), mk("b", "/b"), mk("c", "/c")];
        st.apply_projects(&mut list);
        assert_eq!(list.iter().map(|p| p.path.as_str()).collect::<Vec<_>>(), ["/b", "/a", "/c"]);
        assert_eq!(list[2].name, "自定义");
        st.hide_project("/b");
        assert!(st.hidden_projects.contains("/b") && !st.pinned_projects.contains("/b"));
        st.unhide_project("/b");
        assert!(st.hidden_projects.is_empty());
        st.rename_project("/c", "");
        assert!(st.project_names.is_empty());
    }

    fn ws(id: &str, name: &str) -> WorkspaceDef {
        WorkspaceDef { id: id.into(), name: name.into(), dir: format!("/ws/{id}"), folders: vec!["/a".into(), "/b".into()] }
    }

    #[test]
    fn 工作区增改查与合并进项目() {
        let mut st = UiState::default();
        st.upsert_workspace(ws("1", "前后端"));
        st.upsert_workspace(ws("1", "改名后"));
        assert_eq!(st.workspaces.len(), 1);
        assert_eq!(st.workspace_by_dir("/ws/1").unwrap().name, "改名后");
        assert!(st.workspace_by_dir("/nope").is_none());
        // 已有历史的项目改名,没有历史的补空项目
        st.upsert_workspace(ws("2", "第二个"));
        let mk = |path: &str| Project { name: "目录名".into(), path: path.into(), branch: None, sessions: vec![], expanded: false };
        let mut list = vec![mk("/ws/1"), mk("/other")];
        st.apply_workspaces(&mut list);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].path, "/ws/2");
        assert_eq!(list.iter().find(|p| p.path == "/ws/1").unwrap().name, "改名后");
        assert_eq!(list.iter().find(|p| p.path == "/other").unwrap().name, "目录名");
    }

    #[test]
    fn 保存后可读回() {
        let dir = std::env::temp_dir().join(format!("loom-state-{}", std::process::id()));
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
        let dir = std::env::temp_dir().join(format!("loom-state-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.json");
        std::fs::write(&path, "{坏的").unwrap();
        assert_eq!(UiState::load(&path), UiState::default());
        let _ = std::fs::remove_dir_all(dir);
    }
}
