//! 模型 / 环境变量 / 高级页的 Workbench 方法:把文本字段与开关写入当前配置层,
//! 以及 claude 路径、追加提示词、恢复默认等客户端偏好。
//!
//! @author ixxxxoooo

use crate::workbench::Workbench;
use gpui_kit::*;
use loom_config::AppSettings;
use serde_json::Value;

/// 文本字段写到哪里。
#[derive(Clone, Copy)]
pub enum Target {
    /// settings.json 里的字符串键路径
    Json(&'static [&'static str]),
    /// `env` 里的变量;`numeric` 为真时要求正整数
    Env(&'static str, bool),
}

/// 写入 Claude 配置层的文本字段(失焦 / 回车自动提交)。
pub const CLAUDE_FIELDS: &[(&str, Target)] = &[
    ("model", Target::Json(&["model"])),
    ("thinking", Target::Env("MAX_THINKING_TOKENS", true)),
    ("compact", Target::Env("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE", true)),
    ("output_style", Target::Json(&["outputStyle"])),
    ("key_helper", Target::Json(&["apiKeyHelper"])),
    ("mcp_timeout", Target::Env("MCP_TIMEOUT", true)),
    ("mcp_tool_timeout", Target::Env("MCP_TOOL_TIMEOUT", true)),
    ("mcp_output", Target::Env("MAX_MCP_OUTPUT_TOKENS", true)),
];

/// 写入客户端偏好(settings.json)的文本字段(失焦 / 回车自动提交)。
pub const CLIENT_FIELDS: &[&str] = &["proxy", "timeout", "no_proxy", "claude_path", "accent", "term_shell", "browser_ua", "ignore"];

impl Workbench {
    /// 输入框失焦或回车时提交:内容没变就什么都不做;客户端偏好走 `update_settings`,Claude 配置写当前层。
    ///
    /// 参数:`key` 字段键。
    pub fn commit_field(&mut self, key: &str, cx: &mut Context<Self>) {
        let text = self.field_value(key, cx);
        if self.forms().fields.baseline.get(key).is_some_and(|b| *b == text) {
            return;
        }
        if let Some((_, target)) = CLAUDE_FIELDS.iter().find(|(k, _)| *k == key) {
            self.save_field(key, *target, cx);
        } else {
            match key {
                "proxy" => self.update_settings(cx, |s| s.proxy_address = text.clone()),
                "no_proxy" => self.update_settings(cx, |s| s.no_proxy = text.clone()),
                "timeout" => match text.parse::<u64>() {
                    _ if text.is_empty() => self.update_settings(cx, |s| s.request_timeout = None),
                    Ok(n) if n > 0 => self.update_settings(cx, |s| s.request_timeout = Some(n)),
                    _ => {
                        self.forms_mut().error = Some("超时需要是正整数(秒)".into());
                        cx.notify();
                        return;
                    }
                },
                "claude_path" => self.save_claude_path(cx),
                "term_shell" => self.update_settings(cx, |s| s.terminal.shell = text.trim().to_string()),
                "browser_ua" => self.update_settings(cx, |s| s.browser.user_agent = text.trim().to_string()),
                "ignore" => {
                    let lines = text.split([',', '\n']).map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n");
                    self.update_settings(cx, |s| s.intel.ignore = lines.clone());
                }
                "accent" => {
                    if text.is_empty() || crate::theme::parse_hex(&text).is_some() {
                        self.update_settings(cx, |s| s.accent = (!text.is_empty()).then(|| text.trim_start_matches('#').to_uppercase()));
                    } else {
                        self.forms_mut().error = Some("强调色需要是 6 位十六进制,例如 2563EB".into());
                        cx.notify();
                        return;
                    }
                }
                _ => return,
            }
        }
        if self.forms().error.is_none() {
            self.forms_mut().fields.baseline.insert(key.to_string(), text);
        }
    }

    /// 保存一个文本字段到当前层;空值表示删除该键(继承其它层)。
    ///
    /// 参数:`field` 字段键;`target` 写入位置。
    pub fn save_field(&mut self, field: &str, target: Target, cx: &mut Context<Self>) {
        let text = self.field_value(field, cx);
        let value = (!text.is_empty()).then(|| Value::String(text.clone()));
        match target {
            Target::Json(keys) => self.write_scope_settings(cx, |p| loom_config::claude::set_value(p, keys, value)),
            Target::Env(key, numeric) => {
                if numeric && !text.is_empty() && text.parse::<u64>().map_or(true, |n| n == 0) {
                    self.forms_mut().error = Some(format!("{key} 需要是正整数"));
                    cx.notify();
                    return;
                }
                self.write_scope_settings(cx, |p| loom_config::claude::set_value(p, &["env", key], value));
            }
        }
    }

    /// 当前层的布尔键;`None` 表示删除。
    pub fn set_layer_bool(&mut self, keys: &'static [&'static str], value: Option<bool>, cx: &mut Context<Self>) {
        self.write_scope_settings(cx, |p| loom_config::claude::set_value(p, keys, value.map(Value::Bool)));
    }

    /// 当前层的 env 开关:开为 `"1"`,关为删除。
    pub fn set_env_flag(&mut self, key: &'static str, on: bool, cx: &mut Context<Self>) {
        let v = on.then(|| Value::String("1".into()));
        self.write_scope_settings(cx, |p| loom_config::claude::set_value(p, &["env", key], v));
    }

    /// 当前层的 env 取值;`None` 表示删除。
    pub fn set_env_value(&mut self, key: &'static str, value: Option<&'static str>, cx: &mut Context<Self>) {
        let v = value.map(|s| Value::String(s.into()));
        self.write_scope_settings(cx, |p| loom_config::claude::set_value(p, &["env", key], v));
    }

    /// 用「变量名 / 值」两个字段在当前层新增或覆盖一个环境变量,成功后清空输入。
    pub fn add_env(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let key = self.field_value("env_key", cx);
        let value = self.field("env_value").read(cx).value().to_string();
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || key.starts_with(|c: char| c.is_ascii_digit()) {
            self.forms_mut().error = Some("变量名只能包含字母、数字和下划线,且不能以数字开头".into());
            cx.notify();
            return;
        }
        self.write_scope_settings(cx, |p| loom_config::claude::set_value(p, &["env", &key], Some(Value::String(value))));
        if self.forms().error.is_none() {
            self.set_field("env_key", String::new(), window, cx);
            self.set_field("env_value", String::new(), window, cx);
        }
    }

    /// 从当前层删除一个环境变量。
    pub fn remove_env(&mut self, key: &str, cx: &mut Context<Self>) {
        self.write_scope_settings(cx, |p| loom_config::claude::set_value(p, &["env", key], None));
    }

    /// 保存 claude 路径并重新检测版本。
    pub fn save_claude_path(&mut self, cx: &mut Context<Self>) {
        let path = self.field_value("claude_path", cx);
        if !path.is_empty() && !std::path::Path::new(&path).is_file() {
            self.forms_mut().error = Some(format!("找不到文件:{path}"));
            cx.notify();
            return;
        }
        self.forms_mut().error = None;
        self.update_settings(cx, |s| s.claude_path = (!path.is_empty()).then_some(path));
        self.claude_info = None;
        self.detect_claude(cx);
    }

    /// 保存追加系统提示词(对之后新建或重新连接的会话生效)。
    pub fn save_append_prompt(&mut self, cx: &mut Context<Self>) {
        let text = self.forms().fields.prompt.read(cx).value().trim().to_string();
        self.update_settings(cx, |s| s.append_prompt = text);
    }

    /// 恢复客户端偏好默认值(保留栏宽、最近项目与 claude 路径;不动 Claude 配置)。
    pub fn reset_client_prefs(&mut self, cx: &mut Context<Self>) {
        let keep = self.settings.clone();
        let fused_changed = keep.fused_ide != AppSettings::default().fused_ide;
        self.update_settings(cx, |s| {
            *s = AppSettings { left_width: keep.left_width, right_width: keep.right_width, recent_projects: keep.recent_projects.clone(), claude_path: keep.claude_path.clone(), ..AppSettings::default() };
        });
        crate::app_icon::set_tray_visible(self.settings.tray_icon);
        if fused_changed {
            self.on_fused_changed(cx);
        }
        self.invalidate_fields();
    }
}
