//! 输入框的「上下文」面板(排版参考 OhMyAgent):
//! 标题行「上下文窗口 已用 / 窗口 token · 百分比」→ 用量条 → 图例 → 分隔线 →
//! 「套餐用量」标题(右侧本会话花费)→ 每个限额一行(名称 / 进度条 / 百分比 / 多久后重置)→ 操作。
//! 限额只在用户点开面板时查询一次,没有数据时说明原因。
//!
//! @author ixxxxoooo

use crate::composer::Composer;
use crate::composer_pop::{ratio_color, short_tokens};
use crate::ui::{MONO, icon};
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::{LimitsState, Usage};

/// 把剩余秒数写成「N 分钟后重置」「N 小时 M 分后重置」「N 天 H 小时后重置」;已过期返回空串。
///
/// 参数:`secs` 距重置还有多少秒。返回:提示文字。
pub fn reset_text(secs: i64) -> String {
    if secs <= 0 {
        return String::new();
    }
    let (mins, hours, days) = (secs / 60, secs / 3600, secs / 86400);
    match secs {
        0..=59 => "不足 1 分钟后重置".to_string(),
        60..=3599 => format!("{mins} 分钟后重置"),
        3600..=86399 => format!("{hours} 小时 {} 分后重置", mins % 60),
        _ => format!("{days} 天 {} 小时后重置", hours % 24),
    }
}

/// 当前 Unix 秒。
fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// 一条圆角用量条:`fraction`(0..=1)为已用部分,其余是轨道色。
fn strip(fraction: f32, color: Hsla, height: f32, cx: &App) -> Div {
    let t = cx.theme();
    let f = fraction.clamp(0., 1.);
    div().h(px(height)).w_full().rounded_full().bg(t.muted).overflow_hidden()
        .child(div().h_full().w(relative(f)).min_w(px(if f > 0. { 2. } else { 0. })).bg(color))
}

/// 一行限额:左名称 / 中进度条 / 百分比 / 右重置提示;≥70% 警示色,≥90% 危险色。
fn limit_row(label: &'static str, pct: f32, resets: Option<i64>, cx: &App) -> Div {
    let t = cx.theme();
    let ratio = (pct / 100.).clamp(0., 1.);
    let color = if ratio >= 0.9 { t.danger } else if ratio >= 0.7 { t.warning } else { t.primary };
    let note = resets.map(|r| reset_text(r - now_secs())).unwrap_or_default();
    h_flex().w_full().gap_2().items_center().text_size(px(12.))
        .child(div().w(px(84.)).flex_none().text_color(t.muted_foreground).child(label))
        .child(div().flex_1().min_w_0().child(strip(ratio, color, 4., cx)))
        .child(div().w(px(40.)).flex_none().text_right().font_family(MONO).text_color(if ratio >= 0.9 { color } else { t.foreground }).child(format!("{:.0}%", pct)))
        .child(div().w(px(108.)).flex_none().text_right().text_size(px(11.)).text_color(t.muted_foreground).child(note))
}

/// 上下文面板。
///
/// 参数:`usage` 当前用量;`this` 输入框实体(操作回调用)。返回:绝对定位在输入框上方的面板。
pub fn context_popup(usage: &Usage, this: Entity<Composer>, cx: &App) -> Div {
    let t = cx.theme().clone();
    let ratio = usage.context_ratio();
    let known = usage.context_size > 0;
    let action = |id: &'static str, ic: IconName, label: &'static str, tip: &'static str, f: Box<dyn Fn(&mut Window, &mut App)>| {
        h_flex().id(id).gap_2().px_2().h(px(30.)).items_center().rounded(px(6.)).cursor_pointer().hover(|d| d.bg(t.list_hover))
            .on_click(move |_, window, cx| f(window, cx))
            .child(icon(ic, 13., t.muted_foreground))
            .child(div().text_size(px(13.)).child(label))
            .child(div().flex_1())
            .child(div().text_size(px(11.)).text_color(t.muted_foreground).child(tip))
    };
    let (a, b) = (this.clone(), this);
    let free = usage.context_size.saturating_sub(usage.context_used);
    let legend = |label: &'static str, tokens: u64, color: Option<Hsla>| {
        h_flex().gap_1p5().items_center().text_size(px(12.))
            .when_some(color, |d, c| d.child(div().size(px(9.)).rounded(px(2.)).bg(c)))
            .child(div().text_color(t.muted_foreground).child(label))
            .child(div().font_family(MONO).child(short_tokens(tokens)))
    };
    let no_limits = usage.five_hour_pct.is_none() && usage.weekly_pct.is_none();
    let note = match usage.limits_state {
        LimitsState::Checking => "正在获取额度…",
        LimitsState::Unavailable => "暂时获取不到额度(第三方服务商或离线),稍后重新打开再试。",
        LimitsState::Idle => "额度在打开此面板时获取一次。",
    };
    v_flex().absolute().bottom(px(44.)).right(px(8.)).w(px(460.)).max_w_full().px_4().py_3().gap_2p5().rounded(px(10.)).border_1().border_color(t.border).bg(t.popover).shadow_lg()
        .child(h_flex().gap_2().items_baseline()
            .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child("上下文窗口"))
            .child(div().text_size(px(12.)).text_color(t.muted_foreground).child(if known {
                format!("{} / {} token · {:.0}%", short_tokens(usage.context_used), short_tokens(usage.context_size), ratio * 100.)
            } else {
                "发送消息后统计".to_string()
            })))
        .child(strip(ratio, ratio_color(ratio, cx), 6., cx))
        .when(known, |d| d.child(h_flex().gap_4().child(legend("已使用", usage.context_used, Some(t.primary))).child(legend("剩余", free, None))))
        .child(div().h(px(1.)).bg(t.border))
        .child(h_flex().items_baseline()
            .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child("套餐用量"))
            .child(div().flex_1())
            .child(h_flex().gap_1p5().text_size(px(12.))
                .child(div().text_color(t.muted_foreground).child("本次会话"))
                .child(div().font_family(MONO).child(format!("{:.4} USD", usage.cost_usd)))))
        .when_some(usage.five_hour_pct, |d, p| d.child(limit_row("5 小时限额", p, usage.five_hour_resets, cx)))
        .when_some(usage.weekly_pct, |d, p| d.child(limit_row("每周限额", p, usage.weekly_resets, cx)))
        .when(no_limits, |d| d.child(div().text_size(px(11.)).text_color(t.muted_foreground).child(note)))
        .child(div().h(px(1.)).bg(t.border))
        .child(v_flex()
            .child(action("ctx-compact", IconName::Minimize2, "压缩上下文", "发送 /compact", Box::new(move |window, cx| { a.update(cx, |c, cx| c.compact(window, cx)); })))
            .child(action("ctx-new", IconName::SquarePen, "新会话继续", "⌘N", Box::new(move |window, cx| { b.update(cx, |c, cx| c.close_pop(cx)); window.dispatch_action(Box::new(crate::workbench::NewSession), cx); }))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 重置提示() {
        assert_eq!(reset_text(0), "");
        assert_eq!(reset_text(30), "不足 1 分钟后重置");
        assert_eq!(reset_text(46 * 60), "46 分钟后重置");
        assert_eq!(reset_text(3 * 3600 + 20 * 60), "3 小时 20 分后重置");
        assert_eq!(reset_text(2 * 86400 + 5 * 3600), "2 天 5 小时后重置");
    }
}
