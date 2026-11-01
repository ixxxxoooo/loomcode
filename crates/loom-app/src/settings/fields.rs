//! 设置页的单行文本字段:统一创建、按「分类 + 配置层 + 项目」切换时从配置预填、读取当前值。
//!
//! @author ixxxxoooo

use super::SettingsTab;
use crate::workbench::Workbench;
use gpui_kit::component::input::{InputState, TextareaState};
use gpui_kit::*;
use loom_config::Scope;
use std::collections::HashMap;

/// 字段键与占位文字。
pub const FIELDS: &[(&str, &str)] = &[
    ("model", "模型别名或完整 ID,例如 opus、opusplan、claude-opus-5-5"),
    ("thinking", "例如 16000;留空为默认"),
    ("compact", "1–100;留空为默认"),
    ("output_style", "例如 Explanatory、Learning;留空为默认"),
    ("key_helper", "输出 API Key 的脚本路径;留空不使用"),
    ("env_key", "变量名,例如 HTTPS_PROXY"),
    ("env_value", "值(保存后不再显示)"),
    ("mcp_timeout", "毫秒,例如 30000"),
    ("mcp_tool_timeout", "毫秒"),
    ("mcp_output", "tokens,例如 25000"),
    ("claude_path", "留空自动查找"),
    ("proxy", "例如 127.0.0.1:7890"),
    ("timeout", "秒,例如 600"),
    ("accent", "RRGGBB"),
    ("p_name", "显示名称"),
    ("p_url", "Base URL,例如 https://api.deepseek.com/anthropic"),
    ("p_key", "API Key(保存后不再显示,留空保持原值)"),
    ("p_model", "添加模型 ID,回车或点「添加」"),
    ("plugin_id", "plugin@marketplace"),
    ("market_src", "GitHub 仓库 owner/repo、Git URL 或本地路径"),
    ("search", "搜索插件、技能、MCP…"),
];

/// 文本字段集合。
pub struct Fields {
    inputs: HashMap<&'static str, Entity<InputState>>,
    /// 追加系统提示词(多行)
    pub prompt: Entity<TextareaState>,
    /// 上次预填时的(分类, 层, 项目);变化后重新预填
    synced: Option<(SettingsTab, Scope, Option<String>)>,
}

impl Fields {
    /// 创建全部字段。
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        let inputs = FIELDS.iter().map(|(k, p)| (*k, cx.new(|cx| InputState::new(window, cx).placeholder(*p)))).collect();
        let prompt = cx.new(|cx| TextareaState::new(window, cx).placeholder("每个新会话都会追加到 Claude Code 系统提示词末尾,例如:始终用中文回答"));
        Self { inputs, prompt, synced: None }
    }

    /// 取字段。键不存在属于编程错误。
    pub fn get(&self, key: &str) -> &Entity<InputState> {
        self.inputs.get(key).unwrap_or_else(|| panic!("未定义的设置字段 {key}"))
    }
}

impl Workbench {
    /// 字段输入框。
    pub fn field(&self, key: &str) -> &Entity<InputState> {
        self.forms().fields.get(key)
    }

    /// 字段当前文本(去掉首尾空白)。
    pub fn field_value(&self, key: &str, cx: &App) -> String {
        self.field(key).read(cx).value().trim().to_string()
    }

    /// 设置字段文本。
    pub fn set_field(&self, key: &str, value: String, window: &mut Window, cx: &mut App) {
        self.field(key).clone().update(cx, |i, cx| i.set_value(value, window, cx));
    }

    /// 切换分类、配置层或项目时,按当前配置预填字段(用户正在编辑的内容不会被每帧覆盖)。
    pub fn sync_fields(&mut self, tab: SettingsTab, window: &mut Window, cx: &mut Context<Self>) {
        let key = (tab, self.effective_scope(), self.claude_cfg_root.clone());
        if self.forms().fields.synced.as_ref() == Some(&key) {
            return;
        }
        self.forms_mut().fields.synced = Some(key);
        self.fill_fields(window, cx);
    }

    /// 写入后强制下次重新预填。
    pub fn invalidate_fields(&mut self) {
        self.forms_mut().fields.synced = None;
    }

    /// 从当前层配置与客户端偏好预填所有字段。
    fn fill_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let l = self.claude_cfg.layer(self.effective_scope());
        let s = self.settings.clone();
        let env = |k: &str| l.env(k).unwrap_or("").to_string();
        let values = [
            ("model", l.model.clone().unwrap_or_default()),
            ("thinking", env("MAX_THINKING_TOKENS")),
            ("compact", env("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE")),
            ("output_style", l.output_style.clone().unwrap_or_default()),
            ("key_helper", l.api_key_helper.clone().unwrap_or_default()),
            ("mcp_timeout", env("MCP_TIMEOUT")),
            ("mcp_tool_timeout", env("MCP_TOOL_TIMEOUT")),
            ("mcp_output", env("MAX_MCP_OUTPUT_TOKENS")),
            ("claude_path", s.claude_path.clone().unwrap_or_default()),
            ("proxy", s.proxy_address.clone()),
            ("timeout", s.request_timeout.map(|t| t.to_string()).unwrap_or_default()),
            ("accent", s.accent.clone().unwrap_or_default()),
        ];
        for (k, v) in values {
            self.set_field(k, v, window, cx);
        }
        // 服务商页:当前选中服务商的名称与地址(密钥从不回填)
        let ui = &self.forms().providers;
        if let Some(p) = ui.selected.as_ref().and_then(|id| ui.store.get(id)).cloned() {
            self.set_field("p_name", p.name, window, cx);
            self.set_field("p_url", p.base_url, window, cx);
        }
        let prompt = self.forms().fields.prompt.clone();
        prompt.update(cx, |i, cx| i.set_value(s.append_prompt.clone(), window, cx));
    }
}

/// 把家目录缩写成 `~` 的路径文字。
pub fn tilde(path: &std::path::Path) -> String {
    loom_config::diag::redact_home(&path.display().to_string())
}
