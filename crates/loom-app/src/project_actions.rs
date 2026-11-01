//! 侧栏的项目操作:置顶、重命名显示名、全部标为已读、归档全部、移除项目。
//!
//! @author ixxxxoooo

use crate::workbench::Workbench;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::*;
use std::cell::Cell;
use std::collections::HashMap;
use loom_core::{AgentCommand, project_name_from_path};

impl Workbench {
    /// 已打开会话所在项目的路径(按会话 id)。项目重排前取一份,重排后据此恢复下标。
    pub fn view_owners(&self) -> HashMap<String, String> {
        self.views.iter().filter_map(|(id, v)| Some((id.clone(), self.projects.get(v.project_ix)?.path.clone()))).collect()
    }

    /// 项目列表重排后,按路径恢复已打开会话的 `project_ix`。
    pub fn remap_views(&mut self, owners: &HashMap<String, String>) {
        for (id, v) in self.views.iter_mut() {
            if let Some(ix) = owners.get(id).and_then(|path| self.projects.iter().position(|p| &p.path == path)) {
                v.project_ix = ix;
            }
        }
    }

    /// 切换项目置顶:置顶项目排到最前,并保持已打开会话的归属不乱。
    pub fn toggle_pin_project(&mut self, path: &str, cx: &mut Context<Self>) {
        let owners = self.view_owners();
        self.ui_state.toggle_pin_project(path);
        self.ui_state.apply_projects(&mut self.projects);
        self.remap_views(&owners);
        self.persist_ui_state();
        cx.notify();
    }

    /// 重命名项目显示名(不改目录);空白恢复目录名。
    pub fn rename_project(&mut self, path: &str, name: &str, cx: &mut Context<Self>) {
        self.ui_state.rename_project(path, name);
        if let Some(p) = self.projects.iter_mut().find(|p| p.path == path) {
            p.name = self.ui_state.project_names.get(path).cloned().unwrap_or_else(|| project_name_from_path(path));
        }
        self.persist_ui_state();
        cx.notify();
    }

    /// 弹出项目重命名对话框。
    pub fn rename_project_dialog(&mut self, path: &str, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("项目显示名(留空恢复目录名)").default_value(name.to_string()));
        let (wb, path) = (cx.entity().downgrade(), path.to_string());
        let first = Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false) {
                input.focus_handle(cx).focus(window, cx);
            }
            let (wb, path, input) = (wb.clone(), path.clone(), input.clone());
            dialog.title("重命名项目").child(Input::new(&input.clone())).footer(crate::ui::dialog_footer("保存")).on_ok(move |_, _, cx| {
                let name = input.read(cx).value().to_string();
                let _ = wb.update(cx, |this, cx| this.rename_project(&path, &name, cx));
                true
            })
        });
    }

    /// 把项目下所有会话标为已读。
    pub fn mark_project_read(&mut self, path: &str, cx: &mut Context<Self>) {
        let ids: Vec<String> = self.projects.iter().filter(|p| p.path == path).flat_map(|p| p.sessions.iter().map(|s| s.id.clone())).collect();
        for id in ids {
            self.unread.remove(&id);
        }
        cx.notify();
    }

    /// 归档项目下所有会话(置顶会一并取消)。
    pub fn archive_project_sessions(&mut self, path: &str, cx: &mut Context<Self>) {
        let ids: Vec<String> = self.projects.iter().filter(|p| p.path == path).flat_map(|p| p.sessions.iter().map(|s| s.id.clone())).collect();
        for id in ids {
            self.ui_state.set_archived(&id, true);
        }
        self.apply_ui_state();
        self.persist_ui_state();
        cx.notify();
    }

    /// 从侧栏移除项目:结束它名下已打开会话的进程并隐藏项目,磁盘上的文件和历史都不动。
    pub fn remove_project(&mut self, path: &str, cx: &mut Context<Self>) {
        let doomed: Vec<String> = self.view_owners().into_iter().filter(|(_, p)| p == path).map(|(id, _)| id).collect();
        for id in doomed {
            if let Some(v) = self.views.remove(&id) {
                v.model.update(cx, |m, _| m.handle.send(AgentCommand::Shutdown));
            }
            self.view_order.retain(|x| x != &id);
            if self.active.as_deref() == Some(id.as_str()) {
                self.active = None;
            }
        }
        self.ui_state.hide_project(path);
        self.persist_ui_state();
        cx.notify();
    }
}
