//! Markdown 代码块右上角的操作区:语言标签 + 复制图标(悬停高亮并提示,点击复制,1.5 秒内显示对勾)。
//!
//! @author ixxxxoooo

use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{ActiveTheme, h_flex};
use gpui_kit::*;

/// 给 Markdown 视图的所有围栏代码块挂上复制按钮。
///
/// 参数:`view` Markdown 视图;返回:带操作区的视图。
pub fn with_copy(view: TextView) -> TextView {
    view.code_block_actions(|block, window, cx| {
        let code = block.code().to_string();
        let lang = block.lang().unwrap_or_default();
        let key = SharedString::from(format!("copy-{}-{}", lang, code.len()));
        let done = window.use_keyed_state(key.clone(), cx, |_, _| false);
        let copied = *done.read(cx);
        let t = cx.theme();
        let (fg, hover_bg) = (t.muted_foreground, t.list_hover);
        let ok = t.success;
        h_flex().items_center().gap_1().pl_2().pr_1().h(px(22.))
            .child(div().text_size(px(11.)).text_color(fg).child(lang))
            .child(
                div().id(key).size(px(20.)).flex().items_center().justify_center().rounded(px(4.)).cursor_pointer()
                    .hover(move |d| d.bg(hover_bg))
                    .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(if copied { "已复制" } else { "复制代码" }).build(window, cx))
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(code.clone()));
                        done.update(cx, |v, cx| { *v = true; cx.notify(); });
                        let d = done.clone();
                        cx.spawn(async move |cx| {
                            cx.background_executor().timer(std::time::Duration::from_millis(1500)).await;
                            let _ = d.update(cx, |v, cx| { *v = false; cx.notify(); });
                        }).detach();
                    })
                    .child(icon(if copied { IconName::Check } else { IconName::Copy }, 13., if copied { ok } else { fg })),
            )
    })
}
