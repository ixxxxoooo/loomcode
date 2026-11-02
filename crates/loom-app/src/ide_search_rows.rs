//! 搜索面板的行与小控件:开关小方块、悬停操作图标、文件行(折叠箭头 + 图标 + 数量徽标)、
//! 匹配行(高亮命中、替换预览)。
//!
//! @author ixxxxoooo

use crate::file_icons::file_icon_img;
use crate::ide_search::{Scope, SearchPanel};
use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// 结果列表的统一行高(虚拟列表要求等高)。
pub(crate) const ROW_H: f32 = 22.;

/// 输入框里的开关小方块(Aa / ab / .* / AB):开启时有主题色底与描边。
///
/// 参数:`id` 元素 id;`label` 文字;`tip` 悬停提示;`on` 是否开启;`underline` 文字加下划线(全词的「ab」);
/// `click` 点击处理。
pub(crate) fn chip(id: &'static str, label: &'static str, tip: &'static str, on: bool, underline: bool, cx: &App, click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> AnyElement {
    let t = cx.theme();
    div().id(id).flex_none().w(px(24.)).h(px(22.)).flex().items_center().justify_center().rounded(px(4.)).border_1().cursor_pointer()
        .text_size(px(13.))
        .when(underline, |d| d.underline())
        .when(on, |d| d.bg(t.primary.opacity(0.2)).border_color(t.primary).text_color(t.foreground))
        .when(!on, |d| d.border_color(gpui_kit::gpui::transparent_black()).text_color(t.muted_foreground).hover(|d| d.bg(t.list_hover)))
        .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
        .on_click(click)
        .child(label)
        .into_any_element()
}

/// 行内悬停操作图标(替换 / 忽略):点击不触发所在行的点击。
fn action_icon(id: SharedString, ic: IconName, tip: &'static str, cx: &App, click: impl Fn(&mut Window, &mut App) + 'static) -> AnyElement {
    let t = cx.theme();
    div().id(id).flex_none().size(px(18.)).flex().items_center().justify_center().rounded(px(4.)).cursor_pointer().hover(|d| d.bg(t.border))
        .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            click(window, cx);
        })
        .child(crate::ui::icon(ic, 12., t.muted_foreground))
        .into_any_element()
}

/// 文件行。
///
/// 参数:`this` 面板;`fi` 文件下标;`cx` 面板上下文。
pub(crate) fn file_row(this: &SearchPanel, fi: usize, cx: &mut Context<SearchPanel>) -> AnyElement {
    let t = cx.theme().clone();
    let f = &this.files[fi];
    let name = f.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let dir = f.path.parent().and_then(|p| p.strip_prefix(&this.root).ok()).map(|p| p.display().to_string()).unwrap_or_default();
    let collapsed = this.collapsed.contains(&f.path);
    let group = SharedString::from(format!("sr-fg-{fi}"));
    let weak = cx.weak_entity();
    let (w1, w2) = (weak.clone(), weak);
    h_flex().id(SharedString::from(format!("sr-file-{fi}"))).group(group.clone()).h(px(ROW_H)).w_full().pl_2().pr_2().gap_1p5().items_center().cursor_pointer().text_size(px(13.))
        .hover(|d| d.bg(t.list_hover))
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_file(fi, cx)))
        .child(crate::ui::icon(if collapsed { IconName::ChevronRight } else { IconName::ChevronDown }, 12., t.muted_foreground))
        .child(file_icon_img(&name, false, false, 14.))
        .child(div().flex_none().max_w(px(180.)).truncate().child(name))
        .child(div().flex_1().min_w_0().truncate().text_size(px(11.)).text_color(t.muted_foreground).child(dir))
        .child(
            h_flex().flex_none().gap_0p5().invisible().group_hover(group, |d| d.visible())
                .when(this.replace_open, |d| d.child(action_icon(SharedString::from(format!("sr-frep-{fi}")), IconName::Replace, "替换文件中的全部匹配", cx, move |window, cx| { let _ = w1.update(cx, |p, cx| p.start_replace(Scope::File(fi), window, cx)); })))
                .child(action_icon(SharedString::from(format!("sr-fx-{fi}")), IconName::X, "忽略这个文件", cx, move |_, cx| { let _ = w2.update(cx, |p, cx| p.dismiss_file(fi, cx)); })),
        )
        .child(div().flex_none().min_w(px(18.)).h(px(16.)).px_1p5().rounded_full().bg(t.primary).text_color(t.primary_foreground).text_size(px(10.5)).flex().items_center().justify_center().child(f.matches.len().to_string()))
        .into_any_element()
}

/// 匹配行的文字:命中处高亮;替换栏展开且有替换文本时,命中处显示为「删除线旧文 + 绿底新文」。
fn preview_text(this: &SearchPanel, text: &str, hl: std::ops::Range<usize>, cx: &App) -> AnyElement {
    let t = cx.theme();
    let rep = this.replace_input.read(cx).value().to_string();
    if this.replace_open && !rep.is_empty() {
        let new_range = hl.end..hl.end + rep.len();
        let shown = format!("{}{rep}{}", &text[..hl.end], &text[hl.end..]);
        let old = HighlightStyle { background_color: Some(t.danger.opacity(0.25)), strikethrough: Some(StrikethroughStyle { thickness: px(1.), color: Some(t.danger) }), ..Default::default() };
        let new = HighlightStyle { background_color: Some(t.success.opacity(0.25)), ..Default::default() };
        StyledText::new(shown).with_highlights([(hl, old), (new_range, new)]).into_any_element()
    } else {
        let hit = HighlightStyle { background_color: Some(t.warning.opacity(0.35)), ..Default::default() };
        StyledText::new(text.to_string()).with_highlights([(hl, hit)]).into_any_element()
    }
}

/// 匹配行。参数:`fi` 文件下标;`mi` 该文件里的匹配下标。
pub(crate) fn match_row(this: &SearchPanel, fi: usize, mi: usize, cx: &mut Context<SearchPanel>) -> AnyElement {
    let t = cx.theme().clone();
    let f = &this.files[fi];
    let m = &f.matches[mi];
    let active = this.active.as_ref().is_some_and(|(p, l, c)| *p == f.path && *l == m.line && *c == m.start);
    let group = SharedString::from(format!("sr-mg-{fi}-{mi}"));
    let weak = cx.weak_entity();
    let (w1, w2) = (weak.clone(), weak);
    h_flex().id(SharedString::from(format!("sr-match-{fi}-{mi}"))).group(group.clone()).h(px(ROW_H)).w_full().pl(px(36.)).pr_2().gap_1().items_center().cursor_pointer().text_size(px(12.5))
        .when(active, |d| d.bg(t.list_active))
        .hover(|d| d.bg(t.list_hover))
        .on_click(cx.listener(move |this, _, _, cx| this.open_match(fi, mi, cx)))
        .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().child(preview_text(this, &m.preview.text, m.preview.hl.clone(), cx)))
        .child(
            h_flex().flex_none().gap_0p5().invisible().group_hover(group, |d| d.visible())
                .when(this.replace_open, |d| d.child(action_icon(SharedString::from(format!("sr-rep-{fi}-{mi}")), IconName::Replace, "替换", cx, move |window, cx| { let _ = w1.update(cx, |p, cx| p.start_replace(Scope::One(fi, mi), window, cx)); })))
                .child(action_icon(SharedString::from(format!("sr-x-{fi}-{mi}")), IconName::X, "忽略", cx, move |_, cx| { let _ = w2.update(cx, |p, cx| p.dismiss_match(fi, mi, cx)); })),
        )
        .into_any_element()
}
