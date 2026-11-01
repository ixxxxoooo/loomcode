//! MCP 页:页头(检测连接 / 刷新 / 添加服务器菜单)、范围筛选、卡片列表(连接状态、启停、行菜单、点击展开详情)、
//! 新增 / 编辑 / 粘贴 JSON 表单。同名服务器只显示优先级最高的一条(本地 > 项目 > 用户)。
//!
//! @author ixxxxoooo

use super::confirm::{Confirm, confirm_banner};
use super::ext_ui::{Row, ext_card, ext_empty, ext_header, icon_button, tag};
use super::widgets::{error_banner, scope_chooser, segmented};
use crate::ui::{MONO, caption};
use crate::workbench::Workbench;
use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_config::claude::Health;
use loom_config::{McpServer, Scope};

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
    v_flex().w_full().p_3().gap_2().rounded(px(10.)).border_1().border_color(t.primary.opacity(0.5)).bg(t.popover)
        .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(title))
        .child(scope_chooser(wb, cx))
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

/// 连接状态的色调与文字。
fn health_look(h: Health, cx: &App) -> (Hsla, &'static str) {
    let t = cx.theme();
    match h {
        Health::Connected => (t.success, "已连接"),
        Health::NeedsAuth => (t.warning, "需要认证"),
        Health::Failed => (t.danger, "连接失败"),
        Health::Unknown => (t.muted_foreground, "未知"),
    }
}

/// 点开行后的详情:传输、地址 / 命令、参数、env 与请求头的名称(值不显示)、检测结果。
fn detail(wb: &Workbench, s: &McpServer, cx: &mut Context<Workbench>) -> Div {
    let t = cx.theme().clone();
    let line = |label: &str, value: String| h_flex().gap_3().items_baseline()
        .child(div().w(px(70.)).flex_none().text_size(px(12.)).text_color(t.muted_foreground).child(label.to_string()))
        .child(div().flex_1().min_w_0().font_family(MONO).text_size(px(12.)).child(value));
    v_flex().w_full().gap_1()
        .child(line("传输", s.spec.transport.clone()))
        .when(s.spec.transport == "stdio", |d| d.child(line("命令", s.spec.command.clone())).when(!s.spec.args.is_empty(), |d| d.child(line("参数", s.spec.args.join(" ")))))
        .when(s.spec.transport != "stdio", |d| d.child(line("地址", s.spec.url.clone())))
        .when(!s.spec.env_names.is_empty(), |d| d.child(line("环境变量", s.spec.env_names.join("、") + "(值已隐藏)")))
        .when(!s.spec.header_names.is_empty(), |d| d.child(line("请求头", s.spec.header_names.join("、") + "(值已隐藏)")))
        .when_some(wb.forms().mcp_health.get(&s.name), |d, (_, text)| d.child(line("检测结果", text.clone())))
}

/// 行右侧的「更多」菜单。
fn menu(wb: WeakEntity<Workbench>, s: McpServer) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu, _, _| {
        let (w1, s1) = (wb.clone(), s.clone());
        let edit = PopupMenuItem::new("编辑…").icon(IconName::Pencil).on_click(move |_, window, cx| {
            let s = s1.clone();
            let _ = w1.update(cx, |this, cx| this.open_mcp_form(Some(&s), false, window, cx));
        });
        let (w2, s2) = (wb.clone(), s.clone());
        let del = PopupMenuItem::new("删除…").icon(IconName::Trash).on_click(move |_, _, cx| {
            let s = s2.clone();
            let _ = w2.update(cx, |this, cx| this.ask(Confirm::DeleteMcp(s), cx));
        });
        menu.item(edit).separator().item(del)
    }
}

/// 一行服务器。
fn server_row(wb: &Workbench, i: usize, s: McpServer, has_project: bool, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let health = wb.forms().mcp_health.get(&s.name).map(|(h, _)| *h);
    let tint = health.map(|h| health_look(h, cx).0);
    let mut row = Row::new(format!("mcp-{i}"), IconName::Plug, s.name.clone());
    row.tint = tint;
    row.dimmed = !s.enabled;
    row.mono = true;
    row.lines = 1;
    row.subtitle = Some(s.spec.target());
    row.badges.push(tag(s.scope.label(), Some(t.primary), cx).into_any_element());
    row.badges.push(tag(s.spec.transport.clone(), None, cx).into_any_element());
    if s.needs_approval {
        row.badges.push(tag("待批准", Some(t.warning), cx).into_any_element());
    }
    let name = s.name.clone();
    row.on_click = Some(std::rc::Rc::new(move |this, _, cx| this.toggle_mcp_open(&name, cx)));
    if wb.forms().mcp_open.as_deref() == Some(s.name.as_str()) {
        row.detail = Some(detail(wb, &s, cx).into_any_element());
    }
    let (s_tog, s_appr, weak) = (s.clone(), s.clone(), cx.entity().downgrade());
    row.trailing = Some(h_flex().gap_2p5().items_center()
        .when_some(health, |d, h| {
            let (c, text) = health_look(h, cx);
            d.child(h_flex().gap_1p5().items_center().child(div().size(px(7.)).rounded_full().bg(c)).child(div().text_size(px(11.5)).text_color(t.muted_foreground).child(text)))
        })
        // 启停记在当前项目下,没有项目时不能切换
        .when(s.needs_approval && has_project, |d| d.child(Button::new(SharedString::from(format!("mcp-appr-{i}"))).small().primary().label("批准")
            .on_click(cx.listener(move |this, _, _, cx| { cx.stop_propagation(); this.toggle_mcp(&s_appr, true, cx) }))))
        .when(!s.needs_approval && has_project, |d| d.child(Switch::new(SharedString::from(format!("mcp-on-{i}"))).checked(s.enabled)
            .on_click(cx.listener(move |this, c: &bool, _, cx| this.toggle_mcp(&s_tog, *c, cx)))))
        .child(Button::new(SharedString::from(format!("mcp-more-{i}"))).small().ghost().icon(IconName::Ellipsis).dropdown_menu(menu(weak, s)))
        .into_any_element());
    row.render(cx)
}

/// 范围筛选条:全部 / 用户 / 项目 / 本地,带数量。
fn scope_filter(wb: &Workbench, cx: &mut Context<Workbench>) -> Div {
    let all = &wb.claude_cfg.mcp_servers;
    let cur = wb.forms().mcp_filter;
    let btn = |id: &'static str, label: String, on: bool, scope: Option<Scope>, cx: &mut Context<Workbench>| {
        Button::new(id).small().when(on, |b| b.primary()).when(!on, |b| b.ghost()).label(label)
            .on_click(cx.listener(move |this, _, _, cx| this.set_mcp_filter(scope, cx)))
    };
    let mut row = h_flex().gap_1().child(btn("mcpf-all", format!("全部 {}", all.len()), cur.is_none(), None, cx));
    for (id, scope) in [("mcpf-user", Scope::User), ("mcpf-project", Scope::Project), ("mcpf-local", Scope::Local)] {
        let n = all.iter().filter(|s| s.scope == scope).count();
        row = row.child(btn(id, if n > 0 { format!("{} {n}", scope.label()) } else { scope.label().to_string() }, cur == Some(scope), Some(scope), cx));
    }
    row
}

/// 渲染 MCP 页。参数:`filter` 搜索词(小写,空为不过滤),按名称与目标匹配。
pub fn mcp(wb: &Workbench, filter: &str, cx: &mut Context<Workbench>) -> AnyElement {
    let has_project = wb.claude_cfg_root.is_some();
    let total = wb.claude_cfg.mcp_servers.len();
    let scope_sel = wb.forms().mcp_filter;
    let servers: Vec<McpServer> = wb.claude_cfg.mcp_servers.iter()
        .filter(|s| scope_sel.is_none_or(|sc| s.scope == sc))
        .filter(|s| filter.is_empty() || format!("{} {}", s.name, s.spec.target()).to_lowercase().contains(filter))
        .cloned().collect();
    let open = wb.forms().mcp.open;
    let checking = wb.forms().mcp_checking;
    let weak = cx.entity().downgrade();
    let actions = h_flex().gap_2().items_center()
        .child(Button::new("mcp-check").small().outline().icon(IconName::Timer).label(if checking { "检测中…" } else { "检测连接" }).disabled(checking)
            .tooltip("运行 claude mcp list 检测各服务器的连接状态").on_click(cx.listener(|this, _, _, cx| this.check_mcp_health(cx))))
        .child(icon_button("mcp-refresh", IconName::RefreshCw, "重新读取配置").on_click(cx.listener(|this, _, _, cx| { this.reload_claude_cfg(); cx.notify(); })))
        .child(Button::new("mcp-add").small().primary().icon(IconName::Plus).label("添加服务器").dropdown_menu(move |menu, _, _| {
            let (w1, w2, w3) = (weak.clone(), weak.clone(), weak.clone());
            menu.item(PopupMenuItem::new("表单添加…").icon(IconName::Pencil).on_click(move |_, window, cx| { let _ = w1.update(cx, |this, cx| this.open_mcp_form(None, false, window, cx)); }))
                .item(PopupMenuItem::new("粘贴 JSON 添加…").icon(IconName::Copy).on_click(move |_, window, cx| { let _ = w2.update(cx, |this, cx| this.open_mcp_form(None, true, window, cx)); }))
                .separator()
                .item(PopupMenuItem::new("从 Claude Desktop 导入").icon(IconName::FolderOutput).on_click(move |_, _, cx| { let _ = w3.update(cx, |this, cx| this.import_mcp_desktop(cx)); }))
        }));
    let rows: Vec<AnyElement> = servers.into_iter().enumerate().map(|(i, s)| server_row(wb, i, s, has_project, cx)).collect();
    v_flex().w_full().gap_3()
        .child(ext_header(IconName::Plug, "MCP", "通过 Model Context Protocol 给 Claude 接入外部工具与数据源。配置写入 ~/.claude.json 或项目的 .mcp.json。", actions, cx))
        .children(error_banner(wb, cx))
        .children(confirm_banner(wb, cx))
        .when(open, |d| d.child(form(wb, cx)))
        .when(total > 0, |d| d.child(scope_filter(wb, cx)))
        .when(rows.is_empty(), |d| d.child(ext_empty(
            IconName::Plug,
            if total == 0 { "还没有 MCP 服务器" } else { "没有匹配的服务器" },
            "点击右上角「添加服务器」配置一个,或从 Claude Desktop 导入。",
            cx,
        )))
        .when(!rows.is_empty(), |d| d.child(ext_card(rows, cx)))
        .into_any_element()
}
