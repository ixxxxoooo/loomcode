//! 用量页:根据本机 Claude 会话记录统计 token、回复、工具调用与提问,含 30 天趋势与按模型拆分。
//!
//! @author ixxxxoooo

use super::widgets::{page_title, section_title, segmented};
use crate::ui::{MONO, caption};
use crate::workbench::Workbench;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::*;
use loom_claude::usage::{DayTotal, UsageReport};

/// 数字缩写:1.2K / 3.4M / 5.6B。
pub fn short(n: u64) -> String {
    match n {
        n if n >= 1_000_000_000 => format!("{:.1}B", n as f64 / 1e9),
        n if n >= 1_000_000 => format!("{:.1}M", n as f64 / 1e6),
        n if n >= 1_000 => format!("{:.1}K", n as f64 / 1e3),
        n => n.to_string(),
    }
}

/// 指标卡。
fn stat(label: &str, value: String, cx: &App) -> Div {
    let t = cx.theme();
    v_flex().flex_1().min_w(px(140.)).p_3().gap_1().rounded(px(10.)).border_1().border_color(t.border)
        .child(div().text_size(px(12.)).text_color(t.muted_foreground).child(label.to_string()))
        .child(div().text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).child(value))
}

/// 趋势指标。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Metric {
    Tokens,
    Replies,
    Tools,
    Prompts,
}

fn metric_value(d: &DayTotal, m: Metric) -> u64 {
    match m {
        Metric::Tokens => d.tokens,
        Metric::Replies => d.replies,
        Metric::Tools => d.tools,
        Metric::Prompts => d.prompts,
    }
}

/// 30 天柱状图。
fn chart(r: &UsageReport, m: Metric, cx: &App) -> Div {
    let t = cx.theme();
    let max = r.days.iter().map(|d| metric_value(d, m)).max().unwrap_or(0).max(1);
    v_flex().w_full().gap_1()
        .child(h_flex().w_full().h(px(140.)).items_end().gap(px(3.)).children(r.days.iter().map(|d| {
            let v = metric_value(d, m);
            let h = (v as f32 / max as f32 * 132.).max(if v > 0 { 2. } else { 0. });
            div().flex_1().h(px(h)).rounded_t(px(2.)).bg(if v > 0 { t.primary } else { t.muted })
        })))
        .child(h_flex().w_full().justify_between().font_family(MONO).text_size(px(10.5)).text_color(t.muted_foreground)
            .children(r.days.iter().step_by(7).chain(r.days.last()).map(|d| div().child(d.date[5..].to_string()))))
}

/// 渲染用量页。
pub fn render(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let ops = &wb.forms().ops;
    let busy = ops.usage_busy;
    let header = h_flex().w_full().items_center()
        .child(div().flex_1().child(page_title("用量")))
        .child(Button::new("usage-refresh").small().outline().label(if busy { "统计中…" } else { "刷新" }).disabled(busy).on_click(cx.listener(|this, _, _, cx| this.refresh_usage(cx))));
    let Some(r) = ops.usage.clone() else {
        return v_flex().w_full().child(header).child(caption("正在扫描 ~/.claude/projects 里的会话记录…", cx)).into_any_element();
    };
    let week = r.recent(7);
    let metric = wb.usage_metric;
    let max_model = r.by_model.iter().map(|m| m.1 + m.2 + m.3 + m.4).max().unwrap_or(1).max(1);
    v_flex().w_full()
        .child(header)
        .child(caption(format!("来自 {} 个会话文件(含子 Agent);日期按 UTC。只统计 token,不估算费用;单个会话的花费见输入框旁的上下文圆环", r.files), cx))
        .child(section_title("近 7 天", cx))
        .child(h_flex().w_full().flex_wrap().gap_2()
            .child(stat("提问", short(week.prompts), cx))
            .child(stat("活跃会话", short(week.sessions), cx))
            .child(stat("工具调用", short(week.tools), cx))
            .child(stat("Token", short(week.tokens), cx)))
        .child(section_title("累计", cx))
        .child(h_flex().w_full().flex_wrap().gap_2()
            .child(stat("输入", short(r.total_input), cx))
            .child(stat("输出", short(r.total_output), cx))
            .child(stat("缓存读取", short(r.total_cache_read), cx))
            .child(stat("缓存命中率", format!("{:.0}%", r.cache_hit_rate() * 100.), cx)))
        .child(h_flex().w_full().items_center().pt_5().pb_2()
            .child(div().flex_1().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(t.muted_foreground).child("最近 30 天"))
            .child(segmented("usage-metric", vec![(Metric::Tokens, "Token"), (Metric::Prompts, "提问"), (Metric::Replies, "回复"), (Metric::Tools, "工具调用")], metric, cx,
                |this, v, cx| { this.usage_metric = v; cx.notify(); })))
        .child(chart(&r, metric, cx))
        .child(section_title("按模型", cx))
        .children(r.by_model.iter().map(|(model, input, output, cr, cw, replies)| {
            let total = input + output + cr + cw;
            v_flex().w_full().py_2().gap_1().border_b_1().border_color(t.border)
                .child(h_flex().w_full().gap_2()
                    .child(div().flex_1().font_family(MONO).text_size(px(12.5)).child(model.clone()))
                    .child(caption(format!("{} 次回复 · 输入 {} · 输出 {} · 缓存 {}", short(*replies), short(*input), short(*output), short(cr + cw)), cx)))
                .child(div().w_full().h(px(6.)).rounded_full().bg(t.muted)
                    .child(div().h_full().rounded_full().bg(t.primary).w(relative(total as f32 / max_model as f32))))
        }))
        .into_any_element()
}
