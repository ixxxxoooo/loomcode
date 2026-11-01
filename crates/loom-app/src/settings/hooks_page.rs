//! 钩子页:新增 / 编辑 / 删除 command 类型钩子;http、prompt 等其它类型只读显示,可删除。
//!
//! @author ixxxxoooo

use super::claude_pages::scope_pill;
use super::widgets::{error_banner, page_title, scope_header};
use crate::ui::{MONO, caption, pill};
use crate::workbench::Workbench;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_config::HookEntry;
use loom_config::claude::HOOK_EVENTS;

/// 新增 / 编辑表单。
fn form(wb: &Workbench, cx: &mut Context<Workbench>) -> Div {
    let t = cx.theme().clone();
    let f = &wb.forms().hook;
    let cur = f.event;
    v_flex().w_full().mt_3().p_3().gap_2().rounded(px(10.)).border_1().border_color(t.primary.opacity(0.5))
        .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(if f.editing.is_some() { "编辑钩子" } else { "添加钩子" }))
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

/// 一张钩子卡片。
fn hook_card(i: usize, h: HookEntry, cx: &mut Context<Workbench>) -> Div {
    let t = cx.theme().clone();
    let editable = h.kind == "command";
    let (h_edit, h_del) = (h.clone(), h.clone());
    v_flex().w_full().p_3().gap_1().rounded(px(10.)).border_1().border_color(t.border)
        .child(h_flex().gap_2().items_center()
            .child(pill(h.event.clone(), t.primary, t.muted))
            .when(!h.matcher.is_empty(), |d| d.child(div().font_family(MONO).text_size(px(12.)).text_color(t.muted_foreground).child(format!("匹配 {}", h.matcher))))
            .when(!editable, |d| d.child(pill(h.kind.clone(), t.muted_foreground, t.muted)))
            .when_some(h.timeout, |d, s| d.child(caption(format!("超时 {s}s"), cx)))
            .child(div().flex_1())
            .child(scope_pill(h.scope, cx))
            .when(editable, |d| d.child(Button::new(SharedString::from(format!("hook-edit-{i}"))).xsmall().ghost().icon(IconName::Pencil)
                .on_click(cx.listener(move |this, _, window, cx| this.open_hook_form(Some(&h_edit), window, cx)))))
            .child(Button::new(SharedString::from(format!("hook-del-{i}"))).xsmall().ghost().icon(IconName::Trash)
                .on_click(cx.listener(move |this, _, _, cx| this.delete_hook(&h_del, cx)))))
        .child(div().font_family(MONO).text_size(px(12.5)).child(h.command.clone()))
}

/// 渲染钩子页。参数:`filter` 搜索词(小写,空为不过滤),按事件、匹配器与命令匹配。
pub fn hooks(wb: &Workbench, filter: &str, cx: &mut Context<Workbench>) -> AnyElement {
    let hooks: Vec<_> = wb.claude_cfg.hooks.iter().filter(|h| filter.is_empty() || format!("{} {} {}", h.event, h.matcher, h.command).to_lowercase().contains(filter)).cloned().collect();
    let open = wb.forms().hook.open;
    let target = match wb.scope_settings_file(wb.effective_scope()) {
        Some(path) => format!("新增写入 {}", path.display()),
        None => "先打开一个项目会话".to_string(),
    };
    v_flex().w_full()
        .child(h_flex().w_full().items_center()
            .child(div().flex_1().child(page_title("钩子")))
            .child(Button::new("hook-add").small().primary().icon(IconName::Plus).label("添加")
                .on_click(cx.listener(|this, _, window, cx| this.open_hook_form(None, window, cx)))))
        .child(caption("在工具调用前后、提交提示词、会话结束等时机自动运行的命令", cx))
        .child(scope_header(wb, target, cx))
        .children(error_banner(wb, cx))
        .when(open, |d| d.child(form(wb, cx)))
        .child(v_flex().w_full().pt_4().gap_2()
            .when(hooks.is_empty(), |d| d.child(caption("没有配置钩子", cx)))
            .children(hooks.into_iter().enumerate().map(|(i, h)| hook_card(i, h, cx))))
        .into_any_element()
}
