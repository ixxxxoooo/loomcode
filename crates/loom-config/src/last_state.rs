//! 上次关闭时的界面状态(写在 `state.json` 的 `last` 里):下次启动时恢复。
//!
//! 会话内容本身由 Claude Code 保存在 `~/.claude/projects`,这里只记「当时看的是哪个、开了哪些标签」。
//!
//! @author ixxxxoooo

use serde::{Deserialize, Serialize};

/// 上次关闭时的界面状态。所有字段都有默认值,旧文件缺字段时不会报错。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LastState {
    /// 上次正在看的会话 id
    pub session: Option<String>,
    /// IDE 对话栏里打开的会话标签(顺序即标签顺序)
    pub chat_tabs: Vec<String>,
    /// Agent 右栏打开的标签键;`None` 表示从未记录过,用默认标签
    pub right_tabs: Option<Vec<String>>,
    /// Agent 右栏当前选中的标签键
    pub right_tab: Option<String>,
    /// 「文件」标签里正在预览的文件(绝对路径)
    pub files_current: Option<String>,
    /// 「文件」标签右侧的资源管理器是否收起
    pub tree_collapsed: bool,
    /// 资源管理器里被隐藏的区块键(见 `explorer_sections`)
    pub explorer_hidden: Vec<String>,
    /// 资源管理器里折叠的区块键;`None` 表示从没记录过,用默认(时间线折叠)
    pub explorer_collapsed: Option<Vec<String>>,
    /// 资源管理器区块设置的版本:2 起「打开的编辑器」默认隐藏(旧记录升级时补隐藏一次)
    pub explorer_v: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 旧文件缺字段时用默认值() {
        let s: LastState = serde_json::from_str(r#"{"session":"abc"}"#).unwrap();
        assert_eq!(s.session.as_deref(), Some("abc"));
        assert!(s.chat_tabs.is_empty() && s.right_tabs.is_none() && !s.tree_collapsed);
    }

    #[test]
    fn 往返序列化() {
        let s = LastState { session: Some("a".into()), chat_tabs: vec!["a".into(), "b".into()], right_tabs: Some(vec![]), right_tab: Some("files".into()), files_current: Some("/x/y.rs".into()), tree_collapsed: true, ..Default::default() };
        let back: LastState = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(s, back);
    }
}
