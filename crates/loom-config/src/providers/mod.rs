//! 模型服务商与模型映射:让 Claude Code 通过 Anthropic 协议兼容的服务(DeepSeek、智谱、Kimi…)工作。
//!
//! 配置存 `~/.loomcode/providers.json`;API Key 单独存 `~/.loomcode/provider-keys.json`(权限 0600),
//! 界面不回显。启用映射后,新会话启动 claude 时注入 `ANTHROPIC_BASE_URL`、令牌与各模型槽位的环境变量,
//! 不改动 `~/.claude` 里的任何文件;关闭映射即回到 Claude 官方登录。
//!
//! @author ixxxxoooo

mod net;
mod store;

#[cfg(test)]
mod tests;

pub use net::{fetch_models, test_connection};
pub use store::{load_keys, save_keys};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 鉴权头:多数兼容服务用 `Authorization: Bearer`(`ANTHROPIC_AUTH_TOKEN`),Anthropic 官方用 `x-api-key`(`ANTHROPIC_API_KEY`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AuthHeader {
    #[default]
    Bearer,
    XApiKey,
}

/// 一个服务商。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub auth: AuthHeader,
    /// 可用于映射的模型 ID
    pub models: Vec<String>,
}

/// 模型映射:槽位 → 模型 ID(空表示不设置)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ModelMapping {
    pub enabled: bool,
    /// 使用的服务商 id
    pub provider: String,
    /// `ANTHROPIC_MODEL`
    pub default: String,
    /// `ANTHROPIC_DEFAULT_OPUS_MODEL`
    pub opus: String,
    /// `ANTHROPIC_DEFAULT_SONNET_MODEL`
    pub sonnet: String,
    /// `ANTHROPIC_DEFAULT_HAIKU_MODEL`
    pub haiku: String,
    /// `CLAUDE_CODE_SUBAGENT_MODEL`
    pub subagent: String,
}

/// 映射槽位:(字段名, 界面标签, 环境变量)。
pub const SLOTS: &[(&str, &str, &str)] = &[
    ("default", "默认", "ANTHROPIC_MODEL"),
    ("opus", "Opus", "ANTHROPIC_DEFAULT_OPUS_MODEL"),
    ("sonnet", "Sonnet", "ANTHROPIC_DEFAULT_SONNET_MODEL"),
    ("haiku", "Haiku(后台小任务)", "ANTHROPIC_DEFAULT_HAIKU_MODEL"),
    ("subagent", "子 Agent", "CLAUDE_CODE_SUBAGENT_MODEL"),
];

impl ModelMapping {
    /// 取槽位值。
    pub fn slot(&self, name: &str) -> &str {
        match name {
            "default" => &self.default,
            "opus" => &self.opus,
            "sonnet" => &self.sonnet,
            "haiku" => &self.haiku,
            _ => &self.subagent,
        }
    }

    /// 设置槽位值。
    pub fn set_slot(&mut self, name: &str, value: String) {
        match name {
            "default" => self.default = value,
            "opus" => self.opus = value,
            "sonnet" => self.sonnet = value,
            "haiku" => self.haiku = value,
            _ => self.subagent = value,
        }
    }
}

/// 服务商配置文件内容。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ProviderStore {
    pub providers: Vec<Provider>,
    pub mapping: ModelMapping,
}

/// 预设:(名称, Base URL, 鉴权, 示例模型)。模型名会随服务商更新,可用「获取模型列表」刷新。
pub const PRESETS: &[(&str, &str, AuthHeader, &[&str])] = &[
    ("DeepSeek", "https://api.deepseek.com/anthropic", AuthHeader::Bearer, &["deepseek-chat", "deepseek-reasoner"]),
    ("智谱 GLM", "https://open.bigmodel.cn/api/anthropic", AuthHeader::Bearer, &["glm-4.6", "glm-4.5-air"]),
    ("Kimi", "https://api.moonshot.cn/anthropic", AuthHeader::Bearer, &["kimi-k2-turbo-preview"]),
    ("MiniMax", "https://api.minimaxi.com/anthropic", AuthHeader::Bearer, &["MiniMax-M2"]),
    ("阿里百炼 Qwen", "https://dashscope.aliyuncs.com/apps/anthropic", AuthHeader::Bearer, &["qwen3-coder-plus"]),
    ("Anthropic API", "https://api.anthropic.com", AuthHeader::XApiKey, &["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"]),
];

impl ProviderStore {
    /// 按 id 找服务商。
    pub fn get(&self, id: &str) -> Option<&Provider> {
        self.providers.iter().find(|p| p.id == id)
    }

    /// 映射为什么没有生效;生效时为 `Ok`。
    ///
    /// 参数:`keys` 各服务商的 API Key。返回:未生效原因。
    pub fn mapping_status(&self, keys: &HashMap<String, String>) -> Result<(), String> {
        let m = &self.mapping;
        if !m.enabled {
            return Err("未启用,使用 Claude 官方登录".into());
        }
        let p = self.get(&m.provider).ok_or("没有选择服务商")?;
        if p.base_url.trim().is_empty() {
            return Err(format!("{} 没有填写 Base URL", p.name));
        }
        if keys.get(&p.id).is_none_or(|k| k.trim().is_empty()) {
            return Err(format!("{} 没有填写 API Key", p.name));
        }
        if m.default.trim().is_empty() {
            return Err("没有设置「默认」槽位的模型".into());
        }
        Ok(())
    }

    /// 映射生效时注入 claude 进程的环境变量;未生效返回空。
    ///
    /// 参数:`keys` 各服务商的 API Key。返回:(变量名, 值) 列表。
    pub fn mapping_env(&self, keys: &HashMap<String, String>) -> Vec<(String, String)> {
        if self.mapping_status(keys).is_err() {
            return Vec::new();
        }
        let m = &self.mapping;
        let Some(p) = self.get(&m.provider) else { return Vec::new() };
        let key = keys.get(&p.id).cloned().unwrap_or_default();
        let mut env = vec![("ANTHROPIC_BASE_URL".to_string(), p.base_url.trim().trim_end_matches('/').to_string())];
        match p.auth {
            AuthHeader::Bearer => env.push(("ANTHROPIC_AUTH_TOKEN".into(), key)),
            AuthHeader::XApiKey => env.push(("ANTHROPIC_API_KEY".into(), key)),
        }
        for (slot, _, var) in SLOTS {
            let v = m.slot(slot).trim();
            if !v.is_empty() {
                env.push((var.to_string(), v.to_string()));
            }
        }
        env
    }

    /// 新建服务商 id:名称的 ASCII 部分 + 序号,保证不重复。
    pub fn new_id(&self, name: &str) -> String {
        let base: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase();
        let base = if base.is_empty() { "provider".to_string() } else { base };
        let mut id = base.clone();
        let mut n = 2;
        while self.get(&id).is_some() {
            id = format!("{base}-{n}");
            n += 1;
        }
        id
    }
}
