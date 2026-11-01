//! 输入框里的两个弹层:
//! - 模型弹层(级联):左列选模型,悬停后右列展开该模型的思考强度(带档位说明),点强度同时选定两者;
//! - 上下文弹层:点右下角的圆环打开,显示用量、花费,并提供「压缩」「新会话继续」。
//!
//! @author ixxxxoooo

use crate::composer::Composer;
use crate::ui::{MONO, icon};
use gpui_kit::assets::IconName;
use gpui_kit::component::progress::ProgressCircle;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ucode_core::{Effort, ModelInfo, Usage};

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

/// 输入框右下角的上下文圆环 + 百分比,点击打开上下文弹层。
pub fn context_ring(usage: &Usage, on_click: impl Fn(&mut App) + 'static, cx: &App) -> impl IntoElement {
    let ratio = usage.context_ratio();
    let color = ratio_color(ratio, cx);
    h_flex().id("chip-context").h(px(24.)).px_1p5().gap_1p5().items_center().rounded(px(6.)).cursor_pointer().text_size(px(11.)).text_color(color)
        .hover(|d| d.bg(cx.theme().list_hover))
        .on_click(move |_, _, cx| on_click(cx))
        .child(ProgressCircle::new("ctx-ring").value(ratio * 100.).color(color).size(px(14.)))
        .child(format!("{:.0}%", ratio * 100.))
}

/// 模型弹层(级联):左列是模型,悬停某个模型,右列展开它的思考强度,点强度同时选定两者。
///
/// 参数:`models` 可选模型;`current` 当前模型 id;`hovered` 悬停中的模型 id(没有则跟随当前模型);
/// `effort` 当前强度;`this` 输入框实体(回调用)。
pub fn model_popup(models: &[ModelInfo], current: &str, hovered: Option<&str>, effort: Effort, this: Entity<Composer>, cx: &App) -> Div {
    let t = cx.theme().clone();
    let active = hovered.unwrap_or(current).to_string();
    let list = models.iter().enumerate().map(|(i, m)| {
        let (hv, id, on, shown) = (this.clone(), m.id.clone(), m.id == current, m.id == active);
        let id2 = id.clone();
        h_flex().id(SharedString::from(format!("pick-model-{i}"))).gap_2().px_2().h(px(30.)).items_center().rounded(px(6.)).cursor_pointer()
            .when(shown, |d| d.bg(t.list_hover))
            .on_hover(move |hovering, _, cx| { if *hovering { let id = id2.clone(); hv.update(cx, |c, cx| c.hover_model(&id, cx)); } })
            .child(div().w(px(14.)).child(if on { icon(IconName::Check, 12., t.primary).into_any_element() } else { div().into_any_element() }))
            .child(div().text_size(px(13.)).child(m.name.clone()))
            .child(div().flex_1())
            .child(icon(IconName::ChevronRight, 12., t.muted_foreground))
    });
    let efforts = Effort::ALL.into_iter().map(|e| {
        let (this, id, on) = (this.clone(), active.clone(), e == effort && active == current);
        h_flex().id(SharedString::from(format!("pick-effort-{}", e.cli_value()))).gap_2().px_2().h(px(30.)).items_center().rounded(px(6.)).cursor_pointer()
            .hover(|d| d.bg(t.list_hover))
            .on_click(move |_, _, cx| { let id = id.clone(); this.update(cx, |c, cx| c.choose_model_effort(&id, e, cx)); })
            .child(div().w(px(14.)).child(if on { icon(IconName::Check, 12., t.primary).into_any_element() } else { div().into_any_element() }))
            .child(div().w(px(36.)).text_size(px(13.)).child(e.label()))
            .child(div().text_size(px(11.)).text_color(t.muted_foreground).child(e.hint()))
    });
    let panel = || v_flex().p_1().rounded(px(10.)).border_1().border_color(t.border).bg(t.popover).shadow_lg();
    h_flex().absolute().bottom(px(44.)).left(px(8.)).items_start().gap_1()
        .child(panel().w(px(180.)).children(list))
        .child(panel().w(px(340.)).child(div().px_2().pt_1().pb_0p5().text_size(px(11.)).text_color(t.muted_foreground).child("思考强度")).children(efforts))
}

/// 上下文弹层。
///
/// 参数:`usage` 当前用量;`this` 输入框实体。
pub fn context_popup(usage: &Usage, this: Entity<Composer>, cx: &App) -> Div {
    let t = cx.theme().clone();
    let ratio = usage.context_ratio();
    let row = |label: &'static str, value: String| {
        h_flex().justify_between().text_size(px(12.)).child(div().text_color(t.muted_foreground).child(label)).child(div().font_family(MONO).child(value))
    };
    let action = |id: &'static str, ic: IconName, label: &'static str, tip: &'static str, f: Box<dyn Fn(&mut Window, &mut App)>| {
        h_flex().id(id).gap_2().px_2().h(px(30.)).items_center().rounded(px(6.)).cursor_pointer().hover(|d| d.bg(t.list_hover))
            .on_click(move |_, window, cx| f(window, cx))
            .child(icon(ic, 13., t.muted_foreground))
            .child(div().text_size(px(13.)).child(label))
            .child(div().flex_1())
            .child(div().text_size(px(11.)).text_color(t.muted_foreground).child(tip))
    };
    let (a, b) = (this.clone(), this);
    v_flex().absolute().bottom(px(44.)).right(px(8.)).w(px(280.)).p_2().gap_1().rounded(px(10.)).border_1().border_color(t.border).bg(t.popover).shadow_lg()
        .child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).child("上下文"))
        .child(row("已用", format!("{} / {}", short_tokens(usage.context_used), short_tokens(usage.context_size))))
        .child(row("占比", format!("{:.0}%", ratio * 100.)))
        .child(row("本会话花费", format!("${:.2}", usage.cost_usd)))
        .child(div().h(px(1.)).my_1().bg(t.border))
        .child(action("ctx-compact", IconName::Minimize2, "压缩上下文", "发送 /compact", Box::new(move |window, cx| { a.update(cx, |c, cx| c.compact(window, cx)); })))
        .child(action("ctx-new", IconName::SquarePen, "新会话继续", "⌘N", Box::new(move |window, cx| { b.update(cx, |c, cx| c.close_pop(cx)); window.dispatch_action(Box::new(crate::workbench::NewSession), cx); })))
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
