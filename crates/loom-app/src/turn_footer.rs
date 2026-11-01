//! 助手回复下面的操作条(悬停显示):复制。
//!
//! @author ixxxxoooo

use crate::model::SessionModel;
use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// 操作条高度(固定占位,悬停才可见,避免悬停时行高跳动)。
pub const FOOTER_H: f32 = 26.;

/// 渲染操作条。
///
/// 参数:`session` 会话;`id` 助手条目 id;`text` 要复制的原文;`group` 外层悬停分组名。
pub fn render(session: &Entity<SessionModel>, id: &str, text: &str, group: SharedString, cx: &App) -> AnyElement {
    let t = cx.theme();
    let copied = session.read(cx).copied.as_deref() == Some(id);
    let s = session.clone();
    let (cid, ctext) = (id.to_string(), text.to_string());
    h_flex().h(px(FOOTER_H)).items_center().gap_1().mt_0p5()
        .when(!copied, |d| d.invisible().group_hover(group, |d| d.visible()))
        .child(
            h_flex().id(SharedString::from(format!("copy-{id}"))).h(px(22.)).px_1p5().gap_1().items_center().rounded(px(5.)).cursor_pointer()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .text_size(px(12.)).text_color(t.muted_foreground)
                .hover(|d| d.bg(t.list_hover).text_color(t.foreground))
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(ctext.clone()));
                    s.update(cx, |m, cx| m.mark_copied(&cid, cx));
                })
                .child(icon(if copied { IconName::Check } else { IconName::Copy }, 12., if copied { t.success } else { t.muted_foreground }))
                .child(if copied { "已复制" } else { "复制" }),
        )
        .into_any_element()
}
