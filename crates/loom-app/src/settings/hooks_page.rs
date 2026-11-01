//! 钩子页:按配置层分组的卡片列表;新增 / 编辑 / 删除 command 类型钩子,http、prompt 等类型只读显示、可删除。
//!
//! @author ixxxxoooo

use super::confirm::{Confirm, confirm_banner};
use super::ext_ui::{Row, ext_card, ext_empty, ext_header, ext_label, icon_button, tag};
use super::widgets::{error_banner, scope_chooser};
use crate::ui::caption;
use crate::workbench::Workbench;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_config::claude::HOOK_EVENTS;
use loom_config::{HookEntry, Scope};

/// 每组默认显示的条数,其余折叠到「显示更多」。
const COLLAPSED: usize = 5;

/// 新增 / 编辑表单。
fn form(wb: &Workbench, cx: &mut Context<Workbench>) -> Div {
    let t = cx.theme().clone();
    let f = &wb.forms().hook;
    let cur = f.event;
    v_flex().w_full().p_3().gap_2().rounded(px(10.)).border_1().border_color(t.primary.opacity(0.5)).bg(t.popover)
        .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(if f.editing.is_some() { "编辑钩子" } else { "添加钩子" }))
        .child(scope_chooser(wb, cx))
        .child(h_flex().w_full().flex_wrap().gap_1().children(HOOK_EVENTS.iter().map(|e| {
            let e = *e;
            Button::new(SharedString::from(format!("hook-ev-{e}"))).xsmall().when(e == cur, |b| b.primary()).when(e != cur, |b| b.ghost()).label(e)
                .on_click(cx.listener(move |this, _, _, cx| { this.forms_mut().hook.event = e; cx.notify(); }))
        })))
        .child(Input::new(&f.matcher).small())
        .child(Input::new(&f.command).small())
        .child(div().w(px(200.)).child(Input::new(&f.timeout).small()))
        .child(caption("matcher 只对 PreToolUse / PostToolUse 等工具事件有意义;命令通过 stdin 收到事件 JSON", cx))
        .child(h_flex().w_full().gap_2().justify_end()
            .child(Button::new("hook-cancel").small().ghost().label("取消").on_click(cx.listener(|this, _, _, cx| this.close_hook_form(cx))))
            .child(Button::new("hook-save").small().primary().label("保存").on_click(cx.listener(|this, _, _, cx| this.save_hook_form(cx)))))
}

/// 一行钩子:事件名为标题,命令单行作说明;匹配器与超时放在标签里。
fn hook_row(i: usize, h: HookEntry, cx: &mut Context<Workbench>) -> AnyElement {
    let editable = h.kind == "command";
    let mut row = Row::new(format!("hook-{i}"), IconName::Webhook, h.event.clone());
    row.subtitle = Some(h.command.clone());
    row.mono = true;
    row.lines = 1;
    if !h.matcher.is_empty() {
        row.badges.push(tag(format!("匹配 {}", h.matcher), None, cx).into_any_element());
    }
    if let Some(s) = h.timeout {
        row.badges.push(tag(format!("{s}s"), None, cx).into_any_element());
    }
    if !editable {
        row.badges.push(tag(h.kind.clone(), None, cx).into_any_element());
    }
    let (h_edit, h_del, h_click) = (h.clone(), h.clone(), h);
    if editable {
        row.on_click = Some(std::rc::Rc::new(move |this, window, cx| this.open_hook_form(Some(&h_click), window, cx)));
    }
    row.trailing = Some(h_flex().gap_1()
        .when(editable, |d| d.child(icon_button(SharedString::from(format!("hook-edit-{i}")), IconName::Pencil, "编辑")
            .on_click(cx.listener(move |this, _, window, cx| { cx.stop_propagation(); this.open_hook_form(Some(&h_edit), window, cx) }))))
        .child(icon_button(SharedString::from(format!("hook-del-{i}")), IconName::Trash, "删除")
            .on_click(cx.listener(move |this, _, _, cx| { cx.stop_propagation(); this.ask(Confirm::DeleteHook(h_del.clone()), cx) })))
        .into_any_element());
    row.render(cx)
}

/// 渲染钩子页。参数:`filter` 搜索词(小写,空为不过滤),按事件、匹配器与命令匹配。
pub fn hooks(wb: &Workbench, filter: &str, cx: &mut Context<Workbench>) -> AnyElement {
    let all = wb.claude_cfg.hooks.len();
    let hooks: Vec<HookEntry> = wb.claude_cfg.hooks.iter().filter(|h| filter.is_empty() || format!("{} {} {}", h.event, h.matcher, h.command).to_lowercase().contains(filter)).cloned().collect();
    let open = wb.forms().hook.open;
    let actions = h_flex().gap_2().items_center()
        .child(icon_button("hook-refresh", IconName::RefreshCw, "重新读取配置").on_click(cx.listener(|this, _, _, cx| { this.reload_claude_cfg(); cx.notify(); })))
        .child(Button::new("hook-add").small().primary().icon(IconName::Plus).label("添加 Hook").on_click(cx.listener(|this, _, window, cx| this.open_hook_form(None, window, cx))));
    let mut body = v_flex().w_full().gap_2();
    for scope in Scope::ALL {
        let group: Vec<(usize, HookEntry)> = hooks.iter().cloned().enumerate().filter(|(_, h)| h.scope == scope).collect();
        if group.is_empty() {
            continue;
        }
        let total = group.len();
        let show_all = wb.forms().hooks_more.contains(&scope) || total <= COLLAPSED + 1;
        let shown: Vec<_> = if show_all { group } else { group.into_iter().take(COLLAPSED).collect() };
        body = body.child(ext_label(scope.label(), Some(total), cx));
        body = body.child(ext_card(shown.into_iter().map(|(i, h)| hook_row(i, h, cx)).collect(), cx));
        if !show_all {
            body = body.child(h_flex().child(Button::new(SharedString::from(format!("hook-more-{}", scope.label()))).small().ghost().icon(IconName::ChevronDown).label(format!("显示更多 {} 条", total - COLLAPSED))
                .on_click(cx.listener(move |this, _, _, cx| { this.forms_mut().hooks_more.push(scope); cx.notify(); }))));
        }
    }
    v_flex().w_full().gap_3()
        .child(ext_header(IconName::Webhook, "Hooks", "在工具调用前后、会话开始或结束等事件上自动运行命令。来源:用户 / 项目 / 本地设置。", actions, cx))
        .children(error_banner(wb, cx))
        .children(confirm_banner(wb, cx))
        .when(open, |d| d.child(form(wb, cx)))
        .when(hooks.is_empty(), |d| d.child(ext_empty(IconName::Webhook, if all == 0 { "没有配置 Hook" } else { "没有匹配的 Hook" }, "Hook 可在工具调用前后、会话开始 / 结束等事件上运行命令。", cx)))
        .child(body)
        .into_any_element()
}
