//! 模型服务商页:管理 Anthropic 协议兼容的服务商(地址、密钥、模型清单、测试连接)。
//! 页底嵌入「模型映射」区块(`mapping_page::sections`):模型槽位到服务商模型的映射。
//!
//! @author ixxxxoooo

use super::widgets::{block_row, error_banner, group, note, page_header, row, rows, segmented};
use crate::theme::R_SM;
use crate::ui::{MONO, icon, pill};
use crate::workbench::Workbench;
use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_config::providers::{AuthHeader, PRESETS};

/// 当前服务商的编辑区(一个分组卡片)。
fn detail(wb: &Workbench, cx: &mut Context<Workbench>) -> Option<Div> {
    let t = cx.theme().clone();
    let ui = &wb.forms().providers;
    let p = ui.selected.as_ref().and_then(|id| ui.store.get(id))?.clone();
    let has_key = wb.provider_has_key(&p.id);
    let busy = ui.busy;
    let status = ui.status.clone();
    let default_model = (wb.forms().providers.store.mapping.provider == p.id).then(|| wb.forms().providers.store.mapping.default.clone());
    let models = v_flex().w_full().gap_0p5().children(p.models.iter().enumerate().map(|(i, m)| {
        let (m_del, m_copy, m_star) = (m.clone(), m.clone(), m.clone());
        let is_default = default_model.as_deref() == Some(m.as_str());
        let tool = |id: String, ic: IconName, color: Hsla| div().id(SharedString::from(id)).size(px(22.)).flex().items_center().justify_center().rounded(px(4.)).cursor_pointer()
            .hover(|d| d.bg(t.list_hover)).child(icon(ic, 13., color));
        h_flex().id(SharedString::from(format!("pm-{i}"))).w_full().h(px(28.)).px_2().gap_1().items_center().rounded(px(R_SM)).hover(|d| d.bg(t.muted))
            .child(div().flex_1().min_w_0().font_family(MONO).text_size(px(12.)).truncate().child(m.clone()))
            .child(tool(format!("pm-copy-{i}"), IconName::Copy, t.muted_foreground)
                .on_click(cx.listener(move |_, _, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(m_copy.clone())))))
            .child(tool(format!("pm-star-{i}"), IconName::Star, if is_default { t.warning } else { t.muted_foreground })
                .on_click(cx.listener(move |this, _, _, cx| this.set_default_model(&m_star, cx))))
            .child(tool(format!("pm-del-{i}"), IconName::Trash, t.muted_foreground)
                .on_click(cx.listener(move |this, _, _, cx| this.remove_provider_model(&m_del, cx))))
    }));
    Some(group(&p.name, rows![
        row("名称", "", div().w(px(300.)).child(Input::new(wb.field("p_name")).small()), cx),
        row("Base URL", "Anthropic 协议兼容的地址", div().w(px(300.)).child(Input::new(wb.field("p_url")).small()), cx),
        row("API Key", "保存后不再显示,留空保持原值", h_flex().gap_2().items_center()
            .child(div().w(px(220.)).child(Input::new(wb.field("p_key")).small()))
            .child(if has_key { pill("已保存", t.success, t.muted) } else { pill("未填写", t.warning, t.muted) }), cx),
        row("鉴权方式", "", segmented("p-auth", vec![(AuthHeader::Bearer, "Bearer"), (AuthHeader::XApiKey, "x-api-key")], p.auth, cx,
            |this, v, cx| this.set_provider_auth(v, cx)), cx),
        block_row(&format!("模型清单({})", p.models.len()), "星标设为映射默认模型;复制 ID、删除在行尾;下面输入模型 ID 后回车或点「添加」", v_flex().w_full().gap_2()
            .child(models)
            .child(h_flex().w_full().gap_2()
                .child(div().flex_1().child(Input::new(wb.field("p_model")).small()))
                .child(Button::new("pm-add").small().outline().label("添加").on_click(cx.listener(|this, _, window, cx| this.add_provider_model(window, cx))))), cx),
        block_row("操作", "", v_flex().w_full().gap_2()
            .child(h_flex().w_full().gap_2()
                .child(Button::new("p-save").small().primary().label("保存").on_click(cx.listener(|this, _, window, cx| this.save_provider(window, cx))))
                .child(Button::new("p-test").small().outline().label(if busy { "请求中…" } else { "测试连接" }).disabled(busy).on_click(cx.listener(|this, _, _, cx| this.probe_provider(false, cx))))
                .child(Button::new("p-fetch").small().outline().label("获取模型列表").disabled(busy).on_click(cx.listener(|this, _, _, cx| this.probe_provider(true, cx))))
                .child(div().flex_1())
                .child(Button::new("p-del").small().ghost().icon(IconName::Trash).label("删除").on_click(cx.listener(|this, _, _, cx| this.ask_delete_provider(cx)))))
            .when_some(status, |d, (ok, msg)| d.child(div().text_size(px(12.)).text_color(if ok { t.success } else { t.danger }).child(msg))), cx),
    ], cx))
}

/// 渲染模型服务商页。
pub fn render(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let ui = &wb.forms().providers;
    let status = ui.store.mapping_status(&ui.keys);
    let selected = ui.selected.clone();
    let list: Vec<(String, String, bool)> = ui.store.providers.iter().map(|p| (p.id.clone(), p.name.clone(), wb.provider_has_key(&p.id))).collect();
    v_flex().w_full()
        .child(page_header("模型服务商", "接入 Anthropic 协议兼容的服务(DeepSeek、智谱、Kimi、MiniMax、百炼等),并把模型槽位映射过去", match &status {
            Ok(()) => pill("映射生效中", t.success, t.muted),
            Err(e) => pill(e.clone(), t.muted_foreground, t.muted),
        }, cx))
        .children(error_banner(wb, cx))
        .children(super::confirm::confirm_banner(wb, cx))
        .child(group("服务商", rows![
            block_row("已添加", "", h_flex().w_full().flex_wrap().gap_1().children(list.into_iter().map(|(id, name, has_key)| {
                let on = selected.as_deref() == Some(id.as_str());
                Button::new(SharedString::from(format!("prov-{id}"))).small().when(on, |b| b.primary()).when(!on, |b| b.outline())
                    .label(if has_key { name } else { format!("{name} · 缺密钥") })
                    .on_click(cx.listener(move |this, _, window, cx| this.select_provider(&id, window, cx)))
            })), cx),
            block_row("添加", "预设里的模型名会随服务商更新,以「获取模型列表」为准", h_flex().w_full().flex_wrap().gap_1()
                .children(PRESETS.iter().enumerate().map(|(i, (name, ..))| {
                    Button::new(SharedString::from(format!("preset-{i}"))).xsmall().ghost().icon(IconName::Plus).label(name.to_string())
                        .on_click(cx.listener(move |this, _, window, cx| this.add_provider(Some(i), window, cx)))
                }))
                .child(Button::new("preset-custom").xsmall().ghost().icon(IconName::Plus).label("自定义")
                    .on_click(cx.listener(|this, _, window, cx| this.add_provider(None, window, cx)))), cx),
        ], cx))
        .children(detail(wb, cx))
        .children(super::mapping_page::sections(wb, cx))
        .child(note("配置存 ~/.loomcode/providers.json,API Key 单独存 provider-keys.json(权限 0600),只在启动 claude 时以环境变量注入,不改动 ~/.claude;对之后新建或重新连接的会话生效", cx))
        .into_any_element()
}
