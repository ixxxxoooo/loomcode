//! 模型服务商页:管理 Anthropic 协议兼容的服务商(地址、密钥、模型清单、测试连接),
//! 并把 Claude Code 的模型槽位映射到某个服务商的模型。
//!
//! @author ixxxxoooo

use super::widgets::{error_banner, page_title, row, section_title, segmented};
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
use loom_config::providers::{AuthHeader, PRESETS, SLOTS};

/// 表单里的一行:左标签 + 右内容。
fn field(label: &str, content: impl IntoElement) -> Div {
    h_flex().w_full().gap_3().items_center().child(div().w(px(72.)).flex_none().text_size(px(12.5)).child(label.to_string())).child(div().flex_1().child(content))
}

/// 当前服务商的编辑区。
fn detail(wb: &Workbench, cx: &mut Context<Workbench>) -> Option<Div> {
    let t = cx.theme().clone();
    let ui = &wb.forms().providers;
    let p = ui.selected.as_ref().and_then(|id| ui.store.get(id))?.clone();
    let has_key = wb.provider_has_key(&p.id);
    let busy = ui.busy;
    let status = ui.status.clone();
    Some(v_flex().w_full().mt_3().p_3().gap_2().rounded(px(10.)).border_1().border_color(t.border)
        .child(field("名称", Input::new(wb.field("p_name")).small()))
        .child(field("Base URL", Input::new(wb.field("p_url")).small()))
        .child(field("API Key", h_flex().gap_2().items_center()
            .child(div().flex_1().child(Input::new(wb.field("p_key")).small()))
            .child(if has_key { pill("已保存", t.success, t.muted) } else { pill("未填写", t.warning, t.muted) })))
        .child(field("鉴权方式", segmented("p-auth", vec![(AuthHeader::Bearer, "Bearer(多数兼容服务)"), (AuthHeader::XApiKey, "x-api-key(Anthropic 官方)")], p.auth, cx,
            |this, v, cx| this.set_provider_auth(v, cx))))
        .child(h_flex().w_full().gap_2().pt_1()
            .child(Button::new("p-save").small().primary().label("保存").on_click(cx.listener(|this, _, window, cx| this.save_provider(window, cx))))
            .child(Button::new("p-test").small().outline().label(if busy { "请求中…" } else { "测试连接" }).disabled(busy).on_click(cx.listener(|this, _, _, cx| this.probe_provider(false, cx))))
            .child(Button::new("p-fetch").small().outline().label("获取模型列表").disabled(busy).on_click(cx.listener(|this, _, _, cx| this.probe_provider(true, cx))))
            .child(div().flex_1())
            .child(Button::new("p-del").small().ghost().icon(IconName::Trash).label("删除").on_click(cx.listener(|this, _, _, cx| this.delete_provider(cx)))))
        .when_some(status, |d, (ok, msg)| d.child(div().text_size(px(12.)).text_color(if ok { t.success } else { t.danger }).child(msg)))
        .child(div().pt_2().text_size(px(12.5)).child(format!("模型清单({})", p.models.len())))
        .child(h_flex().w_full().flex_wrap().gap_1().children(p.models.iter().enumerate().map(|(i, m)| {
            let m2 = m.clone();
            h_flex().id(SharedString::from(format!("pm-{i}"))).h(px(24.)).pl_2().pr_1().gap_1().items_center().rounded(px(5.)).bg(t.muted)
                .child(div().font_family(MONO).text_size(px(12.)).child(m.clone()))
                .child(div().id(SharedString::from(format!("pm-del-{i}"))).p_0p5().rounded(px(4.)).cursor_pointer().hover(|d| d.bg(t.list_hover))
                    .on_click(cx.listener(move |this, _, _, cx| this.remove_provider_model(&m2, cx)))
                    .child(icon(IconName::X, 11., t.muted_foreground)))
        })))
        .child(h_flex().w_full().gap_2()
            .child(div().flex_1().child(Input::new(wb.field("p_model")).small()))
            .child(Button::new("pm-add").small().outline().label("添加").on_click(cx.listener(|this, _, window, cx| this.add_provider_model(window, cx))))))
}

/// 模型映射区。
fn mapping(wb: &Workbench, cx: &mut Context<Workbench>) -> Div {
    let t = cx.theme().clone();
    let ui = &wb.forms().providers;
    let m = ui.store.mapping.clone();
    let providers: Vec<(String, String)> = ui.store.providers.iter().map(|p| (p.id.clone(), p.name.clone())).collect();
    let models = ui.store.get(&m.provider).map(|p| p.models.clone()).unwrap_or_default();
    v_flex().w_full()
        .child(section_title("模型映射", cx))
        .child(row("启用映射", "开启后新会话通过所选服务商运行;关闭则使用 Claude 官方登录,映射内容保留", Switch::new("sw-mapping").checked(m.enabled)
            .on_click(cx.listener(|this, c: &bool, _, cx| { let v = *c; this.edit_mapping(cx, |m| m.enabled = v) })), cx))
        .child(v_flex().w_full().py_3().gap_2().border_b_1().border_color(t.border)
            .child(div().text_size(px(13.5)).child("服务商"))
            .child(h_flex().flex_wrap().gap_1().children(providers.into_iter().map(|(id, name)| {
                let on = id == m.provider;
                Button::new(SharedString::from(format!("map-p-{id}"))).xsmall().when(on, |b| b.primary()).when(!on, |b| b.ghost()).label(name)
                    .on_click(cx.listener(move |this, _, _, cx| { let id = id.clone(); this.edit_mapping(cx, |m| m.provider = id) }))
            }))))
        .children(SLOTS.iter().map(|(slot, label, var)| {
            let slot = *slot;
            let cur = m.slot(slot).to_string();
            v_flex().w_full().py_2().gap_1().border_b_1().border_color(t.border)
                .child(h_flex().gap_2().items_baseline().child(div().text_size(px(13.)).child(label.to_string())).child(caption(var.to_string(), cx)))
                .child(h_flex().flex_wrap().gap_1()
                    .child(Button::new(SharedString::from(format!("slot-{slot}-none"))).xsmall().when(cur.is_empty(), |b| b.primary()).when(!cur.is_empty(), |b| b.ghost()).label("不设置")
                        .on_click(cx.listener(move |this, _, _, cx| this.edit_mapping(cx, |m| m.set_slot(slot, String::new())))))
                    .children(models.iter().enumerate().map(|(i, model)| {
                        let on = *model == cur;
                        let v = model.clone();
                        Button::new(SharedString::from(format!("slot-{slot}-{i}"))).xsmall().when(on, |b| b.primary()).when(!on, |b| b.ghost()).label(model.clone())
                            .on_click(cx.listener(move |this, _, _, cx| { let v = v.clone(); this.edit_mapping(cx, |m| m.set_slot(slot, v)) }))
                    })))
        }))
}

/// 渲染模型服务商页。
pub fn render(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let ui = &wb.forms().providers;
    let status = ui.store.mapping_status(&ui.keys);
    let selected = ui.selected.clone();
    let list: Vec<(String, String, bool)> = ui.store.providers.iter().map(|p| (p.id.clone(), p.name.clone(), wb.provider_has_key(&p.id))).collect();
    v_flex().w_full()
        .child(h_flex().w_full().items_center().gap_2()
            .child(div().flex_1().child(page_title("模型服务商")))
            .child(match &status {
                Ok(()) => pill("映射生效中", t.success, t.muted),
                Err(e) => pill(e.clone(), t.muted_foreground, t.muted),
            }))
        .child(caption("接入 Anthropic 协议兼容的服务(DeepSeek、智谱、Kimi、MiniMax、百炼等)。配置存 ~/.loomcode/providers.json,API Key 单独存 provider-keys.json(权限 0600),只在启动 claude 时以环境变量注入,不改动 ~/.claude。对之后新建或重新连接的会话生效", cx))
        .children(error_banner(wb, cx))
        .child(section_title("服务商", cx))
        .child(h_flex().w_full().flex_wrap().gap_1().children(list.into_iter().map(|(id, name, has_key)| {
            let on = selected.as_deref() == Some(id.as_str());
            Button::new(SharedString::from(format!("prov-{id}"))).small().when(on, |b| b.primary()).when(!on, |b| b.outline())
                .label(if has_key { name } else { format!("{name} · 缺密钥") })
                .on_click(cx.listener(move |this, _, window, cx| this.select_provider(&id, window, cx)))
        })))
        .child(h_flex().w_full().flex_wrap().gap_1().pt_2().items_center()
            .child(caption("添加:", cx))
            .children(PRESETS.iter().enumerate().map(|(i, (name, ..))| {
                Button::new(SharedString::from(format!("preset-{i}"))).xsmall().ghost().icon(IconName::Plus).label(name.to_string())
                    .on_click(cx.listener(move |this, _, window, cx| this.add_provider(Some(i), window, cx)))
            }))
            .child(Button::new("preset-custom").xsmall().ghost().icon(IconName::Plus).label("自定义")
                .on_click(cx.listener(|this, _, window, cx| this.add_provider(None, window, cx)))))
        .children(detail(wb, cx))
        .child(mapping(wb, cx))
        .into_any_element()
}
