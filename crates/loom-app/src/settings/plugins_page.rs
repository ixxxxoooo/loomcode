//! 插件页:页头、安装(`名称@市场`,先确认)、已安装列表(启停 / 更新 / 卸载)、插件市场(添加 / 更新 / 移除)。
//! 启停直接写 `enabledPlugins`;安装、更新、卸载与市场操作调用 claude CLI。
//!
//! @author ixxxxoooo

use super::confirm::{Confirm, confirm_banner};
use super::ext_ui::{Row, ext_card, ext_empty, ext_header, ext_label, icon_button, tag};
use super::widgets::{Tone, banner, error_banner, segmented};
use crate::ui::MONO;
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
use loom_config::plugins::{MarketPlugin, Marketplace, PluginEntry};

/// 一行已安装插件。
fn plugin_row(i: usize, p: PluginEntry, busy: bool, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let mut row = Row::new(format!("plug-{i}"), IconName::Plug, p.name.clone());
    row.dimmed = !p.enabled;
    row.subtitle = Some(p.description.clone());
    row.badges.push(tag(format!("v{}", p.version), None, cx).into_any_element());
    row.badges.push(tag(p.scope.label(), Some(t.primary), cx).into_any_element());
    if !p.market.is_empty() {
        row.badges.push(tag(p.market.clone(), None, cx).into_any_element());
    }
    if !p.components.is_empty() || !p.author.is_empty() {
        let tags: Vec<AnyElement> = p.components.iter().cloned().chain((!p.author.is_empty()).then(|| format!("作者 {}", p.author))).map(|c| tag(c, None, cx).into_any_element()).collect();
        row.detail = Some(h_flex().gap_1().flex_wrap().children(tags).into_any_element());
    }
    let (p_tog, p_up, p_del) = (p.clone(), p.clone(), p);
    row.trailing = Some(h_flex().gap_2().items_center()
        .child(Switch::new(SharedString::from(format!("plug-on-{i}"))).checked(p_tog.enabled).on_click(cx.listener(move |this, c: &bool, _, cx| this.toggle_plugin(&p_tog, *c, cx))))
        .child(Button::new(SharedString::from(format!("plug-up-{i}"))).small().outline().label("更新").disabled(busy)
            .on_click(cx.listener(move |this, _, _, cx| this.plugin_action("update", &p_up, cx))))
        .child(Button::new(SharedString::from(format!("plug-del-{i}"))).small().ghost().text_color(t.danger).label("卸载").disabled(busy)
            .on_click(cx.listener(move |this, _, _, cx| this.ask(Confirm::Uninstall(p_del.clone()), cx))))
        .into_any_element());
    row.render(cx)
}

/// 一行市场。
fn market_row(i: usize, m: Marketplace, busy: bool, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let mut row = Row::new(format!("mk-{i}"), IconName::Globe, m.name.clone());
    row.subtitle = Some(m.source.clone());
    row.mono = true;
    row.lines = 1;
    let (n_up, n_del) = (m.name.clone(), m.name.clone());
    row.trailing = Some(h_flex().gap_2().items_center()
        .child(div().text_size(px(11.5)).text_color(t.muted_foreground).child(m.updated.clone()))
        .child(Button::new(SharedString::from(format!("mk-up-{i}"))).small().outline().label("更新").disabled(busy).on_click(cx.listener(move |this, _, _, cx| this.market_action("update", &n_up, cx))))
        .child(Button::new(SharedString::from(format!("mk-del-{i}"))).small().ghost().text_color(t.danger).label("移除").disabled(busy).on_click(cx.listener(move |this, _, _, cx| this.market_action("remove", &n_del, cx))))
        .into_any_element());
    row.render(cx)
}

/// 一行市场里可安装的插件。
fn market_plugin_row(i: usize, p: MarketPlugin, scope: Scope, busy: bool, cx: &mut Context<Workbench>) -> AnyElement {
    let mut row = Row::new(format!("mp-{i}"), IconName::Plug, p.name.clone());
    row.subtitle = Some(p.description.clone());
    row.badges.push(tag(p.market.clone(), None, cx).into_any_element());
    let id = p.id();
    row.trailing = Some(Button::new(SharedString::from(format!("mp-install-{i}"))).small().outline().label("安装").disabled(busy)
        .on_click(cx.listener(move |this, _, _, cx| this.ask(Confirm::Install(id.clone(), scope), cx))).into_any_element());
    row.render(cx)
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
    let total = ui.installed.len();
    let installed: Vec<PluginEntry> = ui.installed.iter().filter(|p| filter.is_empty() || format!("{} {} {}", p.name, p.description, p.market).to_lowercase().contains(filter)).cloned().collect();
    let markets: Vec<Marketplace> = ui.markets.iter().filter(|m| filter.is_empty() || format!("{} {}", m.name, m.source).to_lowercase().contains(filter)).cloned().collect();
    let all_markets = ui.markets.len();
    let plugin_rows: Vec<AnyElement> = installed.iter().cloned().enumerate().map(|(i, p)| plugin_row(i, p, busy, cx)).collect();
    let market_rows: Vec<AnyElement> = markets.into_iter().enumerate().map(|(i, m)| market_row(i, m, busy, cx)).collect();
    // 浏览市场:列出已添加市场里尚未安装的插件(按搜索词过滤,最多 40 条)
    let browse = ui.browse;
    let available: Vec<MarketPlugin> = ui.market_plugins.iter()
        .filter(|m| !ui.installed.iter().any(|p| p.id == m.id()))
        .filter(|m| filter.is_empty() || format!("{} {} {}", m.name, m.description, m.market).to_lowercase().contains(filter))
        .cloned().collect();
    let available_total = available.len();
    let browse_rows: Vec<AnyElement> = if browse {
        available.into_iter().take(40).enumerate().map(|(i, m)| market_plugin_row(i, m, scope, busy, cx)).collect()
    } else {
        Vec::new()
    };
    let actions = icon_button("plug-refresh", IconName::RefreshCw, "重新读取").on_click(cx.listener(|this, _, _, cx| { this.refresh_plugins(); cx.notify(); }));
    v_flex().w_full().gap_3()
        .child(ext_header(IconName::Plug, "插件", "安装、启停与更新 Claude Code 插件;插件可携带技能、子 Agent、Hooks、MCP 与 LSP。安装、更新、卸载需要本机已安装 claude 命令。", actions, cx))
        .children(error_banner(wb, cx))
        .children(confirm_banner(wb, cx))
        .child(ext_label("安装插件", None, cx))
        .child(h_flex().w_full().gap_2().items_center()
            .child(div().flex_1().child(Input::new(wb.field("plugin_id")).small()))
            .child(segmented("plug-scope", options, scope, cx, |this, v, cx| { this.forms_mut().plugins.install_scope = Some(v); cx.notify(); }))
            .child(Button::new("plug-install").small().primary().label("安装…").disabled(busy).on_click(cx.listener(|this, _, _, cx| this.install_plugin(cx)))))
        .when_some(ui.busy.clone(), |d, label| d.child(div().text_size(px(12.5)).text_color(t.muted_foreground).child(label)))
        .when_some(ui.message.clone(), |d, (ok, text)| d.child(banner(if ok { Tone::Success } else { Tone::Danger }, text, cx).mt_0().font_family(MONO).text_size(px(12.))))
        .child(ext_label("已安装", Some(installed.len()), cx))
        .when(plugin_rows.is_empty(), |d| d.child(ext_empty(
            IconName::Plug,
            if total == 0 { "还没有安装插件" } else { "没有匹配的插件" },
            "在上方输入 plugin@marketplace 安装,或先添加一个插件市场。",
            cx,
        )))
        .when(!plugin_rows.is_empty(), |d| d.child(ext_card(plugin_rows, cx)))
        .child(h_flex().w_full().items_center().pt_2()
            .child(div().flex_1().child(ext_label("插件市场", Some(all_markets), cx)))
            .child(Button::new("mk-update-all").small().outline().label("全部更新").disabled(busy || all_markets == 0).on_click(cx.listener(|this, _, _, cx| this.market_action("update", "", cx)))))
        .child(h_flex().w_full().gap_2()
            .child(div().flex_1().child(Input::new(wb.field("market_src")).small()))
            .child(Button::new("mk-add").small().outline().label("添加市场").disabled(busy).on_click(cx.listener(|this, _, _, cx| this.market_action("add", "", cx)))))
        .when(!market_rows.is_empty(), |d| d.child(ext_card(market_rows, cx)))
        .child(h_flex().w_full().items_center().pt_2()
            .child(div().flex_1().child(ext_label("浏览市场", Some(available_total), cx)))
            .child(Button::new("mk-browse").small().outline().label(if browse { "收起" } else { "展开" }).disabled(all_markets == 0)
                .on_click(cx.listener(|this, _, _, cx| { let f = this.forms_mut(); f.plugins.browse = !f.plugins.browse; cx.notify(); }))))
        .when(browse && browse_rows.is_empty(), |d| d.child(ext_empty(IconName::Plug, "没有可安装的插件", "市场里的插件都已安装,或没有匹配搜索词的插件。", cx)))
        .when(!browse_rows.is_empty(), |d| d.child(ext_card(browse_rows, cx)))
        .when(browse && available_total > 40, |d| d.child(div().text_size(px(12.)).text_color(t.muted_foreground).child(format!("仅显示前 40 个,共 {available_total} 个;用上方搜索框缩小范围"))))
        .into_any_element()
}
