//! MCP 页:新增 / 编辑 / 粘贴 JSON、启停、删除;同名服务器只显示优先级最高的一条(本地 > 项目 > 用户)。
//!
//! @author ixxxxoooo

use super::claude_pages::scope_pill;
use super::widgets::{error_banner, page_title, scope_header, segmented};
use crate::ui::{MONO, caption, icon, pill};
use crate::workbench::Workbench;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_config::McpServer;
use loom_config::claude::McpLocation;

/// 表单里的一行:左标签 + 右输入。
fn field(label: &str, input: impl IntoElement) -> Div {
    h_flex().w_full().gap_3().items_center()
        .child(div().w(px(56.)).flex_none().text_size(px(12.5)).child(label.to_string()))
        .child(div().flex_1().child(input))
}

/// 新增 / 编辑表单。
fn form(wb: &Workbench, cx: &mut Context<Workbench>) -> Div {
    let t = cx.theme().clone();
    let f = &wb.forms().mcp;
    let title = match (&f.editing, f.paste) {
        (Some((_, n)), _) => format!("编辑 {n}"),
        (None, true) => "粘贴 JSON 添加".into(),
        (None, false) => "添加服务器".into(),
    };
    let stdio = f.transport == "stdio";
    v_flex().w_full().mt_3().p_3().gap_2().rounded(px(10.)).border_1().border_color(t.primary.opacity(0.5))
        .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(title))
        .when(f.paste, |d| d
            .child(caption("支持 {\"mcpServers\": {...}} 或 {\"名称\": {...}};env、headers 等字段原样写入", cx))
            .child(div().w_full().h(px(140.)).rounded(px(6.)).border_1().border_color(t.border).p_2().font_family(MONO)
                .child(Textarea::new(&f.json).appearance(false).bordered(false).size_full())))
        .when(!f.paste, |d| d
            .child(field("名称", Input::new(&f.name).small()))
            .child(field("传输", segmented("mcp-transport", vec![("stdio", "stdio"), ("http", "http"), ("sse", "sse")], f.transport, cx,
                |this, v, cx| { this.forms_mut().mcp.transport = v; cx.notify(); })))
            .when(stdio, |d| d.child(field("命令", Input::new(&f.command).small())).child(field("参数", Input::new(&f.args).small())))
            .when(!stdio, |d| d.child(field("URL", Input::new(&f.url).small())))
            .child(caption("env、headers 等其它字段保留不变,需要时可用「粘贴 JSON」或直接编辑配置文件", cx)))
        .child(h_flex().w_full().gap_2().justify_end()
            .child(Button::new("mcp-cancel").small().ghost().label("取消").on_click(cx.listener(|this, _, _, cx| this.close_mcp_form(cx))))
            .child(Button::new("mcp-save").small().primary().label("保存").on_click(cx.listener(|this, _, _, cx| this.save_mcp_form(cx)))))
}

/// 一张服务器卡片。
fn server_card(i: usize, s: McpServer, has_project: bool, cx: &mut Context<Workbench>) -> Div {
    let t = cx.theme().clone();
    let (s_edit, s_del, s_tog) = (s.clone(), s.clone(), s.clone());
    h_flex().w_full().p_3().gap_3().items_center().rounded(px(10.)).border_1().border_color(t.border)
        .child(icon(IconName::Plug, 16., if s.enabled { t.primary } else { t.muted_foreground }))
        .child(v_flex().flex_1().min_w_0().gap_0p5()
            .child(h_flex().gap_2().items_center()
                .child(div().text_size(px(13.5)).font_weight(FontWeight::MEDIUM).child(s.name.clone()))
                .child(pill(s.spec.transport.clone(), t.muted_foreground, t.muted))
                .child(scope_pill(s.scope, cx))
                .when(s.needs_approval, |d| d.child(pill("待批准", t.warning, t.muted))))
            .child(div().truncate().font_family(MONO).text_size(px(12.)).text_color(t.muted_foreground).child(s.spec.target())))
        // 启停记在当前项目下,没有项目时不能切换
        .when(has_project, |d| d.child(Switch::new(SharedString::from(format!("mcp-on-{i}"))).checked(s.enabled && !s.needs_approval)
            .on_click(cx.listener(move |this, c: &bool, _, cx| this.toggle_mcp(&s_tog, *c, cx)))))
        .when(!has_project, |d| d.child(pill(if s.enabled { "已启用" } else { "已禁用" }, if s.enabled { t.success } else { t.muted_foreground }, t.muted)))
        .child(Button::new(SharedString::from(format!("mcp-edit-{i}"))).small().ghost().icon(IconName::Pencil)
            .on_click(cx.listener(move |this, _, window, cx| this.open_mcp_form(Some(&s_edit), false, window, cx))))
        .child(Button::new(SharedString::from(format!("mcp-del-{i}"))).small().ghost().icon(IconName::Trash)
            .on_click(cx.listener(move |this, _, _, cx| this.delete_mcp(&s_del, cx))))
}

/// MCP 页。
pub fn mcp(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let servers = wb.claude_cfg.mcp_servers.clone();
    let has_project = wb.claude_cfg_root.is_some();
    let open = wb.forms().mcp.open;
    let target = match McpLocation::of(&wb.claude_home, wb.cfg_project(), wb.effective_scope()) {
        Some(loc) => format!("新增写入 {}({})", loc.file.display(), loc.keys.join(" → ")),
        None => "先打开一个项目会话".into(),
    };
    v_flex().w_full()
        .child(h_flex().w_full().items_center()
            .child(div().flex_1().child(page_title("MCP 服务器")))
            .child(h_flex().gap_2()
                .child(Button::new("mcp-paste").small().outline().label("粘贴 JSON").on_click(cx.listener(|this, _, window, cx| this.open_mcp_form(None, true, window, cx))))
                .child(Button::new("mcp-add").small().primary().icon(IconName::Plus).label("添加").on_click(cx.listener(|this, _, window, cx| this.open_mcp_form(None, false, window, cx))))))
        .child(caption("用户层 ~/.claude.json、项目层 .mcp.json、本地层 ~/.claude.json 的当前项目条目;同名时 本地 > 项目 > 用户。项目层服务器需批准后才会加载,打开开关即批准", cx))
        .child(scope_header(wb, target, cx))
        .children(error_banner(wb, cx))
        .when(open, |d| d.child(form(wb, cx)))
        .child(v_flex().w_full().pt_4().gap_2()
            .when(servers.is_empty(), |d| d.child(caption("没有配置 MCP 服务器", cx)))
            .children(servers.into_iter().enumerate().map(|(i, s)| server_card(i, s, has_project, cx))))
        .into_any_element()
}
