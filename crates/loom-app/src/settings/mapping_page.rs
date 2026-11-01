//! 模型映射区块(嵌在「模型服务商」页底部):把 Claude Code 的模型槽位(默认 / Opus / Sonnet / Haiku / 子 Agent)映射到某个服务商的模型。
//!
//! @author ixxxxoooo

use super::widgets::{group, item, row, rows};
use crate::workbench::Workbench;
use gpui_kit::component::button::Button;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::Sizable;
use gpui_kit::*;
use loom_config::providers::SLOTS;

/// 渲染模型映射区块(当前服务商 + 模型槽位)。
pub fn sections(wb: &Workbench, cx: &mut Context<Workbench>) -> Vec<AnyElement> {
    let ui = &wb.forms().providers;
    let m = ui.store.mapping.clone();
    let providers: Vec<(String, String)> = ui.store.providers.iter().map(|p| (p.id.clone(), p.name.clone())).collect();
    let slot_provider = ui.store.active_id().unwrap_or(&m.provider).to_string();
    let models = ui.store.get(&slot_provider).map(|p| p.models.clone()).unwrap_or_default();
    let wb_weak = cx.entity().downgrade();
    // 每个槽位一行:左边名称(环境变量名放悬停提示里,不再占一行),右边一个下拉显示当前选择,选项不再每行重复铺开
    let slot_rows: Vec<AnyElement> = SLOTS.iter().map(|(slot, label, var)| {
        let slot = *slot;
        let cur = m.slot(slot).to_string();
        let shown = if cur.is_empty() { "不设置".to_string() } else { cur.clone() };
        let (w, models, current) = (wb_weak.clone(), models.clone(), cur.clone());
        let menu = move |menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>| {
            let w0 = w.clone();
            let mut menu = menu.min_w(px(200.)).item(PopupMenuItem::new("不设置").checked(current.is_empty()).on_click(move |_, _, cx| {
                let _ = w0.update(cx, |this, cx| this.edit_mapping(cx, |m| m.set_slot(slot, String::new())));
            }));
            for model in &models {
                let (w1, v) = (w.clone(), model.clone());
                menu = menu.item(PopupMenuItem::new(model.clone()).checked(*model == current).on_click(move |_, _, cx| {
                    let (v, w1) = (v.clone(), w1.clone());
                    let _ = w1.update(cx, |this, cx| this.edit_mapping(cx, |m| m.set_slot(slot, v)));
                }));
            }
            menu
        };
        let tip = var.to_string();
        item(cx).py(px(4.)).justify_between()
            .child(div().id(SharedString::from(format!("slot-name-{slot}"))).text_size(px(13.)).tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)).child(label.to_string()))
            .child(Button::new(SharedString::from(format!("slot-{slot}"))).small().outline().label(shown).dropdown_caret(true).dropdown_menu(menu))
            .into_any_element()
    }).collect();
    vec![
        div().child(group("当前服务商", rows![{
            // Claude 官方登录也是一个选项,不再有单独的「启用映射」开关;切换会让已打开的会话在下一条消息时换上新服务商
            let w = wb_weak.clone();
            let active = ui.store.active_id().map(str::to_string);
            let cur_name = ui.store.active_name().map(str::to_string).unwrap_or_else(|| "Claude 官方登录".into());
            let list = providers.clone();
            let menu = move |menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>| {
                let w0 = w.clone();
                let mut menu = menu.min_w(px(220.)).item(PopupMenuItem::new("Claude 官方登录").checked(active.is_none()).on_click(move |_, _, cx| {
                    let _ = w0.update(cx, |this, cx| this.switch_provider(None, None, cx));
                }));
                menu = menu.separator();
                for (id, name) in &list {
                    let (w1, id1) = (w.clone(), id.clone());
                    menu = menu.item(PopupMenuItem::new(name.clone()).checked(active.as_deref() == Some(id.as_str())).on_click(move |_, _, cx| {
                        let id1 = id1.clone();
                        let _ = w1.update(cx, |this, cx| this.switch_provider(Some(id1), None, cx));
                    }));
                }
                menu
            };
            row("使用", "也可以在输入框的模型选择器里直接切换", Button::new("map-provider").small().outline().label(cur_name).dropdown_caret(true).dropdown_menu(menu), cx)
        }], cx)).into_any_element(),
        div().child(group("模型槽位", slot_rows, cx)).into_any_element(),
    ]
}
