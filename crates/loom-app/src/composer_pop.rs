//! 输入框里的两个弹层:
//! - 模型弹层:上面按服务商分组选模型(选别家的模型即切换服务商),下面是思考强度的分段条(强度与模型放在一起,因为互相影响)和当前档位说明;
//! - 上下文弹层:点右下角的圆环打开,显示用量、花费,并提供「压缩」「新会话继续」。
//!
//! @author ixxxxoooo

use crate::composer::Composer;
use crate::composer_models::Picker;
use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::progress::ProgressCircle;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::{Effort, ModelInfo, Usage};

/// 把 token 数缩写成 `12.3K` / `1.2M`。
pub fn short_tokens(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{:.1}K", n as f64 / 1_000.),
        _ => format!("{:.1}M", n as f64 / 1_000_000.),
    }
}

/// 上下文占用对应的颜色等级:超过 90% 危险,超过 75% 警告,其余次要色。
pub fn ratio_color(ratio: f32, cx: &App) -> Hsla {
    let t = cx.theme();
    if ratio > 0.9 {
        t.danger
    } else if ratio > 0.75 {
        t.warning
    } else {
        t.muted_foreground
    }
}

/// 输入框右下角的上下文圆环(不带百分比文字,数字在面板里),点击打开上下文弹层。
pub fn context_ring(usage: &Usage, on_click: impl Fn(&mut App) + 'static, cx: &App) -> impl IntoElement {
    let ratio = usage.context_ratio();
    let color = ratio_color(ratio, cx);
    h_flex().id("chip-context").h(px(24.)).px_1p5().gap_1p5().items_center().rounded(px(6.)).cursor_pointer().text_size(px(11.)).text_color(color)
        .hover(|d| d.bg(cx.theme().list_hover))
        .on_click(move |_, _, cx| on_click(cx))
        .child(ProgressCircle::new("ctx-ring").value(ratio * 100.).color(color).size(px(14.)))
}

/// 模型弹层:按服务商分组(Claude 官方在前),底部是思考强度。
///
/// 参数:`picker` 分组数据;`running` 回合是否进行中(进行中不能切换服务商);`effort` 当前强度;`this` 输入框实体(回调用)。
pub fn model_popup(picker: &Picker, running: bool, effort: Effort, this: Entity<Composer>, cx: &App) -> Div {
    let t = cx.theme().clone();
    let mut list: Vec<AnyElement> = Vec::new();
    // 每组:组头(服务商名 + 状态)+ 模型行;不是当前服务商时点模型 = 切换服务商
    let push_group = |list: &mut Vec<AnyElement>, gid: Option<String>, title: String, models: Vec<ModelInfo>, blocked: Option<String>| {
        let is_active = gid == picker.active;
        let block = blocked.clone().or_else(|| (running && !is_active).then(|| "回合进行中,结束后可切换".to_string()));
        let key = gid.clone().unwrap_or_else(|| "claude".into());
        let header = h_flex().id(SharedString::from(format!("pick-group-{key}"))).px_2().pt_1p5().pb_0p5().gap_1().items_center().text_size(px(11.)).text_color(t.muted_foreground)
            .child(div().flex_1().child(title))
            .when(is_active, |d| d.child(div().text_color(t.primary).child("使用中")))
            .when(!is_active, |d| match (&blocked, &block) {
                // 缺配置:点组头去设置页;回合进行中:只是提示
                (Some(why), _) => d.cursor_pointer().on_click(|_, _, cx| crate::composer_models::open_settings(cx))
                    .child(div().text_color(t.warning).child(format!("{why} · 去设置"))),
                (None, Some(why)) => d.child(div().child(why.clone())),
                _ => d,
            });
        list.push(header.into_any_element());
        for m in models {
            let on = is_active && m.id == picker.current;
            let enabled = is_active || block.is_none();
            let (this, id, gid) = (this.clone(), m.id.clone(), gid.clone());
            let row = h_flex().id(SharedString::from(format!("pick-model-{key}-{}", m.id))).gap_2().px_2().h(px(30.)).items_center().rounded(px(6.))
                .when(enabled, |d| d.cursor_pointer().hover(|d| d.bg(t.list_hover)).on_click(move |_, _, cx| {
                    let (id, gid) = (id.clone(), gid.clone());
                    this.update(cx, |c, cx| if is_active { c.choose_model(&id, cx) } else { c.choose_provider_model(gid, id, cx) });
                }))
                .when(!enabled, |d| d.opacity(0.45))
                .child(div().w(px(14.)).child(if on { icon(IconName::Check, 12., t.primary).into_any_element() } else { div().into_any_element() }))
                .child(crate::brand_icons::brand_icon(&m.id, &m.name, 14., if on { t.foreground } else { t.muted_foreground }))
                .child(div().text_size(px(13.)).truncate().child(m.name.clone()))
                .child(div().flex_1())
                .child(div().text_size(px(11.)).text_color(t.muted_foreground).child(m.description.clone()));
            list.push(row.into_any_element());
        }
    };
    push_group(&mut list, None, "Claude 官方".into(), picker.official.clone(), None);
    for g in &picker.groups {
        let models = g.models.iter().map(|m| ModelInfo::new(m, m, "")).collect();
        push_group(&mut list, Some(g.id.clone()), g.name.clone(), models, g.blocked.clone());
    }
    let segments = h_flex().gap_0p5().p_0p5().rounded(px(8.)).bg(t.muted).children(Effort::ALL.into_iter().map(|e| {
        let (this, on) = (this.clone(), e == effort);
        div().id(SharedString::from(format!("pick-effort-{}", e.cli_value()))).flex_1().h(px(26.)).flex().items_center().justify_center().rounded(px(6.)).cursor_pointer().text_size(px(12.))
            .when(on, |d| d.bg(t.background).text_color(t.foreground).shadow_sm())
            .when(!on, |d| d.text_color(t.muted_foreground).hover(|d| d.bg(t.list_hover)))
            .on_click(move |_, _, cx| { this.update(cx, |c, cx| c.choose_effort(e, cx)); })
            .child(e.label())
    }));
    v_flex().absolute().bottom(px(44.)).left(px(8.)).w(px(300.)).p_1().rounded(px(10.)).border_1().border_color(t.border).bg(t.popover).shadow_lg()
        .child(v_flex().id("pick-model-list").max_h(px(380.)).overflow_y_scroll().children(list))
        .child(div().h(px(1.)).my_1().bg(t.border))
        .child(div().px_2().pt_1().pb_0p5().text_size(px(11.)).text_color(t.muted_foreground).child("思考强度"))
        .child(div().px_1().child(segments))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 缩写_token_数() {
        assert_eq!(short_tokens(0), "0");
        assert_eq!(short_tokens(999), "999");
        assert_eq!(short_tokens(12_345), "12.3K");
        assert_eq!(short_tokens(1_250_000), "1.2M");
    }
}
