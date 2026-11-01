//! 会话 Fork:从一个历史会话的当前状态分叉出一个新会话,原会话不受影响。
//!
//! 新会话先在界面里回放源会话的历史;用户第一次发消息时才启动 claude,
//! 参数是 `--resume 源 --fork-session --session-id 新`,之后它就是一个普通会话。
//!
//! @author ixxxxoooo

use crate::workbench::Workbench;
use gpui_kit::*;
use loom_core::{SessionBadge, SessionMeta};

impl Workbench {
    /// 分叉一个会话并打开新会话。演示模式没有真实历史,忽略。
    ///
    /// 参数:`src_id` 源会话 id;`cwd` 它所属项目的目录。
    pub fn fork_session(&mut self, src_id: &str, cwd: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.demo {
            return;
        }
        let Some(pi) = self.projects.iter().position(|p| p.path == cwd) else { return };
        let src_title = self.projects[pi].sessions.iter().find(|s| s.id == src_id).map(|s| s.title.clone()).unwrap_or_else(|| "会话".into());
        let new_id = uuid::Uuid::new_v4().to_string();
        let title = format!("{src_title} · 分支");
        self.forks.insert(new_id.clone(), src_id.to_string());
        // 分叉出的会话在磁盘上要等发了第一条消息才出现,先在侧栏里占位;名字存进 state.json,重启后仍对得上
        self.ui_state.rename(&new_id, &title);
        self.persist_ui_state();
        let p = &mut self.projects[pi];
        p.expanded = true;
        p.sessions.insert(0, SessionMeta { id: new_id.clone(), title, updated: "刚刚".into(), pinned: false, archived: false, badge: SessionBadge::Idle });
        self.open_session(pi, &new_id, window, cx);
        if let Some(v) = self.views.get(&new_id) {
            v.composer.update(cx, |c, cx| c.focus(window, cx));
        }
    }
}
