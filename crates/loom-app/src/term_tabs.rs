//! 终端标签条的公共部件:Agent 右侧面板与 IDE 底部面板共用同一套标签外观,
//! 与编辑器文件标签一致(`tab_style`:顶部色条选中态、右分隔线、关闭按钮悬停才显示、可横向滚动)。
//! 两边只负责各自的点击行为与额外按钮。
//!
//! @author ixxxxoooo

use crate::term_panel::clip_label;
use crate::tab_style::{tab_close_box, tab_close_icon, tab_shell, TAB_H};
use gpui_kit::component::{ActiveTheme, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::rc::Rc;

/// 一个终端标签:标题 + 关闭按钮(外观同编辑器标签,中键也能关闭)。
///
/// 参数:`key` 元素 id 前缀(两个面板各用一个,避免冲突);`id` 终端 id;`title` 标题(自动截断);
/// `selected` 是否选中;`exited` shell 是否已退出(标题变灰);`on_select` 点击回调;`on_close` 关闭回调。
/// 返回:可继续链式加子元素的标签。
pub fn term_tab(
    key: &'static str,
    id: u64,
    title: &str,
    selected: bool,
    exited: bool,
    on_select: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&mut Window, &mut App) + 'static,
    cx: &App,
) -> Stateful<Div> {
    closable_tab(SharedString::from(format!("{key}-tab-{id}")), clip_label(title).into_any_element(), selected, exited, on_select, on_close, cx)
}

/// 带关闭按钮的标签(终端标签与只读命令标签共用)。
///
/// 参数:`id` 元素 id;`label` 标签内容;`selected` 是否选中;`dim` 是否变灰;`on_select` / `on_close` 回调。
pub fn closable_tab(
    id: SharedString,
    label: AnyElement,
    selected: bool,
    dim: bool,
    on_select: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&mut Window, &mut App) + 'static,
    cx: &App,
) -> Stateful<Div> {
    let t = cx.theme();
    let group = SharedString::from(format!("{id}-g"));
    let on_close = Rc::new(on_close);
    let (close_click, close_mid) = (on_close.clone(), on_close);
    tab_shell(id.clone(), group.clone(), selected, cx)
        .when(dim && !selected, |d| d.text_color(t.muted_foreground.opacity(0.7)))
        .on_click(on_select)
        .on_mouse_down(MouseButton::Middle, move |_, window, cx| close_mid(window, cx))
        .child(label)
        .child(
            tab_close_box(SharedString::from(format!("{id}-x")), cx)
                .when(!selected, |d| d.invisible().group_hover(group, |d| d.visible()))
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    close_click(window, cx)
                })
                .child(tab_close_icon(cx)),
        )
}

/// 标签条外框:高度同编辑器标签条,底线先画、选中标签盖住它(与下方内容融为一体);标签横向滚动,右侧放固定按钮。
///
/// 参数:`id` 滚动区元素 id;`leading` 标签前的固定内容(如「终端」字样);`tabs` 标签;`trailing` 右侧固定按钮。
pub fn tab_strip(id: &'static str, leading: Vec<AnyElement>, tabs: Vec<AnyElement>, trailing: AnyElement, cx: &App) -> Div {
    let t = cx.theme();
    h_flex().h(px(TAB_H)).w_full().flex_none().items_center().relative().bg(t.sidebar)
        .child(div().absolute().bottom_0().left_0().right_0().h(px(1.)).bg(t.border))
        .children(leading)
        .child(div().id(id).h_full().flex_1().min_w_0().overflow_x_scroll().child(h_flex().h_full().children(tabs)))
        .child(h_flex().h_full().flex_none().px_1().items_center().child(trailing))
}
