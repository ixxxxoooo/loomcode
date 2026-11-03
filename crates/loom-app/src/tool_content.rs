//! 工具卡展开后的内容块:文本、终端输出、Diff、图片(从 `tool_card` 拆出)。
//!
//! @author ixxxxoooo

use crate::ansi;
use crate::diff;
use crate::ui::{MONO, icon, mono_box, pill};
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::*;

/// 限制输出行数,避免一次渲染过长文本。
fn clip_lines(text: &str, max: usize) -> (String, usize) {
    let total = text.lines().count();
    if total <= max {
        return (text.to_string(), 0);
    }
    (text.lines().take(max).collect::<Vec<_>>().join("\n"), total - max)
}

/// 可选中复制的等宽文本:加入窗口级的文字选择(可与会话流里其它文字连选,⌘C 复制),语法色 / ANSI 色照常显示。
fn selectable(id: SharedString, text: String, runs: Vec<(std::ops::Range<usize>, HighlightStyle)>) -> gpui_base::SelectableText {
    gpui_base::SelectableText::new(id, text).highlights(runs)
}

/// 选择状态的元素 id:有稳定的 id 就用它(流式输出变长时选择不丢),否则退回内容哈希。
fn sel_id(kind: &str, run_id: Option<&str>, fold: &Option<(SharedString, crate::code_box::Fold, bool)>, text: &str) -> SharedString {
    if let Some((id, ..)) = fold {
        return SharedString::from(format!("{id}-{kind}-sel"));
    }
    if let Some(r) = run_id {
        return SharedString::from(format!("{r}-{kind}-sel"));
    }
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    SharedString::from(format!("{kind}-{:x}-sel", h.finish()))
}

/// 渲染一个内容块。
/// 参数:`run_id` 所属工具调用的 id;终端块用它显示「在终端面板查看」;`lang` 文本内容的语言(`text` 不着色);
/// `fold` 限高滚动用的 (元素 id, 展开状态, 是否运行中贴底跟随),`None` 时不限高(只按行数截断)。
pub fn render_content(content: &ToolContent, run_id: Option<&str>, lang: &str, fold: Option<(SharedString, crate::code_box::Fold, bool)>, cx: &App) -> AnyElement {
    let t = cx.theme();
    match content {
        ToolContent::Text(text) => {
            let (shown, more) = clip_lines(text, if fold.is_some() { 2000 } else { 60 });
            let total = shown.lines().count();
            let styled = selectable(sel_id("text", run_id, &fold, &shown), shown.clone(), crate::code_hl::styles(&shown, lang, cx));
            let body = match &fold {
                Some((id, f, follow)) => crate::code_box::fold_body(id.clone(), styled, total, Some(f), *follow, cx),
                None => styled.into_any_element(),
            };
            mono_box(cx)
                .child(body)
                .when(more > 0, |d| d.child(div().mt_1().text_color(t.muted_foreground).child(format!("… 还有 {more} 行"))))
                .into_any_element()
        }
        ToolContent::Terminal { command, output, exit_code } => {
            let (shown, more) = clip_lines(output, if fold.is_some() { 2000 } else { 80 });
            let parsed = ansi::parse(&shown);
            let total = shown.lines().count();
            let out_el = {
                let styled = selectable(sel_id("term", run_id, &fold, &shown), parsed.text, parsed.runs);
                match &fold {
                    Some((id, f, follow)) => crate::code_box::fold_body(id.clone(), styled, total, Some(f), *follow, cx),
                    None => styled.into_any_element(),
                }
            };
            mono_box(cx)
                .child(
                    h_flex().gap_2().mb_1().items_start()
                        // 长命令在卡片内折行,不再把右侧的状态与「在终端面板查看」挤出去;
                        // 命令第一行与右侧状态簇同高(22),垂直居中对齐,折行后右侧仍贴着第一行
                        .child(div().flex_1().min_w_0().line_height(px(22.)).text_color(t.muted_foreground).child(crate::code_hl::shell_text(command.lines().next().unwrap_or(""), "$ ", cx)))
                        .child(h_flex().flex_none().h(px(22.)).gap_2().items_center()
                        .when_some(*exit_code, |d, code| {
                            let (fg, bg) = if code == 0 { (t.success, t.background) } else { (t.danger, t.background) };
                            d.child(div().flex_none().child(pill(format!("退出码 {code}"), fg, bg)))
                        })
                        .when_some(run_id.map(str::to_string), |d, id| {
                            // Agent 窗口直接打开右栏终端面板;IDE 窗口转给工作台并提示
                            d.child(
                                div().id(SharedString::from(format!("view-run-{id}"))).flex_none().cursor_pointer().tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new("在终端面板查看").build(window, cx))
                                    .hover(|s| s.opacity(0.7))
                                    .on_click(move |_, window, cx| window.dispatch_action(Box::new(crate::workbench::ViewAgentRun { id: id.clone() }), cx))
                                    .child(icon(IconName::Terminal, 14., t.muted_foreground)),
                            )
                        })),
                )
                .child(out_el)
                .when(more > 0, |d| d.child(div().mt_1().text_color(t.muted_foreground).child(format!("… 还有 {more} 行"))))
                .into_any_element()
        }
        ToolContent::Diff { path, old, new } => {
            let (lines, added, removed) = diff::compute(old, new);
            v_flex()
                .w_full()
                .gap_1()
                .child(
                    h_flex().gap_2().text_size(px(12.)).font_family(MONO)
                        .child({
                            // 点击路径:在编辑器里打开(IDE 窗口由 IdeWindow 处理,Agent 窗口由 Workbench 处理)
                            let target = path.clone();
                            div().id(SharedString::from(format!("diff-path-{path}"))).cursor_pointer().text_color(t.muted_foreground)
                                .hover(|s| s.text_color(t.primary).underline())
                                .on_click(move |_, window, cx| window.dispatch_action(Box::new(crate::ide_window::OpenPath { path: target.clone() }), cx))
                                .child(path.clone())
                        })
                        .child(div().text_color(t.success).child(format!("+{added}")))
                        .child(div().text_color(t.danger).child(format!("−{removed}"))),
                )
                .child({
                    let el = diff::render(&lines, 120, crate::file_tree::language_for(std::path::Path::new(path)), cx);
                    match &fold {
                        Some((id, f, _)) => crate::code_box::fold_body(id.clone(), el, lines.len().min(120), Some(f), false, cx),
                        None => el.into_any_element(),
                    }
                })
                .into_any_element()
        }
        ToolContent::Image { label, path: Some(path) } if std::path::Path::new(path).is_file() => v_flex()
            .gap_1()
            .child(img(std::path::PathBuf::from(path)).max_w_full().max_h(px(360.)).rounded(px(8.)).border_1().border_color(t.border))
            .child(div().text_size(px(12.)).text_color(t.muted_foreground).child(label.clone()))
            .into_any_element(),
        ToolContent::Image { label, .. } => h_flex()
            .gap_2()
            .items_center()
            .p_2()
            .rounded(px(8.))
            .border_1()
            .border_color(t.border)
            .bg(t.muted)
            .child(
                div().size(px(56.)).rounded(px(6.)).bg(t.background).flex().items_center().justify_center()
                    .child(icon(IconName::Image, 22., t.muted_foreground)),
            )
            .child(div().text_size(px(12.)).text_color(t.muted_foreground).child(label.clone()))
            .into_any_element(),
    }
}
