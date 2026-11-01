//! 会话行的右键菜单:置顶、重命名、复制 ID、归档、删除。
//!
//! @author ixxxxoooo

use crate::workbench::Workbench;
use gpui_kit::assets::IconName;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::*;
use loom_core::SessionMeta;

/// 构造会话右键菜单的构建函数(每次打开菜单都会调用,所以内部克隆所需数据)。
///
/// 参数:`wb` 工作台弱引用;`meta` 目标会话;`cwd` 所属项目目录;`demo` 演示模式没有真实历史,Fork 置灰。
pub fn build(wb: WeakEntity<Workbench>, meta: &SessionMeta, cwd: String, demo: bool) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    let (id, title, pinned, archived) = (meta.id.clone(), meta.title.clone(), meta.pinned, meta.archived);
    move |menu, _, _| {
        let item = |label: &'static str, icon: IconName| PopupMenuItem::new(label).icon(icon);
        let (w, i) = (wb.clone(), id.clone());
        let pin = item(if pinned { "取消置顶" } else { "置顶" }, IconName::Pin).on_click(move |_, _, cx| {
            let _ = w.update(cx, |this, cx| this.toggle_pin_session(&i, cx));
        });
        let (w, i, t) = (wb.clone(), id.clone(), title.clone());
        let rename = item("重命名…", IconName::Pencil).on_click(move |_, window, cx| {
            let _ = w.update(cx, |this, cx| this.rename_session_dialog(&i, &t, window, cx));
        });
        let (w, i, c) = (wb.clone(), id.clone(), cwd.clone());
        let fork = item("创建分支会话", IconName::GitFork).disabled(demo).on_click(move |_, window, cx| {
            let _ = w.update(cx, |this, cx| this.fork_session(&i, &c, window, cx));
        });
        let i = id.clone();
        let copy = item("复制会话 ID", IconName::Copy).on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(i.clone())));
        let (w, i) = (wb.clone(), id.clone());
        let archive = item(if archived { "取消归档" } else { "归档" }, IconName::Archive).on_click(move |_, _, cx| {
            let _ = w.update(cx, |this, cx| this.set_session_archived(&i, !archived, cx));
        });
        let (w, i, t, c) = (wb.clone(), id.clone(), title.clone(), cwd.clone());
        let delete = item("删除…", IconName::Trash).on_click(move |_, window, cx| {
            let _ = w.update(cx, |this, cx| this.delete_session_dialog(&i, &c, &t, window, cx));
        });
        menu.item(pin).item(rename).item(fork).item(copy).separator().item(archive).item(delete)
    }
}
