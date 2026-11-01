//! 图片预览(灯箱)与可复用的缩略图:点击缩略图弹出大图,输入框里的缩略图悬停时右上角有删除按钮。
//!
//! @author ixxxxoooo

use gpui_kit::component::{ActiveTheme, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::path::PathBuf;

use crate::ui::icon;
use gpui_kit::assets::IconName;

/// 弹出图片预览对话框。
///
/// 参数:`path` 图片文件路径;`name` 标题。
pub fn preview_image(path: &str, name: &str, window: &mut Window, cx: &mut App) {
    let (path, name) = (PathBuf::from(path), name.to_string());
    window.open_dialog(cx, move |dialog, _, _| {
        let p = path.clone();
        dialog.title(name.clone()).w(px(760.)).child(
            div().w_full().max_h(px(560.)).flex().items_center().justify_center().child(img(p).max_w_full().max_h(px(540.)).object_fit(ObjectFit::Contain)),
        )
    });
}

/// 方形缩略图:点击预览;`on_remove` 非空时悬停显示右上角删除按钮。
///
/// 参数:`id` 元素 id 前缀;`path` 图片;`name` 预览标题;`size` 边长;`on_remove` 删除回调。
pub fn thumbnail(id: &str, path: &str, name: &str, size: f32, on_remove: Option<Box<dyn Fn(&mut Window, &mut App) + 'static>>, cx: &App) -> AnyElement {
    let t = cx.theme();
    let group = SharedString::from(format!("thumb-g-{id}"));
    let (p, n) = (path.to_string(), name.to_string());
    div().id(SharedString::from(format!("thumb-{id}"))).group(group.clone()).relative().size(px(size)).flex_none().rounded(px(8.)).border_1().border_color(t.border).cursor_pointer()
        .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("点击预览").build(window, cx))
        .on_click(move |_, window, cx| preview_image(&p, &n, window, cx))
        .child(div().size_full().rounded(px(7.)).overflow_hidden().child(img(PathBuf::from(path)).size_full().object_fit(ObjectFit::Cover)))
        .when_some(on_remove, |d, remove| {
            d.child(
                div().id(SharedString::from(format!("thumb-x-{id}"))).absolute().top(px(3.)).right(px(3.)).size(px(18.)).flex().items_center().justify_center().rounded_full()
                    .bg(t.foreground.opacity(0.78)).invisible().group_hover(group, |s| s.visible())
                    .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("移除图片").build(window, cx))
                    // 点删除不触发预览
                    .on_click(move |_, window, cx| { cx.stop_propagation(); remove(window, cx); })
                    .child(icon(IconName::X, 10., t.background)),
            )
        })
        .into_any_element()
}
