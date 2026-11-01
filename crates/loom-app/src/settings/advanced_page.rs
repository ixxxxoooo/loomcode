//! 高级页:隐私与遥测、MCP 运行参数(当前配置层 env),claude 路径、数据目录与恢复默认(客户端)。
//!
//! @author ixxxxoooo

use super::layer_actions::Target;
use super::model_page::text_row;
use super::widgets::{error_banner, page_title, row, scope_header, section_title, segmented};
use crate::ui::{MONO, caption};
use crate::workbench::Workbench;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::*;

/// 隐私开关:(env 变量, 标题, 说明)。
const PRIVACY: &[(&str, &str, &str)] = &[
    ("DISABLE_TELEMETRY", "关闭使用统计", "DISABLE_TELEMETRY:不上报功能使用数据"),
    ("DISABLE_ERROR_REPORTING", "关闭错误报告", "DISABLE_ERROR_REPORTING:不上报崩溃与错误"),
    ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "关闭非必要网络请求", "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC:同时关闭统计、错误报告、自动更新检查等"),
    ("DISABLE_AUTOUPDATER", "关闭 claude 自动更新", "DISABLE_AUTOUPDATER"),
];

/// 在 Finder 中显示目录。
fn reveal(path: std::path::PathBuf) {
    let _ = std::process::Command::new("open").arg(path).spawn();
}

/// 渲染高级页。
pub fn render(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let scope = wb.effective_scope();
    let layer = wb.claude_cfg.layer(scope);
    let target = match wb.scope_settings_file(scope) {
        Some(p) => format!("隐私与 MCP 参数写入 {}", p.display()),
        None => "先打开一个项目会话".into(),
    };
    let tool_search = match layer.env("ENABLE_TOOL_SEARCH") {
        Some("true") | Some("1") => "true",
        Some("false") | Some("0") => "false",
        _ => "",
    };
    let data_dir = wb.settings_path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    v_flex().w_full()
        .child(page_title("高级"))
        .child(scope_header(wb, target, cx))
        .children(error_banner(wb, cx))
        .child(section_title("隐私与遥测", cx))
        .children(PRIVACY.iter().map(|(key, title, desc)| {
            let key = *key;
            row(title, desc, Switch::new(SharedString::from(format!("sw-{key}"))).checked(layer.env(key).is_some_and(|v| v != "0" && v != "false"))
                .on_click(cx.listener(move |this, c: &bool, _, cx| this.set_env_flag(key, *c, cx))), cx)
        }))
        .child(section_title("MCP 运行参数", cx))
        .child(row("MCP 工具按需加载", "ENABLE_TOOL_SEARCH:工具很多时只在需要时加载描述,节省上下文", segmented("tool-search", vec![("", "自动"), ("true", "开启"), ("false", "关闭")], tool_search, cx,
            |this, v, cx| this.set_env_value("ENABLE_TOOL_SEARCH", (!v.is_empty()).then_some(v), cx)), cx))
        .child(text_row(wb, "save-mcp-timeout", "启动超时", "MCP_TIMEOUT(毫秒)", "mcp_timeout", Target::Env("MCP_TIMEOUT", true), cx))
        .child(text_row(wb, "save-mcp-tool-timeout", "工具调用超时", "MCP_TOOL_TIMEOUT(毫秒)", "mcp_tool_timeout", Target::Env("MCP_TOOL_TIMEOUT", true), cx))
        .child(text_row(wb, "save-mcp-output", "工具输出上限", "MAX_MCP_OUTPUT_TOKENS", "mcp_output", Target::Env("MAX_MCP_OUTPUT_TOKENS", true), cx))
        .child(section_title("客户端", cx))
        .child(row("claude 可执行文件", &format!("当前:{}", wb.claude_info.clone().unwrap_or_else(|| "检测中…".into())), h_flex().w(px(300.)).gap_2()
            .child(div().flex_1().child(Input::new(wb.field("claude_path")).small()))
            .child(Button::new("save-claude-path").small().outline().label("应用并检测").on_click(cx.listener(|this, _, _, cx| this.save_claude_path(cx)))), cx))
        .child(row("数据目录", "", h_flex().gap_2().items_center()
            .child(div().font_family(MONO).text_size(px(12.)).text_color(t.muted_foreground).child(data_dir.display().to_string()))
            .child(Button::new("reveal-data").small().ghost().label("在 Finder 中显示").on_click(move |_, _, _| reveal(data_dir.clone()))), cx))
        .child(row("恢复默认客户端偏好", "主题、发送键、通知、代理等恢复默认;不影响 Claude 配置、栏宽与最近项目", Button::new("reset-prefs").small().outline().label("恢复默认")
            .on_click(cx.listener(|this, _, _, cx| this.reset_client_prefs(cx))), cx))
        .child(div().pt_2().child(caption("Claude 配置文件被修改前会备份为同目录的 *.loom-backup", cx)))
        .into_any_element()
}
