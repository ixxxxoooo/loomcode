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
    /// One 浅色(Zed One 风格的中性灰蓝)
    OneLight,
    /// One 深色
    OneDark,
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

/// 编辑器自动保存方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AutoSave {
    #[default]
    Off,
    /// 停止输入 1 秒后保存
    Delay,
    /// 切换标签或窗口失焦时保存
    Blur,
}

/// IDE 编辑器偏好。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorPrefs {
    /// 显示迷你地图
    pub minimap: bool,
    /// 迷你地图滑块:真为始终显示,假为悬停时显示
    pub minimap_slider_always: bool,
    /// 迷你地图宽度(像素,60–160)
    pub minimap_width: u16,
    /// 迷你地图右缘显示 Git 改动刻度
    pub minimap_git: bool,
    pub line_numbers: bool,
    pub folding: bool,
    pub soft_wrap: bool,
    /// 缩进宽度(2 / 4 / 8)
    pub tab_size: u8,
    /// 光标贴近括号时高亮配对
    pub bracket_match: bool,
    pub auto_save: AutoSave,
    /// 超过这个大小(KB)的文件不在编辑器里打开
    pub max_file_kb: u32,
    /// 语法高亮(之后新打开的文件生效)
    pub syntax_highlight: bool,
}

impl Default for EditorPrefs {
    fn default() -> Self {
        Self { minimap: true, minimap_slider_always: false, minimap_width: 100, minimap_git: true, line_numbers: true, folding: true, soft_wrap: false, tab_size: 4, bracket_match: true, auto_save: AutoSave::Off, max_file_kb: 1024, syntax_highlight: true }
    }
}

/// 终端光标形状。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CursorShape {
    #[default]
    Block,
    Bar,
    Underline,
}

/// 终端偏好(新开的终端生效;字号与光标立即生效)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalPrefs {
    pub font_size: u8,
    pub cursor: CursorShape,
    /// 回滚行数
    pub scrollback: u32,
    /// shell 程序;空为 `$SHELL`
    pub shell: String,
    /// 以登录 shell 启动(`-l`)
    pub login_shell: bool,
    /// 新终端的起始目录:真为项目根,假为家目录
    pub start_in_project: bool,
}

impl Default for TerminalPrefs {
    fn default() -> Self {
        Self { font_size: 12, cursor: CursorShape::Block, scrollback: 5000, shell: String::new(), login_shell: true, start_in_project: true }
    }
}

/// 内置浏览器偏好。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BrowserPrefs {
    /// 抓取超时(秒)
    pub timeout_secs: u32,
    /// 自定义 User-Agent;空用内置
    pub user_agent: String,
}

impl Default for BrowserPrefs {
    fn default() -> Self {
        Self { timeout_secs: 15, user_agent: String::new() }
    }
}

/// 源代码管理偏好。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScmPrefs {
    /// Git 更改里超过这个大小(KB)的文件只列出不算 diff
    pub diff_max_kb: u32,
}

impl Default for ScmPrefs {
    fn default() -> Self {
        Self { diff_max_kb: 1024 }
    }
}

/// 代码智能(文件检索)偏好。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IntelPrefs {
    /// 快速打开与项目内搜索跳过的目录名,每行一个;支持开头或结尾的 `*`
    pub ignore: String,
}

impl Default for IntelPrefs {
    fn default() -> Self {
        Self { ignore: "target\nnode_modules\nDerivedData\n.build\n.git\ndist".into() }
    }
}

impl IntelPrefs {
    /// 目录 / 文件名是否被忽略规则命中。
    pub fn ignored(&self, name: &str) -> bool {
        self.ignore.lines().map(str::trim).filter(|l| !l.is_empty()).any(|pat| match (pat.strip_prefix('*'), pat.strip_suffix('*')) {
            (Some(suffix), _) if !suffix.is_empty() => name.ends_with(suffix),
            (_, Some(prefix)) if !prefix.is_empty() => name.starts_with(prefix),
            _ => name == pat,
        })
    }
}

/// 滚动条显示方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScrollbarStyle {
    /// 沿用 macOS「显示滚动条」设置
    #[default]
    System,
    /// 仅在滚动时出现
    Auto,
    /// 常驻
    Always,
}

/// 等待动画(运行中提示)的文案风格。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SpinnerStyle {
    #[default]
    Classic,
    Fun,
    ClaudeCode,
    Poetic,
    Geek,
    Minimal,
}

/// 工具调用详细程度(参考 OhMyAgent 与 Cursor 的「对话密度」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolDensity {
    /// 精简:相邻的工具调用(不论读、改、跑命令)合并成一行摘要
    Compact,
    /// 标准:只把连续的浏览类调用(读文件、搜索、访问网页)合并成摘要;编辑、命令等各占一行
    #[default]
    Standard,
    /// 详细:每个工具调用单独一行,编辑与命令在行下直接显示 Diff / 输出
    Detailed,
}

impl ToolDensity {
    /// 三个档位。
    pub const ALL: [ToolDensity; 3] = [Self::Compact, Self::Standard, Self::Detailed];

    /// 档位名。
    pub fn label(self) -> &'static str {
        match self {
            Self::Compact => "精简",
            Self::Standard => "标准",
            Self::Detailed => "详细",
        }
    }

    /// 一句话说明。
    pub fn describe(self) -> &'static str {
        match self {
            Self::Compact => "相邻的工具调用合并成一行摘要,如「运行了 2 条命令、读取了 3 个文件」",
            Self::Standard => "只合并连续的浏览类调用;编辑与命令各占一行,点开看详情",
            Self::Detailed => "每个工具调用一行,编辑的 Diff 与命令输出直接显示在行下",
        }
    }
}

/// 外观偏好:内容宽度、字号、滚动条、等待动画、工具调用详细程度。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearancePrefs {
    /// 会话时间线与输入框的最大宽度(pt)
    pub chat_width: u16,
    /// 设置页右侧内容的最大宽度(pt)
    pub settings_width: u16,
    /// 时间线正文字号百分比(85–150)
    pub timeline_font_pct: u16,
    pub scrollbar: ScrollbarStyle,
    pub spinner: SpinnerStyle,
    /// 工具调用详细程度
    pub tool_density: ToolDensity,
}

impl Default for AppearancePrefs {
    fn default() -> Self {
        Self { chat_width: 820, settings_width: 760, timeline_font_pct: 100, scrollbar: ScrollbarStyle::System, spinner: SpinnerStyle::Classic, tool_density: ToolDensity::Standard }
    }
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
    /// 启动时自动检查更新(只提示,不自动安装)
    pub auto_update_check: bool,
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
    /// 不走代理的主机(逗号分隔),写 `NO_PROXY`
    pub no_proxy: String,
    /// 上次停留的设置分类(小写名,供下次打开设置时恢复)
    pub last_settings_tab: String,
    /// 上次停留的「自定义」标签(小写名)
    pub last_customize_tab: String,
    pub appearance: AppearancePrefs,
    pub editor: EditorPrefs,
    pub terminal: TerminalPrefs,
    pub browser: BrowserPrefs,
    pub scm: ScmPrefs,
    pub intel: IntelPrefs,
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
            auto_update_check: true,
            accent: None,
            append_prompt: String::new(),
            load_claude_md: true,
            proxy_mode: ProxyMode::System,
            proxy_address: String::new(),
            request_timeout: None,
            no_proxy: String::new(),
            last_settings_tab: String::new(),
            last_customize_tab: String::new(),
            appearance: AppearancePrefs::default(),
            editor: EditorPrefs::default(),
            terminal: TerminalPrefs::default(),
            browser: BrowserPrefs::default(),
            scm: ScmPrefs::default(),
            intel: IntelPrefs::default(),
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
        let bypass = self.no_proxy.trim();
        if !bypass.is_empty() && matches!(self.proxy_mode, ProxyMode::Http | ProxyMode::Socks5) {
            env.push(("NO_PROXY".into(), bypass.to_string()));
            env.push(("no_proxy".into(), bypass.to_string()));
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
    fn 新增偏好缺失时用默认值() {
        let s: AppSettings = serde_json::from_str(r#"{"editor":{"minimap":false}}"#).unwrap();
        assert!(!s.editor.minimap);
        assert!(s.editor.folding);
        assert_eq!(s.editor.tab_size, 4);
        assert_eq!(s.terminal.scrollback, 5000);
        assert_eq!(s.browser.timeout_secs, 15);
    }

    #[test]
    fn 旧主题值仍可解析() {
        let s: AppSettings = serde_json::from_str(r#"{"theme":"paper_dark"}"#).unwrap();
        assert_eq!(s.theme, ThemeChoice::PaperDark);
        let s: AppSettings = serde_json::from_str(r#"{"theme":"one_dark"}"#).unwrap();
        assert_eq!(s.theme, ThemeChoice::OneDark);
    }

    #[test]
    fn 忽略规则() {
        let p = IntelPrefs { ignore: "target\n*.log\ntmp*".into() };
        assert!(p.ignored("target"));
        assert!(p.ignored("a.log"));
        assert!(p.ignored("tmp-1"));
        assert!(!p.ignored("src"));
    }

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
        s.proxy_mode = ProxyMode::Http;
        s.proxy_address = "127.0.0.1:7890".into();
        s.no_proxy = "localhost,127.0.0.1".into();
        assert!(s.session_env().contains(&("NO_PROXY".into(), "localhost,127.0.0.1".into())));
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
