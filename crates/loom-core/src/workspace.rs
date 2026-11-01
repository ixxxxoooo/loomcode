//! 项目与会话列表模型(侧栏的数据)。

use serde::{Deserialize, Serialize};

/// 会话的运行状态点。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SessionBadge {
    #[default]
    Idle,
    /// 运行中
    Running,
    /// 等待用户(权限 / 提问)
    Waiting,
    /// 有未读结果
    Unread,
}

/// 侧栏里的一个会话。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: String,
    pub title: String,
    /// 最近活动的时间描述(演示用字符串,真实模式由时间戳格式化)
    pub updated: String,
    pub pinned: bool,
    /// 已归档(侧栏默认隐藏)
    #[serde(default)]
    pub archived: bool,
    pub badge: SessionBadge,
}

/// 一个项目(工作目录)及其会话。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub path: String,
    /// 当前 Git 分支
    pub branch: Option<String>,
    pub sessions: Vec<SessionMeta>,
    pub expanded: bool,
}

/// 多根工作区:几个平级文件夹组成一个工作区,第一个是主目录。
///
/// 会话历史存在 `dir`(工作区自己的专属目录,作为 claude 的 cwd)名下,
/// `folders` 里的真实文件夹通过 `--add-dir` 授权访问。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceDef {
    pub id: String,
    pub name: String,
    /// 工作区专属目录(绝对路径,作为 claude 的 cwd)
    pub dir: String,
    /// 真实文件夹;第一个是主目录
    pub folders: Vec<String>,
}

impl WorkspaceDef {
    /// 校验工作区是否可保存:名称非空、至少一个文件夹、没有重复文件夹。
    ///
    /// 返回:合法返回 `Ok(())`,否则返回给用户看的中文原因。
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("请填写工作区名称".into());
        }
        if self.folders.is_empty() {
            return Err("请至少添加一个文件夹".into());
        }
        let mut seen = std::collections::HashSet::new();
        for f in &self.folders {
            if !seen.insert(f.trim_end_matches('/')) {
                return Err(format!("文件夹重复:{f}"));
            }
        }
        Ok(())
    }

    /// 添加一个文件夹;已存在(忽略末尾斜杠)时不重复添加。
    ///
    /// 返回:是否真的添加了。
    pub fn add_folder(&mut self, path: &str) -> bool {
        let key = path.trim_end_matches('/');
        if key.is_empty() || self.folders.iter().any(|f| f.trim_end_matches('/') == key) {
            return false;
        }
        self.folders.push(key.to_string());
        true
    }

    /// 移除指定下标的文件夹;下标越界时忽略。
    pub fn remove_folder(&mut self, ix: usize) {
        if ix < self.folders.len() {
            self.folders.remove(ix);
        }
    }

    /// 把指定下标的文件夹设为主目录(移到最前);下标越界时忽略。
    pub fn make_primary(&mut self, ix: usize) {
        if ix < self.folders.len() {
            let f = self.folders.remove(ix);
            self.folders.insert(0, f);
        }
    }

    /// 追加到系统提示末尾的工作区说明,告诉 claude 真实文件夹在哪。
    pub fn system_prompt(&self) -> String {
        let list: String = self.folders.iter().enumerate().map(|(i, f)| format!("- {f}{}\n", if i == 0 { "(主目录)" } else { "" })).collect();
        format!("<workspace name=\"{}\">\n当前会话的工作区包含以下文件夹,都可以读写;当前目录只是工作区的元数据目录,请到下面的文件夹里工作:\n{list}</workspace>", self.name)
    }
}

/// 把 `/Users/x/proj` 一类路径缩成末级目录名。
pub fn project_name_from_path(path: &str) -> String {
    path.trim_end_matches('/').rsplit('/').next().unwrap_or(path).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(name: &str, folders: &[&str]) -> WorkspaceDef {
        WorkspaceDef { id: "w1".into(), name: name.into(), dir: "/ws/w1".into(), folders: folders.iter().map(|s| s.to_string()).collect() }
    }

    #[test]
    fn 工作区校验() {
        assert!(ws("前后端", &["/a", "/b"]).validate().is_ok());
        assert!(ws("  ", &["/a"]).validate().is_err());
        assert!(ws("x", &[]).validate().is_err());
        assert!(ws("x", &["/a", "/a/"]).validate().unwrap_err().contains("重复"));
    }

    #[test]
    fn 文件夹增删与设主目录() {
        let mut w = ws("x", &["/a"]);
        assert!(w.add_folder("/b/"));
        assert!(!w.add_folder("/b"), "忽略末尾斜杠后重复");
        assert!(!w.add_folder("/"), "根路径去斜杠后为空,拒绝");
        assert_eq!(w.folders, ["/a", "/b"]);
        w.make_primary(1);
        assert_eq!(w.folders, ["/b", "/a"]);
        w.remove_folder(9);
        w.make_primary(9);
        assert_eq!(w.folders.len(), 2);
        w.remove_folder(0);
        assert_eq!(w.folders, ["/a"]);
    }

    #[test]
    fn 工作区提示词列出全部文件夹() {
        let p = ws("前后端", &["/a", "/b"]).system_prompt();
        assert!(p.contains("- /a(主目录)") && p.contains("- /b\n") && p.contains("前后端"));
    }

    #[test]
    fn 路径取末级目录() {
        assert_eq!(project_name_from_path("/a/b/c/"), "c");
        assert_eq!(project_name_from_path("solo"), "solo");
    }
}
