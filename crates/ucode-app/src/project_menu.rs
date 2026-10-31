//! 项目行的右键菜单:置顶、重命名、全部标为已读、归档全部、移除。
//!
//! @author ixxxxoooo

use crate::workbench::Workbench;
use gpui_kit::assets::IconName;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::*;

/// 构造项目右键菜单的构建函数。
///
/// 参数:`wb` 工作台弱引用;`path` 项目目录;`name` 当前显示名;`pinned` 是否已置顶;
/// `is_workspace` 为真时多一项「编辑工作区…」。
pub fn build(wb: WeakEntity<Workbench>, path: String, name: String, pinned: bool, is_workspace: bool) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu, _, _| {
        let item = |label: &'static str, icon: IconName| PopupMenuItem::new(label).icon(icon);
        let (w, p) = (wb.clone(), path.clone());
        let pin = item(if pinned { "取消置顶" } else { "置顶" }, IconName::Pin).on_click(move |_, _, cx| {
            let _ = w.update(cx, |this, cx| this.toggle_pin_project(&p, cx));
        });
        let (w, p, n) = (wb.clone(), path.clone(), name.clone());
        let rename = item("重命名…", IconName::Pencil).on_click(move |_, window, cx| {
            let _ = w.update(cx, |this, cx| this.rename_project_dialog(&p, &n, window, cx));
        });
        let (w, p) = (wb.clone(), path.clone());
        let read = item("全部标为已读", IconName::Check).on_click(move |_, _, cx| {
            let _ = w.update(cx, |this, cx| this.mark_project_read(&p, cx));
        });
        let (w, p) = (wb.clone(), path.clone());
        let archive = item("归档全部会话", IconName::Archive).on_click(move |_, _, cx| {
            let _ = w.update(cx, |this, cx| this.archive_project_sessions(&p, cx));
        });
        let (w, p) = (wb.clone(), path.clone());
        let remove = item("从侧栏移除", IconName::Trash).on_click(move |_, _, cx| {
            let _ = w.update(cx, |this, cx| this.remove_project(&p, cx));
        });
        let (w, p) = (wb.clone(), path.clone());
        let edit = item("编辑工作区…", IconName::Folders).on_click(move |_, window, cx| {
            let _ = w.update(cx, |this, cx| this.edit_workspace_dialog(&p, window, cx));
        });
        let menu = if is_workspace { menu.item(edit) } else { menu };
        menu.item(pin).item(rename).separator().item(read).item(archive).separator().item(remove)
    }
}
