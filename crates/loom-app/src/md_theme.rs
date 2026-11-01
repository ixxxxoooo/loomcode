//! 富文本(Markdown)的全局样式:在库按主题装好的默认样式上,把行内代码改成跟随主题强调色
//! (主色文字 + 主色淡底),而不是库默认的灰底。每次应用主题后调用 `install` 重装一次。
//!
//! 只替换样式,库装好的代码块语法高亮与「继承容器文字色」设置原样保留。
//!
//! @author ixxxxoooo

use gpui_base::{TextViewDefaults, TextViewStyle};
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;

/// 行内代码的样式:主题主色文字 + 主色淡底。
///
/// 参数:`cx` 取当前主题。返回:高亮样式。
pub fn inline_code(cx: &App) -> HighlightStyle {
    let primary = cx.theme().primary;
    HighlightStyle { color: Some(primary), background_color: Some(primary.opacity(0.12)), ..Default::default() }
}

/// 按当前主题重装富文本默认样式(与库的 `base_text_view_style` 同构,只改行内代码)。
pub fn install(cx: &mut App) {
    let t = cx.theme().clone();
    let radius = t.radius;
    let mut code_block = StyleRefinement::default();
    code_block.corner_radii.top_left = Some(radius.into());
    code_block.corner_radii.top_right = Some(radius.into());
    code_block.corner_radii.bottom_left = Some(radius.into());
    code_block.corner_radii.bottom_right = Some(radius.into());
    let table = code_block.clone();
    let style = TextViewStyle::default()
        .with_foreground(t.foreground)
        .with_muted_foreground(t.muted_foreground)
        .with_link(t.link)
        .with_selection(t.selection)
        .with_code_background(t.muted)
        .with_border(t.border)
        .with_code_block(code_block)
        .with_table(table)
        .with_table_head(StyleRefinement::default().bg(t.table_head).text_color(t.table_head_foreground))
        .with_inline_code(inline_code(cx))
        .with_dark(t.is_dark());
    TextViewDefaults::global(cx).with_style(style).install(cx);
}
