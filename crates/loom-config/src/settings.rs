//! LoomCode 自身偏好。
//!
//! @author ixxxxoooo

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use loom_core::{Effort, PermissionMode};

/// 主题选择。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemeChoice {
    #[default]
    PaperLight,
    PaperDark,
    /// 跟随系统外观
    System,
}

/// 网络代理模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProxyMode {
    /// 不改动,沿用系统 / 登录 Shell 的代理环境变量
    #[default]
    System,
    /// 直连:清空代理变量
    Direct,
    Http,
    Socks5,
}

/// 偏好设置。所有字段都有默认值,旧文件缺字段时不会报错。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub theme: ThemeChoice,
    pub default_model: String,
    pub default_mode: PermissionMode,
    pub default_effort: Effort,
    /// 思考内容默认展开
    pub expand_thinking: bool,
    /// 右侧面板默认打开
    pub right_open: bool,
    /// Agent 窗口左侧栏宽度(像素,拖动调整后保存)
    pub left_width: f32,
    /// Agent 窗口右侧面板宽度(像素,拖动调整后保存)
    pub right_width: f32,
    /// 融合模式:开启后 Agent 与 IDE 在同一窗口原地切换;关闭时 IDE 是独立窗口
    pub fused_ide: bool,
    /// 手动指定 `claude` 可执行文件路径;空则自动查找
    pub claude_path: Option<String>,
    /// 最近使用的项目目录
    pub recent_projects: Vec<String>,
    /// 发送键:真为 ⌘Enter 发送(Enter 换行),假为 Enter 发送
    pub send_with_cmd_enter: bool,
    /// 回合结束且窗口不在前台时发送系统通知
    pub notify_on_finish: bool,
    /// 在菜单栏显示图标
    pub tray_icon: bool,
    /// 主题强调色(十六进制 RRGGBB);空则用主题默认
    pub accent: Option<String>,
    /// 追加到 Claude Code 系统提示末尾的内容(所有新会话)
    pub append_prompt: String,
    /// 加载 CLAUDE.md;关闭时注入 `CLAUDE_CODE_DISABLE_CLAUDE_MDS=1`
    pub load_claude_md: bool,
    pub proxy_mode: ProxyMode,
    /// 代理地址,如 `127.0.0.1:7890`
    pub proxy_address: String,
    /// API 请求超时(秒),写 `API_TIMEOUT_MS`;空为默认
    pub request_timeout: Option<u64>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::default(),
            default_model: "default".into(),
            default_mode: PermissionMode::Default,
            default_effort: Effort::Medium,
            expand_thinking: false,
            right_open: true,
            left_width: 264.,
            right_width: 400.,
            fused_ide: false,
            claude_path: None,
            recent_projects: Vec::new(),
            send_with_cmd_enter: false,
            notify_on_finish: true,
            tray_icon: true,
            accent: None,
            append_prompt: String::new(),
            load_claude_md: true,
            proxy_mode: ProxyMode::System,
            proxy_address: String::new(),
            request_timeout: None,
        }
    }
}

impl AppSettings {
    /// 默认存放位置 `~/.loomcode/settings.json`。
    pub fn default_path() -> Option<PathBuf> {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".loomcode").join("settings.json"))
    }

    /// 读取;文件不存在或损坏时返回默认值。
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    /// 写入(自动创建目录);先写临时文件再改名,避免写一半崩溃。
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self).map_err(std::io::Error::other)?)?;
        std::fs::rename(tmp, path)
    }

    /// 由客户端偏好生成、注入 claude 进程的环境变量(代理、超时、CLAUDE.md 开关)。
    ///
    /// 返回:(变量名, 值) 列表;直连模式下代理变量设为空串以覆盖继承值。
    pub fn session_env(&self) -> Vec<(String, String)> {
        let mut env = Vec::new();
        let addr = self.proxy_address.trim();
        let proxy = match self.proxy_mode {
            ProxyMode::System => None,
            ProxyMode::Direct => Some(String::new()),
            ProxyMode::Http if !addr.is_empty() => Some(if addr.contains("://") { addr.to_string() } else { format!("http://{addr}") }),
            ProxyMode::Socks5 if !addr.is_empty() => Some(if addr.contains("://") { addr.to_string() } else { format!("socks5://{addr}") }),
            _ => None,
        };
        if let Some(p) = proxy {
            for k in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"] {
                env.push((k.to_string(), p.clone()));
            }
        }
        if let Some(t) = self.request_timeout.filter(|t| *t > 0) {
            env.push(("API_TIMEOUT_MS".into(), (t * 1000).to_string()));
        }
        if !self.load_claude_md {
            env.push(("CLAUDE_CODE_DISABLE_CLAUDE_MDS".into(), "1".into()));
        }
        env
    }

    /// 记录最近项目(去重,最多 12 个,最新在前)。
    pub fn touch_project(&mut self, path: &str) {
        self.recent_projects.retain(|p| p != path);
        self.recent_projects.insert(0, path.to_string());
        self.recent_projects.truncate(12);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 缺字段使用默认值() {
        let s: AppSettings = serde_json::from_str(r#"{"expand_thinking":true}"#).unwrap();
        assert!(s.expand_thinking);
        assert_eq!(s.default_model, "default");
        assert!(s.right_open);
    }

    #[test]
    fn 旧文件缺宽度字段时用默认值() {
        let s: AppSettings = serde_json::from_str(r#"{"right_open":false}"#).unwrap();
        assert_eq!((s.left_width, s.right_width), (264., 400.));
    }

    #[test]
    fn 保存后可读回() {
        let dir = std::env::temp_dir().join(format!("loom-cfg-{}", std::process::id()));
        let path = dir.join("settings.json");
        let mut s = AppSettings::default();
        s.theme = ThemeChoice::PaperDark;
        s.touch_project("/a");
        s.touch_project("/b");
        s.touch_project("/a");
        s.save(&path).unwrap();
        let back = AppSettings::load(&path);
        assert_eq!(back, s);
        assert_eq!(back.recent_projects, vec!["/a", "/b"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn 会话环境变量() {
        let mut s = AppSettings::default();
        assert!(s.session_env().is_empty());
        s.proxy_mode = ProxyMode::Http;
        s.proxy_address = "127.0.0.1:7890".into();
        s.request_timeout = Some(60);
        s.load_claude_md = false;
        let env = s.session_env();
        assert!(env.contains(&("HTTPS_PROXY".into(), "http://127.0.0.1:7890".into())));
        assert!(env.contains(&("API_TIMEOUT_MS".into(), "60000".into())));
        assert!(env.contains(&("CLAUDE_CODE_DISABLE_CLAUDE_MDS".into(), "1".into())));
        s.proxy_mode = ProxyMode::Direct;
        assert!(s.session_env().contains(&("ALL_PROXY".into(), String::new())));
        s.proxy_mode = ProxyMode::Socks5;
        s.proxy_address = String::new();
        assert!(!s.session_env().iter().any(|(k, _)| k == "ALL_PROXY"));
    }

    #[test]
    fn 损坏文件回退默认() {
        let dir = std::env::temp_dir().join(format!("loom-cfg-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(&path, "{bad").unwrap();
        assert_eq!(AppSettings::load(&path), AppSettings::default());
        let _ = std::fs::remove_dir_all(dir);
    }
}
