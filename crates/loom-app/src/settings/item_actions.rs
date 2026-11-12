//! 命令 / 子 Agent / Skills 行上的小操作:在对话中使用、在 Finder 中显示。
//!
//! @author ixxxxoooo

use crate::workbench::Workbench;
use gpui_kit::*;
use loom_config::items::{Item, ItemKind};
use std::path::Path;

/// 点「在对话中使用」时插入输入框的文字:命令与 Skill 是 `/名称 `,子 Agent 是 `@名称 `。
pub fn chat_snippet(item: &Item) -> String {
    if item.kind == ItemKind::Agent { format!("@{} ", item.name) } else { format!("/{} ", item.name) }
}

impl Workbench {
    /// 回到对话并把文字插入当前会话的输入框;没有打开的会话时先新建一个。
    pub fn use_in_chat(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.active.as_ref().and_then(|id| self.views.get(id)).is_none() {
            self.new_session(self.target_project(), window, cx);
        }
        self.close_settings(window, cx);
        if let Some(composer) = self.active.as_ref().and_then(|id| self.views.get(id)).map(|v| v.composer.clone()) {
            composer.update(cx, |c, cx| c.insert_text(&text, window, cx));
        }
    }
}

/// 在 Finder 中显示文件。
pub fn reveal(path: &Path) {
    let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_config::Scope;

    #[::core::prelude::v1::test]
    fn 插入文字区分类型() {
        let mk = |kind, name: &str| Item { kind, name: name.into(), description: String::new(), scope: Scope::User, path: Default::default(), user_invocable: true };
        assert_eq!(chat_snippet(&mk(ItemKind::Command, "review")), "/review ");
        assert_eq!(chat_snippet(&mk(ItemKind::Skill, "ios")), "/ios ");
        assert_eq!(chat_snippet(&mk(ItemKind::Agent, "code-reviewer")), "@code-reviewer ");
    }
}
