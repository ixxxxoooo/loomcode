//! 侧栏的会话操作:置顶、归档、取消归档,并把选择落盘到 `state.json`。
//!
//! @author ixxxxoooo

use crate::workbench::Workbench;
use gpui_kit::*;

impl Workbench {
    /// 把 `ui_state`(置顶 / 归档 / 自定义名)合并进所有项目的会话列表。
    pub fn apply_ui_state(&mut self) {
        for p in self.projects.iter_mut() {
            self.ui_state.apply(&mut p.sessions);
        }
    }

    /// 保存 `state.json`;演示模式也写(落在沙盒目录里)。
    fn persist_ui_state(&self) {
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

    /// 切换「显示已归档会话」。
    pub fn toggle_show_archived(&mut self, cx: &mut Context<Self>) {
        self.show_archived = !self.show_archived;
        cx.notify();
    }
}
