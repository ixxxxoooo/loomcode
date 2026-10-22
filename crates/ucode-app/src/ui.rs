//! 公共小组件与样式常量。

use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, Icon, Sizable, h_flex, v_flex};
use gpui_kit::*;
use ucode_core::SessionBadge;

/// 等宽字体族。
pub const MONO: &str = "Menlo";

/// 小图标(12 / 14 / 16 三档)。
pub fn icon(name: IconName, size: f32, color: Hsla) -> Icon {
    Icon::new(name).size(px(size)).text_color(color)
}

/// 会话状态点:运行实心、等待空心环、未读小点、空闲不显示。
pub fn status_dot(badge: SessionBadge, cx: &App) -> Option<Div> {
    let t = cx.theme();
    let base = div().size(px(8.)).rounded_full().flex_none();
    match badge {
        SessionBadge::Idle => None,
        SessionBadge::Running => Some(base.bg(t.primary)),
        SessionBadge::Waiting => Some(base.border_2().border_color(t.warning)),
        SessionBadge::Unread => Some(base.bg(t.info)),
    }
}

/// 圆角小标签(胶囊)。
pub fn pill(text: impl Into<SharedString>, fg: Hsla, bg: Hsla) -> Div {
    div()
        .px_1p5()
        .h(px(18.))
        .flex()
        .items_center()
        .rounded(px(9.))
        .bg(bg)
        .text_color(fg)
        .text_size(px(11.))
        .child(text.into())
}

/// 带发丝边框的卡片容器。
pub fn card(cx: &App) -> Div {
    let t = cx.theme();
    v_flex().w_full().rounded(px(10.)).border_1().border_color(t.border).bg(t.background)
}

/// 区块标题行(小号、次要色)。
pub fn caption(text: impl Into<SharedString>, cx: &App) -> Div {
    div().text_size(px(11.)).text_color(cx.theme().muted_foreground).child(text.into())
}

/// 等宽文本容器。
pub fn mono_box(cx: &App) -> Div {
    let t = cx.theme();
    div()
        .w_full()
        .p_2()
        .rounded(px(8.))
        .bg(t.muted)
        .font_family(MONO)
        .text_size(px(12.))
        .text_color(t.foreground)
}

/// 右侧面板与设置里的分段切换按钮行高。
pub fn row_h() -> Pixels {
    px(28.)
}

/// 纵向居中的空状态。
pub fn empty_state(icon_name: IconName, title: &str, hint: &str, cx: &App) -> Div {
    let t = cx.theme();
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_2()
        .text_color(t.muted_foreground)
        .child(icon(icon_name, 28., t.muted_foreground))
        .child(div().text_size(px(13.)).text_color(t.foreground).child(title.to_string()))
        .child(div().text_size(px(12.)).child(hint.to_string()))
}

#[allow(dead_code)]
pub fn flex_row() -> Div {
    h_flex()
}
