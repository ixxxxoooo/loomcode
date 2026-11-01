//! 外观页「工具调用详细程度」的示例预览卡(参考 OhMyAgent):照着会话里真实的样子画,
//! 随档位切换为精简摘要 / 标准单行 / 详细内联 Diff 与输出。
//!
//! @author ixxxxoooo

use crate::ui::{MONO, icon};
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_config::ToolDensity;

/// 预览卡(含与上方设置行的间距)。
///
/// 参数:`density` 当前档位。
pub fn density_preview(density: ToolDensity, cx: &App) -> impl IntoElement {
    let t = cx.theme();
    let (muted, fg, success, danger) = (t.muted_foreground, t.foreground, t.success, t.danger);
    // 一行工具调用:图标 + 标题 + (diff 统计) + 完成勾 + 展开箭头
    let tool = move |ic: IconName, title: &'static str, stats: Option<(&'static str, &'static str)>, open: bool| {
        h_flex().h(px(26.)).w_full().gap_2().items_center().text_size(px(12.5))
            .child(icon(ic, 13., muted))
            .child(div().flex_1().min_w_0().truncate().text_color(fg).child(title))
            .when_some(stats, |d, (a, r)| {
                d.child(h_flex().gap_1().font_family(MONO).text_size(px(11.)).child(div().text_color(success).child(a)).child(div().text_color(danger).child(r)))
            })
            .child(icon(IconName::Check, 12., success))
            .child(icon(if open { IconName::ChevronDown } else { IconName::ChevronRight }, 11., muted))
    };
    // 内联 Diff / 输出块:缩进对齐到标题,圆角浅底
    let block = move |bg: Hsla| v_flex().ml(px(21.)).mb_1().rounded(px(6.)).overflow_hidden().bg(bg).font_family(MONO).text_size(px(11.));
    let line = move |bg: Hsla, color: Hsla, text: &'static str| div().w_full().px_2().py(px(2.)).bg(bg).text_color(color).child(text);
    let body = match density {
        ToolDensity::Compact => v_flex().child(
            h_flex().h(px(26.)).gap_2().items_center().text_size(px(12.5))
                .child(icon(IconName::Layers, 13., muted))
                .child(div().text_color(muted).child("编辑了 2 个文件、运行了 2 条命令"))
                .child(icon(IconName::ChevronRight, 11., muted)),
        ),
        ToolDensity::Standard => v_flex()
            .child(tool(IconName::FilePen, "编辑 src/router.rs", Some(("+4", "−1")), false))
            .child(tool(IconName::FilePen, "编辑 README.md", Some(("+5", "−1")), false))
            .child(tool(IconName::Terminal, "cargo test router", None, false))
            .child(tool(IconName::Terminal, "cargo check", None, false)),
        ToolDensity::Detailed => v_flex()
            .child(tool(IconName::FilePen, "编辑 src/router.rs", Some(("+4", "−1")), true))
            .child(block(t.muted.opacity(0.6))
                .child(line(danger.opacity(0.14), fg, "−  if route.path == path {"))
                .child(line(success.opacity(0.16), fg, "+  if route.path.eq_ignore_ascii_case(path) {")))
            .child(tool(IconName::Terminal, "cargo test router", None, true))
            .child(block(t.muted.opacity(0.6)).child(line(t.muted.opacity(0.0), muted, "test result: ok. 14 passed"))),
    };
    div().w_full().px(px(14.)).pb(px(12.)).child(
        v_flex().w_full().px_3().py_2().rounded(px(8.)).border_1().border_color(t.border.opacity(0.8)).bg(t.muted.opacity(0.35)).child(body),
    )
}
