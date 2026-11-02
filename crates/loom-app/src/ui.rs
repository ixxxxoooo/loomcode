//! 公共小组件与样式常量。

use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, Icon, h_flex, v_flex};
use gpui_kit::*;
use loom_core::SessionBadge;

/// 等宽字体族。
pub const MONO: &str = "Menlo";

/// 小图标(12 / 14 / 16 三档)。
pub fn icon(name: IconName, size: f32, color: Hsla) -> Icon {
    Icon::new(name).size(px(size)).text_color(color)
}

/// 呼吸动画:一个来回 1.6 秒,透明度在 0.35–1.0 之间平滑起伏。
///
/// 参数:`el` 带 id 的元素;`id` 动画标识。返回:带呼吸动画的元素。
pub fn breathe<E: IntoElement + StatefulInteractiveElement + Styled + 'static>(el: E, id: &'static str) -> AnyElement {
    el.with_animation(id, Animation::new(std::time::Duration::from_millis(1600)).repeat(),
        |d, delta| d.opacity(0.675 + 0.325 * (delta * std::f32::consts::TAU).cos())).into_any_element()
}

/// 会话状态点:运行实心(只有运行中才呼吸)、等待空心环(静止)、未读小点(静止)、空闲不显示。
pub fn status_dot(badge: SessionBadge, cx: &App) -> Option<AnyElement> {
    let t = cx.theme();
    let base = |id: &'static str| div().id(id).size(px(8.)).rounded_full().flex_none();
    match badge {
        SessionBadge::Idle => None,
        SessionBadge::Running => Some(breathe(base("dot-run").bg(t.primary), "dot-run-breath")),
        SessionBadge::Waiting => Some(base("dot-wait").border_2().border_color(t.warning).into_any_element()),
        SessionBadge::Unread => Some(base("dot-unread").bg(t.info).into_any_element()),
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

/// 开发版标识:由 `scripts/dev.sh` 以 `LOOM_DEV=1` 启动时显示,正式版返回 `None`。
pub fn dev_badge(cx: &App) -> Option<Div> {
    if !std::env::var("LOOM_DEV").is_ok_and(|v| v == "1") {
        return None;
    }
    let t = cx.theme();
    Some(pill("开发版", t.primary_foreground, t.primary))
}

/// 当前运行环境的标识,一次只显示一个,都是主题色徽章。
///
/// | 后端 | 构建 | 显示 |
/// | --- | --- | --- |
/// | 演示(`--demo` / `LOOM_DEMO=1`) | 开发版或正式版 | 演示模式(演示会换掉后端和数据,优先提示) |
/// | 真实(默认) | 开发版(`scripts/dev.sh`,`LOOM_DEV=1`) | 开发版 |
/// | 真实 | 正式版(打包的 LoomCode.app) | 无,不占位置 |
///
/// 放在左下角设置按钮的右侧。
///
/// 参数:`demo` 是否演示模式。
pub fn env_badges(demo: bool, cx: &App) -> Option<Stateful<Div>> {
    let t = cx.theme();
    let badge = if demo { Some(pill("演示模式", t.primary_foreground, t.primary)) } else { dev_badge(cx) }?;
    // 点击徽章 = 一键切换演示 / 真实(确认后重启)
    Some(
        div().id("env-badge").cursor_pointer().child(badge)
            .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(if demo { "点击切换到真实模式" } else { "点击切换到演示模式" }).build(window, cx))
            .on_click(|_, window, cx| window.dispatch_action(Box::new(crate::workbench::SwitchMode), cx)),
    )
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

/// 标题栏右侧的「窗口切换」按钮(Agent 窗口的「IDE」与 IDE 窗口的「Agent」共用),
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
pub const MODE_SWITCH_W: f32 = 92.;
