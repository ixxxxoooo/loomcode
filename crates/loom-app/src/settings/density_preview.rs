//! 外观页「工具调用详细程度」的示例预览卡(参考 OhMyAgent):随档位切换为
//! 精简摘要 / 标准单行 / 详细内联 Diff 与输出。
//!
//! @author ixxxxoooo

use crate::ui::{MONO, icon};
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::*;
use loom_config::ToolDensity;

/// 预览卡。
///
/// 参数:`density` 当前档位。
pub fn density_preview(density: ToolDensity, cx: &App) -> impl IntoElement {
    let t = cx.theme();
    let (muted, fg, border, bg) = (t.muted_foreground, t.foreground, t.border, t.background);
    // 一行示例:图标 + 动词 + 对象 + 附注
    let sample = move |ic: IconName, verb: &'static str, subject: &'static str, note: &'static str| {
        h_flex().gap_2().items_center().text_size(px(12.))
            .child(div().w(px(16.)).child(icon(ic, 12., muted)))
            .child(div().text_color(muted).child(verb))
            .child(div().text_color(fg).child(subject))
            .child(div().font_family(MONO).text_size(px(11.)).text_color(muted.opacity(0.8)).child(note))
    };
    // 示意条:宽度占整行的比例
    let bar = move |color: Hsla, frac: f32| div().h(px(6.)).w(relative(frac)).rounded(px(3.)).bg(color);
    let bars = move |items: Vec<(Hsla, f32)>| v_flex().gap(px(3.)).pl(px(24.)).pr_2().children(items.into_iter().map(move |(c, f)| bar(c, f)));
    let (removed, added, grey) = (t.danger.opacity(0.22), t.success.opacity(0.22), t.muted);
    let body = match density {
        ToolDensity::Compact => v_flex().child(
            h_flex().gap_2().items_center().text_size(px(12.))
                .child(div().w(px(16.)).child(icon(IconName::Layers, 12., muted)))
                .child(div().text_color(muted).child("编辑了 2 个文件、运行了 2 条命令")),
        ),
        ToolDensity::Standard => v_flex().gap_1p5()
            .child(sample(IconName::FilePen, "编辑", "math.rs", "+4 −1"))
            .child(sample(IconName::FilePen, "编辑", "README.md", "+5 −1"))
            .child(sample(IconName::Terminal, "运行", "focused tests", "cargo test"))
            .child(sample(IconName::Terminal, "运行", "type-check", "cargo check")),
        ToolDensity::Detailed => v_flex().gap_1p5()
            .child(sample(IconName::FilePen, "编辑", "math.rs", "+4 −1"))
            .child(bars(vec![(removed, 1.), (removed, 0.7), (added, 1.), (added, 0.7)]))
            .child(sample(IconName::Terminal, "运行", "focused tests", "cargo test"))
            .child(bars(vec![(grey, 1.), (grey, 0.55)])),
    };
    div().w_full().px_3().pb_3().child(
        v_flex().w_full().p_3().rounded(px(8.)).border_1().border_color(border).bg(bg).child(body),
    )
}
