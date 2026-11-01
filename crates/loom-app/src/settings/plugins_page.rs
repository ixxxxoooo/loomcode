//! 插件页:安装(`名称@市场`)、已安装列表(启停 / 更新 / 卸载)、插件市场(添加 / 更新 / 移除)。
//! 启停直接写 `enabledPlugins`;安装、更新、卸载与市场操作调用 claude CLI。
//!
//! @author ixxxxoooo

use super::claude_pages::scope_pill;
use super::widgets::{error_banner, section_title, segmented};
use crate::ui::{MONO, caption, icon, pill};
use crate::workbench::Workbench;
use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_config::Scope;
use loom_config::plugins::{Marketplace, PluginEntry};

/// 图标方块。
fn tile(name: IconName, cx: &App) -> Div {
    let t = cx.theme();
    div().size(px(32.)).flex_none().flex().items_center().justify_center().rounded(px(8.)).bg(t.primary.opacity(0.1)).child(icon(name, 16., t.primary))
}

/// 一行已安装插件。
fn plugin_row(i: usize, p: PluginEntry, busy: bool, cx: &mut Context<Workbench>) -> Div {
    let t = cx.theme().clone();
    let (p_tog, p_up, p_del) = (p.clone(), p.clone(), p.clone());
    h_flex().w_full().p_3().gap_3().items_start().when(!p.enabled, |d| d.opacity(0.7))
        .child(tile(IconName::Plug, cx))
        .child(v_flex().flex_1().min_w_0().gap_1()
            .child(h_flex().gap_2().items_center().flex_wrap()
                .child(div().text_size(px(14.)).font_weight(FontWeight::MEDIUM).child(p.name.clone()))
                .child(pill(format!("v{}", p.version), t.muted_foreground, t.muted))
                .child(scope_pill(p.scope, cx))
                .when(!p.market.is_empty(), |d| d.child(pill(p.market.clone(), t.muted_foreground, t.muted))))
            .when(!p.description.is_empty(), |d| d.child(div().text_size(px(12.5)).text_color(t.muted_foreground).child(p.description.clone())))
            .when(!p.components.is_empty() || !p.author.is_empty(), |d| d.child(h_flex().gap_1().flex_wrap()
                .children(p.components.iter().map(|c| pill(c.clone(), t.muted_foreground, t.muted)))
                .when(!p.author.is_empty(), |d| d.child(pill(format!("作者 {}", p.author), t.muted_foreground, t.muted))))))
        .child(h_flex().gap_2().items_center().flex_none()
            .child(Switch::new(SharedString::from(format!("plug-on-{i}"))).checked(p.enabled).on_click(cx.listener(move |this, c: &bool, _, cx| this.toggle_plugin(&p_tog, *c, cx))))
            .child(Button::new(SharedString::from(format!("plug-up-{i}"))).small().outline().label("更新").disabled(busy)
                .on_click(cx.listener(move |this, _, _, cx| this.plugin_action("update", &p_up, cx))))
            .child(Button::new(SharedString::from(format!("plug-del-{i}"))).small().ghost().label("卸载").disabled(busy)
                .on_click(cx.listener(move |this, _, _, cx| this.plugin_action("uninstall", &p_del, cx)))))
}

/// 一行市场。
fn market_row(i: usize, m: Marketplace, busy: bool, cx: &mut Context<Workbench>) -> Div {
    let t = cx.theme().clone();
    let (n_up, n_del) = (m.name.clone(), m.name.clone());
    h_flex().w_full().p_3().gap_3().items_center()
        .child(tile(IconName::Globe, cx))
        .child(v_flex().flex_1().min_w_0()
            .child(div().text_size(px(14.)).font_weight(FontWeight::MEDIUM).child(m.name.clone()))
            .child(div().truncate().font_family(MONO).text_size(px(12.)).text_color(t.muted_foreground).child(m.source.clone())))
        .child(caption(m.updated.clone(), cx))
        .child(Button::new(SharedString::from(format!("mk-up-{i}"))).small().outline().label("更新").disabled(busy).on_click(cx.listener(move |this, _, _, cx| this.market_action("update", &n_up, cx))))
        .child(Button::new(SharedString::from(format!("mk-del-{i}"))).small().ghost().label("移除").disabled(busy).on_click(cx.listener(move |this, _, _, cx| this.market_action("remove", &n_del, cx))))
}

/// 渲染插件页。
///
/// 参数:`filter` 搜索词(小写,空为不过滤),按名称、描述、市场匹配。
pub fn render(wb: &Workbench, filter: &str, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let ui = &wb.forms().plugins;
    let busy = ui.busy.is_some();
    let has_project = wb.claude_cfg_root.is_some();
    let scope = wb.plugin_install_scope();
    let options: Vec<(Scope, &'static str)> = Scope::ALL.into_iter().filter(|s| has_project || !s.needs_project()).map(|s| (s, s.label())).collect();
    let installed: Vec<PluginEntry> = ui.installed.iter().filter(|p| filter.is_empty() || format!("{} {} {}", p.name, p.description, p.market).to_lowercase().contains(filter)).cloned().collect();
    let markets: Vec<Marketplace> = ui.markets.iter().filter(|m| filter.is_empty() || format!("{} {}", m.name, m.source).to_lowercase().contains(filter)).cloned().collect();
    let all_markets = ui.markets.len();
    let panel = |rows: Vec<Div>| v_flex().w_full().rounded(px(10.)).border_1().border_color(t.border).children(rows.into_iter().enumerate().map(|(i, r)| {
        div().w_full().when(i > 0, |d| d.border_t_1().border_color(t.border)).child(r)
    }));
    v_flex().w_full()
        .child(caption("安装、启停与更新 Claude Code 插件;插件可携带技能、子 Agent、Hooks、MCP 与 LSP。安装、更新、卸载需要本机已安装 claude 命令,只安装你信任的来源", cx))
        .children(error_banner(wb, cx))
        .child(section_title("安装插件", cx))
        .child(h_flex().w_full().gap_2().items_center()
            .child(div().flex_1().child(Input::new(wb.field("plugin_id")).small()))
            .child(segmented("plug-scope", options, scope, cx, |this, v, cx| { this.forms_mut().plugins.install_scope = Some(v); cx.notify(); }))
            .child(Button::new("plug-install").small().primary().label("安装").disabled(busy).on_click(cx.listener(|this, _, _, cx| this.install_plugin(cx)))))
        .when_some(ui.busy.clone(), |d, label| d.child(div().pt_2().text_size(px(12.5)).text_color(t.muted_foreground).child(label)))
        .when_some(ui.message.clone(), |d, (ok, text)| d.child(div().mt_2().px_3().py_2().rounded(px(8.)).bg(t.muted).text_size(px(12.)).font_family(MONO)
            .text_color(if ok { t.foreground } else { t.danger }).child(text)))
        .child(section_title(&format!("已安装 {}", installed.len()), cx))
        .when(installed.is_empty(), |d| d.child(caption(if filter.is_empty() { "还没有安装插件" } else { "没有匹配的插件" }, cx)))
        .when(!installed.is_empty(), |d| d.child(panel(installed.into_iter().enumerate().map(|(i, p)| plugin_row(i, p, busy, cx)).collect())))
        .child(h_flex().w_full().items_center().pt_5().pb_1()
            .child(div().flex_1().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(t.muted_foreground).child(format!("插件市场 {all_markets}")))
            .child(Button::new("mk-update-all").small().outline().label("全部更新").disabled(busy || all_markets == 0).on_click(cx.listener(|this, _, _, cx| this.market_action("update", "", cx)))))
        .child(h_flex().w_full().gap_2().pb_2()
            .child(div().flex_1().child(Input::new(wb.field("market_src")).small()))
            .child(Button::new("mk-add").small().outline().label("添加市场").disabled(busy).on_click(cx.listener(|this, _, _, cx| this.market_action("add", "", cx)))))
        .when(!markets.is_empty(), |d| d.child(panel(markets.into_iter().enumerate().map(|(i, m)| market_row(i, m, busy, cx)).collect())))
        .into_any_element()
}
