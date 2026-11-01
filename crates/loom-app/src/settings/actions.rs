//! 设置页用到的 Workbench 方法:按当前项目加载 Claude 配置、配置层选择、权限规则与权限层设置。
//!
//! @author ixxxxoooo

use super::Forms;
use crate::workbench::Workbench;
use gpui_kit::*;
use loom_config::{Rule, Scope};
use serde_json::Value;
use std::path::{Path, PathBuf};

impl Workbench {
    /// 首次进入设置页时创建表单输入框。
    pub fn ensure_forms(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.watch_error(cx);
        if self.settings_forms.is_none() {
            self.settings_forms = Some(Forms::new(window, cx));
            self.refresh_items();
            self.load_providers();
            self.refresh_plugins();
            // 设置类文本框:失焦或回车即提交,不再需要「保存」按钮
            use gpui_kit::component::input::InputEvent;
            for key in super::layer_actions::CLAUDE_FIELDS.iter().map(|(k, _)| *k).chain(super::layer_actions::CLIENT_FIELDS.iter().copied()) {
                let field = self.field(key).clone();
                cx.subscribe(&field, move |this, _, ev: &InputEvent, cx| {
                    if matches!(ev, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                        this.commit_field(key, cx);
                    }
                })
                .detach();
            }
            // 设置导航搜索框:输入时重绘导航
            let nav_search = self.field("settings_search").clone();
            cx.subscribe(&nav_search, |_, _, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Change) {
                    cx.notify();
                }
            })
            .detach();
            // 搜索框每次输入都要重绘列表
            let search = self.field("search").clone();
            cx.subscribe(&search, |_, _, ev: &gpui_kit::component::input::InputEvent, cx| {
                if matches!(ev, gpui_kit::component::input::InputEvent::Change) {
                    cx.notify();
                }
            })
            .detach();
        }
    }

    /// 提示条 4 秒后自动消失:每次渲染调用,发现新的一条就排一个定时清除(只清同一条,不误伤后来的)。
    pub fn watch_error(&mut self, cx: &mut Context<Self>) {
        let Some(forms) = self.settings_forms.as_mut() else { return };
        let Some(err) = forms.error.clone() else {
            forms.error_seen = None;
            return;
        };
        if forms.error_seen.as_ref() == Some(&err) {
            return;
        }
        forms.error_seen = Some(err.clone());
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_secs(4)).await;
            let _ = this.update(cx, |this, cx| {
                if let Some(f) = this.settings_forms.as_mut().filter(|f| f.error.as_ref() == Some(&err)) {
                    f.error = None;
                    f.error_seen = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// 表单状态(调用前已由 `render` 创建)。
    pub fn forms(&self) -> &Forms {
        self.settings_forms.as_ref().expect("设置页表单尚未创建")
    }

    /// 表单状态(可变)。
    pub fn forms_mut(&mut self) -> &mut Forms {
        self.settings_forms.as_mut().expect("设置页表单尚未创建")
    }

    /// 当前项目变化时重读 Claude 配置。
    pub fn ensure_claude_cfg(&mut self) {
        if self.active_project_path() != self.claude_cfg_root {
            self.reload_claude_cfg();
        }
    }

    /// 按当前项目重读用户 / 项目 / 本地三层 Claude 配置。
    pub fn reload_claude_cfg(&mut self) {
        let root = self.active_project_path();
        self.claude_cfg = loom_config::claude::load(&self.claude_home, root.as_deref().map(Path::new));
        self.claude_cfg_root = root;
        self.refresh_items();
    }

    /// 当前项目根(没有打开项目时为 `None`)。
    pub fn cfg_project(&self) -> Option<&Path> {
        self.claude_cfg_root.as_deref().map(Path::new)
    }

    /// 实际生效的配置层:没有打开项目时只能用用户层。
    pub fn effective_scope(&self) -> Scope {
        if self.cfg_scope.needs_project() && self.claude_cfg_root.is_none() { Scope::User } else { self.cfg_scope }
    }

    /// 某一层 `settings.json` 的路径。
    ///
    /// 参数:`scope` 层。返回:路径;项目层 / 本地层但没有项目时为 `None`。
    pub fn scope_settings_file(&self, scope: Scope) -> Option<PathBuf> {
        loom_config::claude::scope_file(&self.claude_home, self.cfg_project(), scope)
    }

    /// 写配置后的统一收尾:失败记到页顶提示,成功清除提示;然后重读配置并刷新。
    ///
    /// 参数:`result` 写入结果。返回:是否成功。
    pub fn finish_cfg_write(&mut self, result: std::io::Result<()>, cx: &mut Context<Self>) -> bool {
        let ok = result.is_ok();
        if let Err(e) = &result {
            loom_config::diag::error(&format!("写入 Claude 配置失败:{e}"));
        }
        self.forms_mut().error = result.err().map(|e| format!("写入失败:{e}"));
        self.reload_claude_cfg();
        cx.notify();
        ok
    }

    /// 在当前配置层的 `settings.json` 上执行一次写入。
    ///
    /// 参数:`f` 拿到文件路径后执行的写入。
    pub fn write_scope_settings(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&Path) -> std::io::Result<()>) {
        let result = match self.scope_settings_file(self.effective_scope()) {
            Some(path) => f(&path),
            None => Err(std::io::Error::other("先打开一个项目会话")),
        };
        self.finish_cfg_write(result, cx);
    }

    /// 设置当前层的 `permissions.defaultMode`;`None` 表示删除(继承其它层)。
    pub fn set_default_mode(&mut self, mode: Option<&'static str>, cx: &mut Context<Self>) {
        let v = mode.map(|m| Value::String(m.into()));
        self.write_scope_settings(cx, |p| loom_config::claude::set_value(p, &["permissions", "defaultMode"], v));
    }

    /// 设置当前层的「禁止绕过权限模式」。
    pub fn set_disable_bypass(&mut self, on: bool, cx: &mut Context<Self>) {
        let v = on.then(|| Value::String("disable".into()));
        self.write_scope_settings(cx, |p| loom_config::claude::set_value(p, &["permissions", "disableBypassPermissionsMode"], v));
    }

    /// 设置当前层的 `sandbox.enabled`。
    pub fn set_sandbox(&mut self, on: bool, cx: &mut Context<Self>) {
        self.write_scope_settings(cx, |p| loom_config::claude::set_value(p, &["sandbox", "enabled"], Some(Value::Bool(on))));
    }

    /// 把输入框里的目录加到当前层的 `permissions.additionalDirectories`。
    pub fn add_additional_dir(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dir = self.forms().dir.read(cx).value().trim().to_string();
        if dir.is_empty() {
            return;
        }
        self.write_scope_settings(cx, |p| loom_config::claude::set_string_list(p, &["permissions", "additionalDirectories"], &dir, true));
        self.forms().dir.clone().update(cx, |i, cx| i.set_value(String::new(), window, cx));
    }

    /// 从当前层移除一个额外目录。
    pub fn remove_additional_dir(&mut self, dir: &str, cx: &mut Context<Self>) {
        self.write_scope_settings(cx, |p| loom_config::claude::set_string_list(p, &["permissions", "additionalDirectories"], dir, false));
    }

    /// 把输入框里的规则加到当前配置层。
    ///
    /// 参数:`kind` 为 `allow` / `ask` / `deny`。
    pub fn add_rule(&mut self, kind: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.forms().rule.read(cx).value().trim().to_string();
        if text.is_empty() {
            return;
        }
        self.write_scope_settings(cx, |p| loom_config::claude::add_permission_rule(p, kind, &text));
        if self.forms().error.is_none() {
            self.forms().rule.clone().update(cx, |i, cx| i.set_value(String::new(), window, cx));
        }
    }

    /// 删除一条规则:写回它所在的层文件。
    ///
    /// 参数:`kind` 种类;`rule` 规则及其来源层。
    pub fn remove_rule(&mut self, kind: &str, rule: &Rule, cx: &mut Context<Self>) {
        let result = match self.scope_settings_file(rule.scope) {
            Some(path) => loom_config::claude::remove_permission_rule(&path, kind, &rule.text),
            None => Ok(()),
        };
        self.finish_cfg_write(result, cx);
    }
}
