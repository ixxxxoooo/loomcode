//! 设置页表单的输入框状态:权限规则、额外目录、MCP 编辑、钩子编辑。首次进入设置页时创建。
//!
//! @author ixxxxoooo

use gpui_kit::component::input::{InputState, TextareaState};
use gpui_kit::*;
use loom_config::Scope;

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
            error: None,
        }
    }
}
