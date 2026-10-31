//! 公共小组件与样式常量。

use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, Icon, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
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

/// 开发版标识:由 `scripts/dev.sh` 以 `UCODE_DEV=1` 启动时显示,正式版返回 `None`。
pub fn dev_badge(cx: &App) -> Option<Div> {
    if !std::env::var("UCODE_DEV").is_ok_and(|v| v == "1") {
        return None;
    }
    let t = cx.theme();
    Some(pill("开发版", t.background, t.warning))
}

/// 环境标识行:开发版、演示模式(都是中文小标签),放在左下角设置按钮的右侧。
/// 两者都不是时返回 `None`,调用方就不占位置。
///
/// 参数:`demo` 是否演示模式。
pub fn env_badges(demo: bool, cx: &App) -> Option<Div> {
    let dev = dev_badge(cx);
    if dev.is_none() && !demo {
        return None;
    }
    let t = cx.theme();
    Some(h_flex().gap_1().items_center().children(dev).when(demo, |d| d.child(pill("演示模式", t.muted_foreground, t.muted))))
}

/// 标题栏里的可点击控件包装:吞掉鼠标按下,避免在按钮上双击触发窗口最大化或拖动窗口。
/// 只有标题栏的空白处才响应双击。
pub fn no_drag(child: impl IntoElement) -> Div {
    div().on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).child(child)
}

/// 对话框底部的「取消 / 确认」按钮。
///
/// 普通 `Dialog` 不会自己渲染按钮,需要显式放进 `footer`;确认按钮触发 `on_ok` 回调,
/// 取消按钮触发 `on_cancel` 并关闭对话框。
///
/// 参数:`ok_label` 确认按钮文字。
pub fn dialog_footer(ok_label: &'static str) -> gpui_kit::component::dialog::DialogFooter {
    use gpui_kit::component::button::{Button, ButtonVariants};
    use gpui_kit::component::dialog::{DialogAction, DialogClose, DialogFooter};
    DialogFooter::new()
        .child(DialogClose::new().trigger(|b| b.label("取消")))
        .child(DialogAction::new().child(Button::new("dialog-ok").primary().label(ok_label)))
}

/// 标题栏右侧的「窗口切换」按钮(Agent 窗口的「IDE」与 IDE 窗口的「Agents Window」共用),
/// 固定同一宽度,这样两个窗口里切换按钮的位置和大小一致。
///
/// 参数:`id` 元素 id;`label` 文案;`icon_name` 图标;`primary` 为真用主题色实心(只有「回到 Agent」这一个),否则描边。
pub fn mode_switch(id: &'static str, label: &'static str, icon_name: IconName, primary: bool) -> gpui_kit::component::button::Button {
    use gpui_kit::component::Sizable;
    use gpui_kit::component::button::{Button, ButtonVariants};
    let b = Button::new(id).small().label(label).icon(icon_name).w(px(MODE_SWITCH_W));
    if primary { b.primary() } else { b.outline() }
}

/// 窗口切换按钮的宽度。
pub const MODE_SWITCH_W: f32 = 150.;
