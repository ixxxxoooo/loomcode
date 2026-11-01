//! 高级页:MCP 运行参数(当前配置层 env),claude 路径、数据目录与恢复默认(客户端)。
//!
//! @author ixxxxoooo

use super::model_page::text_row;
use super::widgets::{error_banner, group, note, page_header, row, rows, scope_header, segmented};
use crate::ui::MONO;
use crate::workbench::Workbench;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::*;

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
        Some(p) => format!("MCP 参数写入 {}", p.display()),
        None => "先打开一个项目会话".into(),
    };
    let tool_search = match layer.env("ENABLE_TOOL_SEARCH") {
        Some("true") | Some("1") => "true",
        Some("false") | Some("0") => "false",
        _ => "",
    };
    let data_dir = wb.settings_path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    v_flex().w_full()
        .child(page_header("高级", "MCP 运行参数、claude 路径与数据目录", div(), cx))
        .child(scope_header(wb, target, cx))
        .children(error_banner(wb, cx))
        .child(group("MCP 运行参数", rows![
            row("MCP 工具按需加载", "ENABLE_TOOL_SEARCH:工具很多时只在需要时加载描述,节省上下文", segmented("tool-search", vec![("", "自动"), ("true", "开启"), ("false", "关闭")], tool_search, cx,
                |this, v, cx| this.set_env_value("ENABLE_TOOL_SEARCH", (!v.is_empty()).then_some(v), cx)), cx),
            text_row(wb, "启动超时", "MCP_TIMEOUT(毫秒)", "mcp_timeout", cx),
            text_row(wb, "工具调用超时", "MCP_TOOL_TIMEOUT(毫秒)", "mcp_tool_timeout", cx),
            text_row(wb, "工具输出上限", "MAX_MCP_OUTPUT_TOKENS", "mcp_output", cx),
        ], cx))
        .child(group("客户端", rows![
            row("claude 可执行文件", &format!("当前:{};留空自动查找,失焦或回车后校验并重新检测", wb.claude_info.clone().unwrap_or_else(|| "检测中…".into())),
                div().w(px(280.)).child(Input::new(wb.field("claude_path")).small()), cx),
            row("数据目录", "设置、服务商、日志与诊断包所在目录", h_flex().gap_2().items_center()
                .child(div().font_family(MONO).text_size(px(12.)).text_color(t.muted_foreground).child(crate::settings::fields::tilde(&data_dir)))
                .child(Button::new("reveal-data").small().ghost().label("在 Finder 中显示").on_click(move |_, _, _| reveal(data_dir.clone()))), cx),
            row("恢复默认客户端偏好", "主题、发送键、通知、代理等恢复默认;不影响 Claude 配置、栏宽与最近项目", Button::new("reset-prefs").small().outline().label("恢复默认")
                .on_click(cx.listener(|this, _, _, cx| this.reset_client_prefs(cx))), cx),
        ], cx))
        .child(note("Claude 配置文件被修改前会备份为同目录的 *.loom-backup", cx))
        .into_any_element()
}
