//! 模型映射页:把 Claude Code 的模型槽位(默认 / Opus / Sonnet / Haiku / 子 Agent)映射到某个服务商的模型。
//!
//! @author ixxxxoooo

use super::widgets::{block_row, group, note, page_header, row, rows};
use crate::ui::pill;
use crate::workbench::Workbench;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_config::providers::SLOTS;

/// 渲染模型映射页。
pub fn render(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let ui = &wb.forms().providers;
    let status = ui.store.mapping_status(&ui.keys);
    let m = ui.store.mapping.clone();
    let providers: Vec<(String, String)> = ui.store.providers.iter().map(|p| (p.id.clone(), p.name.clone())).collect();
    let models = ui.store.get(&m.provider).map(|p| p.models.clone()).unwrap_or_default();
    let slot_rows: Vec<AnyElement> = SLOTS.iter().map(|(slot, label, var)| {
        let slot = *slot;
        let cur = m.slot(slot).to_string();
        block_row(label, var, h_flex().flex_wrap().gap_1()
            .child(Button::new(SharedString::from(format!("slot-{slot}-none"))).xsmall().when(cur.is_empty(), |b| b.primary()).when(!cur.is_empty(), |b| b.ghost()).label("不设置")
                .on_click(cx.listener(move |this, _, _, cx| this.edit_mapping(cx, |m| m.set_slot(slot, String::new())))))
            .children(models.iter().enumerate().map(|(i, model)| {
                let on = *model == cur;
                let v = model.clone();
                Button::new(SharedString::from(format!("slot-{slot}-{i}"))).xsmall().when(on, |b| b.primary()).when(!on, |b| b.ghost()).label(model.clone())
                    .on_click(cx.listener(move |this, _, _, cx| { let v = v.clone(); this.edit_mapping(cx, |m| m.set_slot(slot, v)) }))
            })), cx).into_any_element()
    }).collect();
    v_flex().w_full()
        .child(page_header("模型映射", "让 Claude Code 通过所选服务商的模型运行", match &status {
            Ok(()) => pill("映射生效中", t.success, t.muted),
            Err(e) => pill(e.clone(), t.muted_foreground, t.muted),
        }, cx))
        .child(group("映射", rows![
            row("启用映射", "开启后新会话通过所选服务商运行;关闭则使用 Claude 官方登录,映射内容保留", Switch::new("sw-mapping").checked(m.enabled)
                .on_click(cx.listener(|this, c: &bool, _, cx| { let v = *c; this.edit_mapping(cx, |m| m.enabled = v) })), cx),
            block_row("服务商", "在「模型服务商」页添加与配置", h_flex().flex_wrap().gap_1().children(providers.into_iter().map(|(id, name)| {
                let on = id == m.provider;
                Button::new(SharedString::from(format!("map-p-{id}"))).xsmall().when(on, |b| b.primary()).when(!on, |b| b.ghost()).label(name)
                    .on_click(cx.listener(move |this, _, _, cx| { let id = id.clone(); this.edit_mapping(cx, |m| m.provider = id) }))
            })), cx),
        ], cx))
        .child(group("模型槽位", slot_rows, cx))
        .child(note("映射以环境变量(ANTHROPIC_MODEL 等)在启动 claude 时注入,不改动 ~/.claude;对之后新建或重新连接的会话生效", cx))
        .into_any_element()
}
