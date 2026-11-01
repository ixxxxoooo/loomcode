//! Claude Code 配置的解析与受控写入:权限、钩子、MCP 服务器、环境变量。
//!
//! 每一项都带「来源层」(用户 / 项目 / 本地),界面据此显示来源并把修改写回对应文件。
//! 子模块:`parse` 纯函数解析;`load` 读盘合并;`write` JSON 受控写入(保留未知字段、原子替换、备份);
//! `mcp_write` / `hooks_write` 各自的增删改。
//!
//! @author ixxxxoooo

mod hooks_write;
mod load;
mod mcp_health;
mod mcp_write;
mod parse;
mod trust;
mod write;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_write;

pub use hooks_write::{HOOK_EVENTS, add_hook, remove_hook};
pub use load::{load, scope_file};
pub use mcp_health::{Health, desktop_config_path, import_from_desktop, parse_mcp_list};
pub use mcp_write::{McpLocation, parse_mcp_paste, remove_mcp, set_mcp_enabled, upsert_mcp, upsert_mcp_raw, validate as validate_mcp};
pub use trust::{read_trust, set_trust};
pub use parse::{merge, parse_mcp_servers, parse_settings};
pub use write::{add_permission_rule, backup_path, edit_json, object_at, remove_permission_rule, set_string_list, set_value};

use serde_json::Value;

/// 配置来源层。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    /// `~/.claude/settings.json`(MCP 为 `~/.claude.json` 顶层)
    User,
    /// `<项目>/.claude/settings.json`(MCP 为 `<项目>/.mcp.json`)
    Project,
    /// `<项目>/.claude/settings.local.json`(MCP 为 `~/.claude.json` 的 `projects[<项目>]`)
    Local,
}

impl Scope {
    pub const ALL: [Scope; 3] = [Scope::User, Scope::Project, Scope::Local];

    /// 界面标签。
    pub fn label(self) -> &'static str {
        match self {
            Scope::User => "用户",
            Scope::Project => "项目",
            Scope::Local => "本地",
        }
    }

    /// 是否依赖已打开的项目。
    pub fn needs_project(self) -> bool {
        !matches!(self, Scope::User)
    }
}

/// 一条权限规则及其来源层。
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub text: String,
    pub scope: Scope,
}

/// 权限规则(allow / deny / ask 三组)。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PermissionRules {
    pub allow: Vec<Rule>,
    pub deny: Vec<Rule>,
    pub ask: Vec<Rule>,
}

impl PermissionRules {
    /// 按种类(`allow` / `deny` / `ask`)取可变列表;未知种类归入 `ask`。
    pub fn list_mut(&mut self, kind: &str) -> &mut Vec<Rule> {
        match kind {
            "allow" => &mut self.allow,
            "deny" => &mut self.deny,
            _ => &mut self.ask,
        }
    }
}

/// 单个 `settings.json` 层里的非列表型设置(权限页、MCP 启停用)。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LayerSettings {
    /// `permissions.defaultMode`
    pub default_mode: Option<String>,
    /// `permissions.additionalDirectories`
    pub additional_dirs: Vec<String>,
    /// `permissions.disableBypassPermissionsMode == "disable"`
    pub disable_bypass: bool,
    /// `sandbox.enabled`
    pub sandbox: Option<bool>,
    /// `enableAllProjectMcpServers`
    pub enable_all_project_mcp: bool,
    /// `enabledMcpjsonServers`
    pub enabled_mcpjson: Vec<String>,
    /// `disabledMcpjsonServers`
    pub disabled_mcpjson: Vec<String>,
    /// `model`
    pub model: Option<String>,
    /// `alwaysThinkingEnabled`
    pub always_thinking: Option<bool>,
    /// `outputStyle`
    pub output_style: Option<String>,
    /// `apiKeyHelper`(脚本路径,不是密钥本身)
    pub api_key_helper: Option<String>,
    /// 本层 `env` 的变量名(值不读取)
    pub env_names: Vec<String>,
    /// 白名单内、不含密钥的 `env` 值(见 `PUBLIC_ENV`)
    pub env_public: Vec<(String, String)>,
}

impl LayerSettings {
    /// 取白名单 env 的值。
    pub fn env(&self, key: &str) -> Option<&str> {
        self.env_public.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

/// 允许读取值的环境变量(都是开关或数值,不含密钥);其余变量只读名称。
pub const PUBLIC_ENV: &[&str] = &[
    "MAX_THINKING_TOKENS",
    "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE",
    "CLAUDE_CODE_MAX_OUTPUT_TOKENS",
    "MCP_TIMEOUT",
    "MCP_TOOL_TIMEOUT",
    "MAX_MCP_OUTPUT_TOKENS",
    "ENABLE_TOOL_SEARCH",
    "DISABLE_TELEMETRY",
    "DISABLE_ERROR_REPORTING",
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
    "DISABLE_AUTOUPDATER",
    "API_TIMEOUT_MS",
    "BASH_DEFAULT_TIMEOUT_MS",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
];

/// 一条钩子。
#[derive(Debug, Clone, PartialEq)]
pub struct HookEntry {
    /// 事件名,如 `PreToolUse`
    pub event: String,
    /// 工具匹配,如 `Bash`;空表示全部
    pub matcher: String,
    /// 处理器类型:`command` / `http` / `prompt`…
    pub kind: String,
    /// 命令(command)、URL(http)或提示词(prompt)
    pub command: String,
    /// 超时(秒)
    pub timeout: Option<u64>,
    pub scope: Scope,
}

/// MCP 服务器的可编辑部分;其余字段(env、headers…)写入时原样保留。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct McpSpec {
    /// `stdio` / `http` / `sse`
    pub transport: String,
    pub command: String,
    pub args: Vec<String>,
    pub url: String,
    /// `env` 里的变量名(值不读取)
    pub env_names: Vec<String>,
    /// `headers` 里的请求头名(值不读取)
    pub header_names: Vec<String>,
}

impl McpSpec {
    /// 列表里显示的目标:URL 或「命令 参数」。
    pub fn target(&self) -> String {
        if self.transport == "stdio" { format!("{} {}", self.command, self.args.join(" ")).trim().to_string() } else { self.url.clone() }
    }
}

/// 一个 MCP 服务器。
#[derive(Debug, Clone, PartialEq)]
pub struct McpServer {
    pub name: String,
    pub spec: McpSpec,
    pub enabled: bool,
    /// 项目层(`.mcp.json`)服务器尚未被批准
    pub needs_approval: bool,
    pub scope: Scope,
}

/// 聚合后的 Claude 配置视图。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClaudeConfig {
    pub permissions: PermissionRules,
    pub hooks: Vec<HookEntry>,
    pub mcp_servers: Vec<McpServer>,
    /// 环境变量名(值不读取)
    pub env_names: Vec<String>,
    pub default_model: Option<String>,
    /// 各层的非列表型设置
    pub layers: Vec<(Scope, LayerSettings)>,
}

impl ClaudeConfig {
    /// 取某一层的设置;该层不存在时返回默认值。
    pub fn layer(&self, scope: Scope) -> LayerSettings {
        self.layers.iter().find(|(s, _)| *s == scope).map(|(_, l)| l.clone()).unwrap_or_default()
    }
}

fn string_list(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}
