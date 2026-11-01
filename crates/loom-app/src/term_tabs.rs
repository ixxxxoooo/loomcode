//! 终端标签条的公共部件:Agent 右侧面板与 IDE 底部面板共用同一个标签样式与关闭按钮,
//! 两边只负责各自的点击行为与外框(高度、背景、额外按钮)。
//!
//! @author ixxxxoooo

use crate::term_panel::clip_label;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// 一个终端标签:标题 + 关闭按钮。
///
/// 参数:`key` 元素 id 前缀(两个面板各用一个,避免冲突);`id` 终端 id;`title` 标题(自动截断);
/// `selected` 是否选中;`exited` shell 是否已退出(标题变灰);`height` 标签高度;
/// `on_select` / `on_close` 点击回调(关闭按钮已自动阻止冒泡)。返回:可继续链式加子元素的标签。
pub fn term_tab(
    key: &'static str,
    id: u64,
    title: &str,
    selected: bool,
    exited: bool,
    height: f32,
    on_select: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Stateful<Div> {
    let t = cx.theme();
    let (active_bg, hover_bg, muted) = (t.list_active, t.list_hover, t.muted_foreground);
    div().id(SharedString::from(format!("{key}-tab-{id}"))).h(px(height)).px_2().flex().items_center().gap_1().rounded(px(6.)).cursor_pointer().text_size(px(12.))
        .when(selected, |d| d.bg(active_bg).font_weight(FontWeight::MEDIUM))
        .when(!selected, |d| d.text_color(muted).hover(move |d| d.bg(hover_bg)))
        .when(exited, |d| d.text_color(muted))
        .on_click(on_select)
        .child(clip_label(title))
        .child(
            Button::new(SharedString::from(format!("{key}-close-{id}"))).xsmall().ghost().icon(IconName::X).tooltip("关闭终端")
                .on_click(move |e, w, cx| {
                    cx.stop_propagation();
                    on_close(e, w, cx)
                }),
        )
}
