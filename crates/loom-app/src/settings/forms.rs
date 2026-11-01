//! 设置页表单的输入框状态:权限规则、额外目录、MCP 编辑、钩子编辑。首次进入设置页时创建。
//!
//! @author ixxxxoooo

use super::CustomizeTab;
use super::fields::Fields;
use gpui_kit::component::input::{EditorState, InputState, TextareaState};
use gpui_kit::*;
use loom_config::Scope;
use loom_config::items::{Item, ItemKind, MemoryFile};
use std::collections::HashMap;
use std::path::PathBuf;

/// 正在编辑的 Markdown 文件(记忆、命令、子 Agent、Skill 共用)。
pub struct DocEdit {
    pub path: PathBuf,
    pub editor: Entity<EditorState>,
    pub saved: bool,
    /// 打开它的自定义标签;切到别的标签时不显示
    pub tab: CustomizeTab,
}

/// 模型服务商页的状态。密钥只在内存里用于注入与测试,界面不显示。
#[derive(Default)]
pub struct ProviderUi {
    pub store: loom_config::providers::ProviderStore,
    pub keys: HashMap<String, String>,
    /// 正在编辑的服务商 id
    pub selected: Option<String>,
    /// 最近一次测试 / 获取模型的结果:(成功, 提示)
    pub status: Option<(bool, String)>,
    /// 后台请求进行中
    pub busy: bool,
}

/// 插件页的状态。
#[derive(Default)]
pub struct PluginUi {
    pub installed: Vec<loom_config::plugins::PluginEntry>,
    pub markets: Vec<loom_config::plugins::Marketplace>,
    /// 正在执行的 CLI 操作说明;`Some` 时按钮置灰
    pub busy: Option<String>,
    /// 最近一次操作结果:(成功, 输出)
    pub message: Option<(bool, String)>,
    /// 安装到哪一层
    pub install_scope: Option<Scope>,
}

/// 网络、用量、诊断页的状态(后台任务结果)。
#[derive(Default)]
pub struct OpsUi {
    /// 连通性测试:(名称, 网址, 结果);结果为 `None` 表示测试中
    pub net: Vec<(String, String, Option<Result<u64, String>>)>,
    pub usage: Option<loom_claude::usage::UsageReport>,
    pub usage_busy: bool,
    /// `claude doctor` 输出
    pub doctor: Option<String>,
    pub doctor_busy: bool,
    /// (pid, 内存 MB, CPU %, 命令)
    pub procs: Vec<(u32, f64, f64, String)>,
    pub log: Vec<String>,
}

/// 新建命令 / 子 Agent / Skill 的表单。
pub struct ItemForm {
    pub open: bool,
    pub scope: Scope,
    pub name: Entity<InputState>,
    pub description: Entity<InputState>,
}

/// MCP 编辑表单。
pub struct McpForm {
    /// 表单是否展开
    pub open: bool,
    /// 粘贴 JSON 模式
    pub paste: bool,
    /// 正在编辑的服务器(所在层, 原名);新增时为 `None`
    pub editing: Option<(Scope, String)>,
    /// `stdio` / `http` / `sse`
    pub transport: &'static str,
    pub name: Entity<InputState>,
    pub command: Entity<InputState>,
    pub args: Entity<InputState>,
    pub url: Entity<InputState>,
    pub json: Entity<TextareaState>,
}

/// 钩子编辑表单。
pub struct HookForm {
    pub open: bool,
    /// 正在编辑的钩子(所在层, 事件, matcher, 命令);保存时先删旧的
    pub editing: Option<(Scope, String, String, String)>,
    pub event: &'static str,
    pub matcher: Entity<InputState>,
    pub command: Entity<InputState>,
    pub timeout: Entity<InputState>,
}

/// 设置页全部表单状态。
pub struct Forms {
    pub rule: Entity<InputState>,
    pub dir: Entity<InputState>,
    pub mcp: McpForm,
    pub hook: HookForm,
    pub item: ItemForm,
    pub doc: Option<DocEdit>,
    /// 扫描结果缓存(进入设置页、切换项目、增删后刷新)
    pub items: HashMap<ItemKind, Vec<Item>>,
    pub memory_files: Vec<MemoryFile>,
    pub fields: Fields,
    pub providers: ProviderUi,
    pub ops: OpsUi,
    pub plugins: PluginUi,
    /// 待确认的危险操作
    pub confirm: Option<super::confirm::Confirm>,
    /// MCP 页的范围筛选(`None` 为全部)
    pub mcp_filter: Option<Scope>,
    /// MCP 页展开详情的服务器名
    pub mcp_open: Option<String>,
    /// MCP 连接状态:服务器名 → (状态, 文字);检测中为 `checking`
    pub mcp_health: HashMap<String, (loom_config::claude::Health, String)>,
    pub mcp_checking: bool,
    /// 钩子页展开了「显示更多」的分组(层)
    pub hooks_more: Vec<Scope>,
    /// 最近一次写配置失败的提示(显示在页顶,下次成功时清除)
    pub error: Option<String>,
}

/// 新建单行输入框。
fn input(window: &mut Window, cx: &mut App, placeholder: &'static str) -> Entity<InputState> {
    cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
}

impl Forms {
    /// 创建全部输入框。
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            rule: input(window, cx, "输入规则,再点右侧按钮添加"),
            dir: input(window, cx, "目录路径,例如 ../shared-lib"),
            mcp: McpForm {
                open: false,
                paste: false,
                editing: None,
                transport: "stdio",
                name: input(window, cx, "名称,例如 filesystem"),
                command: input(window, cx, "命令,例如 npx"),
                args: input(window, cx, "参数,空格分隔,例如 -y @modelcontextprotocol/server-filesystem ."),
                url: input(window, cx, "https://example.com/mcp"),
                json: cx.new(|cx| TextareaState::new(window, cx).placeholder("{\"名称\": {\"command\": \"npx\", \"args\": [\"-y\", \"...\"]}}")),
            },
            hook: HookForm {
                open: false,
                editing: None,
                event: "PreToolUse",
                matcher: input(window, cx, "工具匹配,例如 Bash 或 Edit|Write;留空匹配全部"),
                command: input(window, cx, "要执行的命令"),
                timeout: input(window, cx, "超时秒数(可空)"),
            },
            item: ItemForm {
                open: false,
                scope: Scope::User,
                name: input(window, cx, "名称:小写字母、数字、- 和 _"),
                description: input(window, cx, "描述:Claude 据此判断什么时候使用"),
            },
            doc: None,
            items: HashMap::new(),
            memory_files: Vec::new(),
            fields: Fields::new(window, cx),
            providers: ProviderUi::default(),
            ops: OpsUi::default(),
            plugins: PluginUi::default(),
            confirm: None,
            mcp_filter: None,
            mcp_open: None,
            mcp_health: HashMap::new(),
            mcp_checking: false,
            hooks_more: Vec::new(),
            error: None,
        }
    }
}
