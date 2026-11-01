//! 钩子页的 Workbench 方法:打开表单(新增 / 编辑)、保存、删除。
//!
//! @author ixxxxoooo

use crate::workbench::Workbench;
use gpui_kit::*;
use loom_config::HookEntry;
use loom_config::claude::HOOK_EVENTS;

impl Workbench {
    /// 打开钩子表单。
    ///
    /// 参数:`hook` 要编辑的钩子(新增时为 `None`)。
    pub fn open_hook_form(&mut self, hook: Option<&HookEntry>, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(h) = hook {
            self.cfg_scope = h.scope;
        }
        let f = &mut self.forms_mut().hook;
        f.open = true;
        f.editing = hook.map(|h| (h.scope, h.event.clone(), h.matcher.clone(), h.command.clone()));
        f.event = hook.and_then(|h| HOOK_EVENTS.iter().copied().find(|e| *e == h.event)).unwrap_or("PreToolUse");
        let values = [
            (f.matcher.clone(), hook.map(|h| h.matcher.clone()).unwrap_or_default()),
            (f.command.clone(), hook.map(|h| h.command.clone()).unwrap_or_default()),
            (f.timeout.clone(), hook.and_then(|h| h.timeout).map(|t| t.to_string()).unwrap_or_default()),
        ];
        for (input, v) in values {
            input.update(cx, |i, cx| i.set_value(v, window, cx));
        }
        self.forms_mut().error = None;
        cx.notify();
    }

    /// 收起钩子表单。
    pub fn close_hook_form(&mut self, cx: &mut Context<Self>) {
        self.forms_mut().hook.open = false;
        cx.notify();
    }

    /// 保存表单:编辑时先删旧钩子再写新钩子(可换层)。
    pub fn save_hook_form(&mut self, cx: &mut Context<Self>) {
        let f = &self.forms().hook;
        let event = f.event;
        let matcher = f.matcher.read(cx).value().trim().to_string();
        let command = f.command.read(cx).value().trim().to_string();
        let timeout_text = f.timeout.read(cx).value().trim().to_string();
        let editing = f.editing.clone();
        let timeout = match timeout_text.parse::<u64>() {
            _ if timeout_text.is_empty() => None,
            Ok(t) if t > 0 => Some(t),
            _ => {
                self.forms_mut().error = Some("超时需要是正整数(秒)".into());
                cx.notify();
                return;
            }
        };
        let Some(target) = self.scope_settings_file(self.effective_scope()) else { return };
        let mut result = loom_config::claude::add_hook(&target, event, &matcher, &command, timeout);
        if let (Ok(()), Some((scope, ev, m, cmd))) = (&result, editing) {
            if let Some(old) = self.scope_settings_file(scope) {
                result = loom_config::claude::remove_hook(&old, &ev, &m, &cmd);
            }
        }
        if self.finish_cfg_write(result, cx) {
            self.close_hook_form(cx);
        }
    }

    /// 删除钩子(写回它所在的层)。
    pub fn delete_hook(&mut self, hook: &HookEntry, cx: &mut Context<Self>) {
        let result = match self.scope_settings_file(hook.scope) {
            Some(path) => loom_config::claude::remove_hook(&path, &hook.event, &hook.matcher, &hook.command),
            None => Ok(()),
        };
        self.finish_cfg_write(result, cx);
    }
}
