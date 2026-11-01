//! 输入框里的两个弹层:
//! - 模型弹层:上面选模型,下面是思考强度的分段条(强度与模型放在一起,因为互相影响)和当前档位说明;
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

/// 模型弹层。
///
/// 参数:`models` 可选模型;`current` 当前模型 id;`effort` 当前强度;`this` 输入框实体(回调用)。
pub fn model_popup(models: &[ModelInfo], current: &str, effort: Effort, this: Entity<Composer>, cx: &App) -> Div {
    let t = cx.theme().clone();
    let list = models.iter().enumerate().map(|(i, m)| {
        let (this, id, on) = (this.clone(), m.id.clone(), m.id == current);
        h_flex().id(SharedString::from(format!("pick-model-{i}"))).gap_2().px_2().h(px(30.)).items_center().rounded(px(6.)).cursor_pointer()
            .hover(|d| d.bg(t.list_hover))
            .on_click(move |_, _, cx| { let id = id.clone(); this.update(cx, |c, cx| c.choose_model(&id, cx)); })
            .child(div().w(px(14.)).child(if on { icon(IconName::Check, 12., t.primary).into_any_element() } else { div().into_any_element() }))
            .child(crate::brand_icons::brand_icon(&m.id, &m.name, 14., if on { t.foreground } else { t.muted_foreground }))
            .child(div().text_size(px(13.)).child(m.name.clone()))
            .child(div().flex_1())
            .child(div().text_size(px(11.)).text_color(t.muted_foreground).child(m.description.clone()))
    });
    let segments = h_flex().gap_0p5().p_0p5().rounded(px(8.)).bg(t.muted).children(Effort::ALL.into_iter().map(|e| {
        let (this, on) = (this.clone(), e == effort);
        div().id(SharedString::from(format!("pick-effort-{}", e.cli_value()))).flex_1().h(px(26.)).flex().items_center().justify_center().rounded(px(6.)).cursor_pointer().text_size(px(12.))
            .when(on, |d| d.bg(t.background).text_color(t.foreground).shadow_sm())
            .when(!on, |d| d.text_color(t.muted_foreground).hover(|d| d.bg(t.list_hover)))
            .on_click(move |_, _, cx| { this.update(cx, |c, cx| c.choose_effort(e, cx)); })
            .child(e.label())
    }));
    v_flex().absolute().bottom(px(44.)).left(px(8.)).w(px(300.)).p_1().rounded(px(10.)).border_1().border_color(t.border).bg(t.popover).shadow_lg()
        .children(list)
        .child(div().h(px(1.)).my_1().bg(t.border))
        .child(div().px_2().pt_1().pb_0p5().text_size(px(11.)).text_color(t.muted_foreground).child("思考强度"))
        .child(div().px_1().child(segments))
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
        .when_some(usage.five_hour_pct, |d, p| d.child(row("5 小时限额", format!("{p:.0}%"))))
        .when_some(usage.weekly_pct, |d, p| d.child(row("每周限额", format!("{p:.0}%"))))
        .when(usage.five_hour_pct.is_none() && usage.weekly_pct.is_none(), |d| {
            let note = match usage.limits_state {
                loom_core::LimitsState::Checking => "正在查询账户限额…",
                loom_core::LimitsState::Unavailable => "账户限额不可用(第三方服务商或离线)",
                loom_core::LimitsState::Idle => "打开此面板时查询账户限额",
            };
            d.child(div().text_size(px(11.)).text_color(t.muted_foreground).child(note))
        })
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
