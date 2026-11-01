//! 设置页用到的 Workbench 方法:按当前项目加载 Claude 配置、按来源层增删权限规则、编辑 CLAUDE.md。
//!
//! @author ixxxxoooo

use super::MemoryDoc;
use crate::workbench::Workbench;
use gpui_kit::component::input::{EditorState, InputEvent, InputState};
use gpui_kit::*;
use loom_config::{Rule, Scope};
use std::path::{Path, PathBuf};

/// 家目录(读取 `~/.claude*` 用)。
pub fn claude_home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

/// 供 Workbench 创建规则输入框状态用。
pub fn new_rule_input(window: &mut Window, cx: &mut App) -> Entity<InputState> {
    cx.new(|cx| InputState::new(window, cx).placeholder("输入规则,再点右侧按钮添加"))
}

impl Workbench {
    /// 当前项目变化时重读 Claude 配置(演示模式不读磁盘)。
    pub fn ensure_claude_cfg(&mut self) {
        if !self.demo && self.active_project_path() != self.claude_cfg_root {
            self.reload_claude_cfg();
        }
    }

    /// 按当前项目重读用户 / 项目 / 本地三层 Claude 配置。
    pub fn reload_claude_cfg(&mut self) {
        if self.demo {
            return;
        }
        let root = self.active_project_path();
        self.claude_cfg = loom_config::claude::load(&claude_home(), root.as_deref().map(Path::new));
        self.claude_cfg_root = root;
    }

    /// 实际生效的写入层:没有打开项目时只能写用户层。
    pub fn effective_rule_scope(&self) -> Scope {
        if self.rule_scope.needs_project() && !self.demo && self.claude_cfg_root.is_none() { Scope::User } else { self.rule_scope }
    }

    /// 某一层 `settings.json` 的路径。
    ///
    /// 参数:`scope` 层。返回:路径;项目层 / 本地层但没有项目时为 `None`。
    pub fn rule_scope_file(&self, scope: Scope) -> Option<PathBuf> {
        loom_config::claude::scope_file(&claude_home(), self.claude_cfg_root.as_deref().map(Path::new), scope)
    }

    /// 把输入框里的规则加到当前写入层(演示模式只改内存)。
    ///
    /// 参数:`kind` 为 `allow` / `ask` / `deny`。
    pub fn add_rule(&mut self, kind: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.rule_input.read(cx).value().trim().to_string();
        if text.is_empty() {
            return;
        }
        let scope = self.effective_rule_scope();
        if self.demo {
            let rule = Rule { text, scope };
            let list = self.claude_cfg.permissions.list_mut(kind);
            if !list.contains(&rule) {
                list.push(rule);
            }
        } else {
            let Some(path) = self.rule_scope_file(scope) else { return };
            if let Err(e) = loom_config::claude::add_permission_rule(&path, kind, &text) {
                eprintln!("写入权限规则失败:{e}");
                return;
            }
            self.reload_claude_cfg();
        }
        self.rule_input.update(cx, |i, cx| i.set_value(String::new(), window, cx));
        cx.notify();
    }

    /// 删除一条规则:写回它所在的层文件(演示模式只改内存)。
    ///
    /// 参数:`kind` 种类;`rule` 规则及其来源层。
    pub fn remove_rule(&mut self, kind: &str, rule: &Rule, cx: &mut Context<Self>) {
        if self.demo {
            self.claude_cfg.permissions.list_mut(kind).retain(|r| r != rule);
        } else {
            let Some(path) = self.rule_scope_file(rule.scope) else { return };
            if let Err(e) = loom_config::claude::remove_permission_rule(&path, kind, &rule.text) {
                eprintln!("删除权限规则失败:{e}");
            }
            self.reload_claude_cfg();
        }
        cx.notify();
    }

    /// 确保记忆页的编辑器对应当前项目的 CLAUDE.md。
    pub fn ensure_memory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.active_project_path() else { return };
        let path = PathBuf::from(project).join("CLAUDE.md");
        if self.memory.as_ref().is_some_and(|m| m.path == path) {
            return;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let editor = cx.new(|cx| EditorState::new(window, cx).language("markdown").default_value(text));
        // 编辑后标记为未保存
        cx.subscribe(&editor, |this, _, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                if let Some(m) = this.memory.as_mut() {
                    m.saved = false;
                    cx.notify();
                }
            }
        })
        .detach();
        self.memory = Some(MemoryDoc { path, editor, saved: true });
    }

    /// 保存记忆文件。
    pub fn save_memory(&mut self, cx: &mut Context<Self>) {
        if let Some(doc) = self.memory.as_mut() {
            let text = doc.editor.read(cx).value().to_string();
            match std::fs::write(&doc.path, text) {
                Ok(()) => doc.saved = true,
                Err(e) => eprintln!("保存 CLAUDE.md 失败:{e}"),
            }
            cx.notify();
        }
    }
}
