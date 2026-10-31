//! 侧栏的会话操作:置顶、归档、取消归档,并把选择落盘到 `state.json`。
//!
//! @author ixxxxoooo

use crate::workbench::Workbench;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::ButtonVariant;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::*;
use std::cell::Cell;
use ucode_core::AgentCommand;

impl Workbench {
    /// 把 `ui_state`(置顶 / 归档 / 自定义名)合并进所有项目的会话列表。
    pub fn apply_ui_state(&mut self) {
        for p in self.projects.iter_mut() {
            self.ui_state.apply(&mut p.sessions);
            if let Some(name) = self.ui_state.project_names.get(&p.path) {
                p.name = name.clone();
            }
        }
    }

    /// 保存 `state.json`;演示模式也写(落在沙盒目录里)。
    pub fn persist_ui_state(&self) {
        if let Err(e) = self.ui_state.save(&self.ui_state_path) {
            eprintln!("保存界面状态失败:{e}");
        }
    }

    /// 切换会话置顶。
    ///
    /// 参数:`id` 会话 id。
    pub fn toggle_pin_session(&mut self, id: &str, cx: &mut Context<Self>) {
        self.ui_state.toggle_pin(id);
        self.apply_ui_state();
        self.persist_ui_state();
        cx.notify();
    }

    /// 归档或取消归档会话。
    ///
    /// 参数:`id` 会话 id;`archived` 为真表示归档。
    pub fn set_session_archived(&mut self, id: &str, archived: bool, cx: &mut Context<Self>) {
        self.ui_state.set_archived(id, archived);
        self.apply_ui_state();
        self.persist_ui_state();
        cx.notify();
    }

    /// 回收空闲会话:已打开的会话超过上限时,结束最久未用的空闲会话的 claude 进程。
    ///
    /// 运行中、等待用户、有未发送草稿、还没有内容的会话不会被回收;演示模式不回收。
    /// 被回收的会话下次点开时会重新回放历史,发送消息时再 `--resume`。
    pub fn evict_idle_views(&mut self, cx: &mut Context<Self>) {
        if self.demo {
            return;
        }
        let busy = |id: &str| {
            self.views.get(id).is_none_or(|v| {
                let st = &v.model.read(cx).state;
                st.phase == ucode_core::Phase::Running || st.awaiting_user() || st.items.is_empty() || v.composer.read(cx).has_draft(cx)
            })
        };
        let victims = crate::view_cache::pick_evictions(&self.view_order, self.active.as_deref(), busy, crate::view_cache::MAX_OPEN_VIEWS);
        for id in victims {
            if let Some(v) = self.views.remove(&id) {
                v.model.update(cx, |m, _| m.handle.send(AgentCommand::Shutdown));
            }
            // 已写进历史的新会话,之后要走 --resume 而不是 --session-id
            self.new_sessions.remove(&id);
            self.view_order.retain(|x| x != &id);
        }
    }

    /// 切换「显示已归档会话」。
    pub fn toggle_show_archived(&mut self, cx: &mut Context<Self>) {
        self.show_archived = !self.show_archived;
        cx.notify();
    }

    /// 重命名会话:自定义名只存在 `state.json`,不改 claude 的历史文件;空白表示恢复原标题。
    ///
    /// 参数:`id` 会话 id;`name` 新名称。
    pub fn rename_session(&mut self, id: &str, name: &str, cx: &mut Context<Self>) {
        self.ui_state.rename(id, name);
        self.apply_ui_state();
        self.persist_ui_state();
        cx.notify();
    }

    /// 弹出重命名对话框,输入框预填当前标题。
    pub fn rename_session_dialog(&mut self, id: &str, title: &str, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("会话名称(留空恢复原标题)").default_value(title.to_string()));
        let (wb, id) = (cx.entity().downgrade(), id.to_string());
        // 只在第一次构建时聚焦,避免对话框重建时抢走焦点
        let first = Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false) {
                input.focus_handle(cx).focus(window, cx);
            }
            let (wb, id, input) = (wb.clone(), id.clone(), input.clone());
            dialog.title("重命名会话").child(Input::new(&input.clone())).footer(crate::ui::dialog_footer("保存")).on_ok(move |_, _, cx| {
                let name = input.read(cx).value().to_string();
                let _ = wb.update(cx, |this, cx| this.rename_session(&id, &name, cx));
                true
            })
        });
    }

    /// 调试用:按 `UCODE_DIALOG` 打开对话框。`rename` 重命名当前会话,`workspace` 新建工作区。
    pub fn debug_open_dialog(&mut self, kind: &str, window: &mut Window, cx: &mut Context<Self>) {
        if kind == "workspace" {
            return self.new_workspace_dialog(window, cx);
        }
        let Some(id) = self.active.clone() else { return };
        let title = self.projects.iter().flat_map(|p| p.sessions.iter()).find(|s| s.id == id).map(|s| s.title.clone()).unwrap_or_default();
        self.rename_session_dialog(&id, &title, window, cx);
    }

    /// 弹出删除确认。运行中的会话额外提示删除会中断它。
    ///
    /// 参数:`id` 会话 id;`cwd` 所属项目目录;`title` 用于提示。
    pub fn delete_session_dialog(&mut self, id: &str, cwd: &str, title: &str, window: &mut Window, cx: &mut Context<Self>) {
        let running = self.views.get(id).is_some_and(|v| v.model.read(cx).state.phase == ucode_core::Phase::Running);
        let (wb, id, cwd, title) = (cx.entity().downgrade(), id.to_string(), cwd.to_string(), title.to_string());
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (wb, id, cwd) = (wb.clone(), id.clone(), cwd.clone());
            let tip = if running { "该会话正在运行,删除会中断它。" } else { "" };
            alert
                .confirm()
                .title("删除会话?")
                .description(format!("「{title}」的历史记录会被永久删除,无法恢复。{tip}"))
                .ok_text("删除")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, _, cx| {
                    let _ = wb.update(cx, |this, cx| this.delete_session(&id, &cwd, cx));
                    true
                })
        });
    }

    /// 删除会话:结束后端进程、移出侧栏、清理记录,真实模式下删除历史文件。
    ///
    /// 参数:`id` 会话 id;`cwd` 所属项目目录(定位历史文件)。
    pub fn delete_session(&mut self, id: &str, cwd: &str, cx: &mut Context<Self>) {
        if let Some(v) = self.views.remove(id) {
            v.model.update(cx, |m, _| m.handle.send(AgentCommand::Shutdown));
        }
        if self.active.as_deref() == Some(id) {
            self.active = None;
        }
        self.unread.remove(id);
        self.new_sessions.remove(id);
        self.view_order.retain(|x| x != id);
        if !self.demo {
            if let Err(e) = ucode_claude::history::delete_session(cwd, id) {
                eprintln!("删除会话文件失败:{e}");
            }
        }
        for p in self.projects.iter_mut() {
            p.sessions.retain(|s| s.id != id);
        }
        self.ui_state.forget_session(id);
        self.persist_ui_state();
        cx.notify();
    }
}
