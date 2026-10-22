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

/// 把 `/Users/x/proj` 一类路径缩成末级目录名。
pub fn project_name_from_path(path: &str) -> String {
    path.trim_end_matches('/').rsplit('/').next().unwrap_or(path).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 路径取末级目录() {
        assert_eq!(project_name_from_path("/a/b/c/"), "c");
        assert_eq!(project_name_from_path("solo"), "solo");
    }
}
