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
    /// 上次所在项目的目录;会话没发过消息(没写进历史)时靠它回到那个项目
    pub project: Option<String>,
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
    /// 窗口布局:左右栏开关、所在页面、IDE 是否开着、侧栏项目展开等
    pub layout: LayoutMemory,
}

/// 窗口布局记忆。`Option` 字段为 `None` 表示从没记录过,沿用默认值。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LayoutMemory {
    /// Agent 窗口左侧栏是否展开(窗口变窄被自动收起时记收起前的状态)
    pub left_open: Option<bool>,
    /// Agent 窗口右侧面板是否展开
    pub right_open: Option<bool>,
    /// 主区域页面键:chat / settings / canvas / automation / browser / customize
    pub page: Option<String>,
    /// 关闭时 IDE 是否开着(融合模式的 IDE 视图,或独立 IDE 窗口)
    pub ide: bool,
    /// 侧栏里展开的项目路径;`None` 表示从没记录过
    pub expanded_projects: Option<Vec<String>>,
    /// 侧栏是否显示已归档会话
    pub show_archived: bool,
    /// IDE 内部布局
    pub ide_layout: IdeLayout,
}

/// IDE 窗口内部的布局记忆。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct IdeLayout {
    /// 左栏(文件 / 搜索 / 改动)是否展开;`None` = 默认展开
    pub left_open: Option<bool>,
    /// 对话栏是否展开;`None` = 默认展开
    pub chat_open: Option<bool>,
    /// 左栏是否显示搜索
    pub search_open: bool,
    /// 底部终端面板是否展开
    pub term_open: bool,
    /// 对话栏是否展开为占满窗口
    pub chat_wide: bool,
    /// 左栏选中的分段键(files / changes)
    pub left_tab: Option<String>,
    /// 左栏宽度;`None` = 默认
    pub left_w: Option<f32>,
    /// 对话栏宽度;`None` = 默认
    pub chat_w: Option<f32>,
    /// 底部终端面板高度;`None` = 默认
    pub term_h: Option<f32>,
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
    fn 布局记忆往返且旧文件缺字段时为默认() {
        let old: LastState = serde_json::from_str(r#"{"session":"abc"}"#).unwrap();
        assert_eq!(old.layout, LayoutMemory::default());
        assert_eq!(old.layout.left_open, None);
        let s = LastState {
            layout: LayoutMemory {
                left_open: Some(false),
                right_open: Some(true),
                page: Some("canvas".into()),
                ide: true,
                expanded_projects: Some(vec!["/a".into()]),
                show_archived: true,
                ide_layout: IdeLayout { left_open: Some(false), term_open: true, left_tab: Some("changes".into()), ..Default::default() },
            },
            ..Default::default()
        };
        let back: LastState = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn 往返序列化() {
        let s = LastState { session: Some("a".into()), chat_tabs: vec!["a".into(), "b".into()], right_tabs: Some(vec![]), right_tab: Some("files".into()), files_current: Some("/x/y.rs".into()), tree_collapsed: true, ..Default::default() };
        let back: LastState = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(s, back);
    }
}
