//! 终端集合:按项目根目录分组,Agent 窗口与 IDE 窗口共用(通过全局句柄)。
//!
//! @author ixxxxoooo

use crate::terminal_view::TerminalView;
use gpui_kit::*;
use std::collections::HashMap;

/// 终端分段里当前选中的子标签。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TermSel {
    /// 只读的「Agent 命令」记录
    #[default]
    Agent,
    Term(u64),
}

/// 一个终端标签。
pub struct TermTab {
    pub id: u64,
    pub name: String,
    pub view: Entity<TerminalView>,
    _sub: Subscription,
}

#[derive(Default)]
struct Group {
    tabs: Vec<TermTab>,
    active: TermSel,
    /// 本项目累计创建的终端数(用来起「终端 N」的名字)
    counter: u32,
}

/// 全部项目的终端。
#[derive(Default)]
pub struct ProjectTerminals {
    groups: HashMap<String, Group>,
    next_id: u64,
}

/// 全局句柄:任意窗口的右侧面板都从这里拿到同一个终端集合。
pub struct TerminalHub(pub Entity<ProjectTerminals>);
impl Global for TerminalHub {}

/// 关闭一个标签后该选中谁:关的不是当前标签就不变;关的是当前标签则选左边相邻的,
/// 左边没有了就回到「Agent 命令」。
///
/// 参数:`ids` 关闭前的标签 id(按顺序);`closing` 要关的标签;`active` 关闭前的选中项。
pub fn selection_after_close(ids: &[u64], closing: u64, active: TermSel) -> TermSel {
    if active != TermSel::Term(closing) {
        return active;
    }
    match ids.iter().position(|&i| i == closing) {
        Some(ix) if ix > 0 => TermSel::Term(ids[ix - 1]),
        _ => TermSel::Agent,
    }
}

impl ProjectTerminals {
    /// 取全局终端集合,没有就创建。
    pub fn hub(cx: &mut App) -> Entity<Self> {
        if let Some(h) = cx.try_global::<TerminalHub>() {
            return h.0.clone();
        }
        let e = cx.new(|_| Self::default());
        cx.set_global(TerminalHub(e.clone()));
        e
    }

    /// 某个项目的终端标签。
    pub fn tabs(&self, root: &str) -> &[TermTab] {
        self.groups.get(root).map_or(&[], |g| &g.tabs)
    }

    /// 某个项目当前选中的子标签。
    pub fn active(&self, root: &str) -> TermSel {
        self.groups.get(root).map(|g| g.active).unwrap_or_default()
    }

    /// 在项目目录里新开一个终端并选中它。
    ///
    /// 参数:`root` 项目根目录(也是终端的工作目录);`command` 要运行的命令,为空启动登录 shell。
    /// 返回:新终端的 id;启动失败返回原因。
    pub fn spawn(&mut self, root: &str, command: Option<Vec<String>>, cx: &mut Context<Self>) -> Result<u64, String> {
        let view = TerminalView::start(root, command, cx)?;
        self.next_id += 1;
        let id = self.next_id;
        let g = self.groups.entry(root.to_string()).or_default();
        g.counter += 1;
        // 终端有输出 / 改标题时,面板的标签条也要跟着重绘
        let sub = cx.observe(&view, |_, _, cx| cx.notify());
        g.tabs.push(TermTab { id, name: format!("终端 {}", g.counter), view, _sub: sub });
        g.active = TermSel::Term(id);
        cx.notify();
        Ok(id)
    }

    /// 选中一个子标签。
    pub fn activate(&mut self, root: &str, sel: TermSel, cx: &mut Context<Self>) {
        if let Some(g) = self.groups.get_mut(root) {
            g.active = sel;
            cx.notify();
        }
    }

    /// 关闭一个终端(丢弃视图会结束它的 shell 进程)。
    pub fn close(&mut self, root: &str, id: u64, cx: &mut Context<Self>) {
        let Some(g) = self.groups.get_mut(root) else { return };
        let ids: Vec<u64> = g.tabs.iter().map(|t| t.id).collect();
        g.active = selection_after_close(&ids, id, g.active);
        g.tabs.retain(|t| t.id != id);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 关闭非当前标签选中项不变() {
        assert_eq!(selection_after_close(&[1, 2, 3], 1, TermSel::Term(3)), TermSel::Term(3));
        assert_eq!(selection_after_close(&[1, 2], 2, TermSel::Agent), TermSel::Agent);
    }

    #[::core::prelude::v1::test]
    fn 关闭当前标签选左邻否则回到_agent() {
        assert_eq!(selection_after_close(&[1, 2, 3], 3, TermSel::Term(3)), TermSel::Term(2));
        assert_eq!(selection_after_close(&[1, 2, 3], 2, TermSel::Term(2)), TermSel::Term(1));
        assert_eq!(selection_after_close(&[1, 2], 1, TermSel::Term(1)), TermSel::Agent);
        // 不存在的 id:保守回到 Agent
        assert_eq!(selection_after_close(&[1], 9, TermSel::Term(9)), TermSel::Agent);
    }
}
