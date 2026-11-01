//! 会话时间线视图:把时间线从工作台的 render 里拆成独立视图,以 `.cached()` 嵌入对话区。
//!
//! 为什么:gpui 把一个视图标脏时会连同它的所有祖先一起标脏。时间线原本画在 `Workbench` 里,
//! 输入框的转圈、侧栏的呼吸点、托盘等任何动画每一帧都会让整条时间线(Markdown、高亮、工具卡)
//! 重新构建。拆出来并缓存后,工作台重跑时时间线直接复用上一帧,只有它自己被通知才重建。
//!
//! 缓存的视图只在「自己被 notify」时重建,渲染里读到的模型不会自动让它失效,
//! 所以这里显式观察:会话模型、滚动状态、影响排版的全局偏好与主题。
//! 时间线里嵌的输入框、Markdown 视图都是子视图,它们的通知会自动连带标脏本视图。
//!
//! @author ixxxxoooo

use crate::model::SessionModel;
use gpui_kit::*;

/// 一个会话的时间线(列表 + 吸顶的用户消息 + 跳到最新按钮)。
pub struct TimelineView {
    model: WeakEntity<SessionModel>,
    _subs: Vec<Subscription>,
}

impl TimelineView {
    /// 创建时间线视图并订阅会让它失效的来源。
    ///
    /// 参数:`model` 会话模型;`cx` 视图上下文。返回:视图。
    pub fn new(model: &Entity<SessionModel>, cx: &mut Context<Self>) -> Self {
        let scroller = model.read(cx).scroller.clone();
        let subs = vec![
            cx.observe(model, |_, _, cx| cx.notify()),
            cx.observe(&scroller, |_, _, cx| cx.notify()),
            cx.observe_global::<crate::UiPrefs>(|_, cx| cx.notify()),
            cx.observe_global::<crate::Prefs>(|_, cx| cx.notify()),
            cx.observe_global::<gpui_kit::component::Theme>(|_, cx| cx.notify()),
        ];
        Self { model: model.downgrade(), _subs: subs }
    }
}

impl Render for TimelineView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(model) = self.model.upgrade() else { return div().into_any_element() };
        let scroller_state = model.read(cx).scroller.clone();
        let wheel_model = model.clone();
        let sticky = crate::sticky_user::render(&model, &scroller_state, cx);
        div().size_full().min_h_0()
            .on_scroll_wheel(move |ev, _, cx| {
                let dy = f32::from(ev.delta.pixel_delta(px(20.)).y);
                wheel_model.update(cx, |m, cx| m.on_wheel(dy, cx));
            })
            .child(crate::chat_scroll::chat_scroller("timeline", scroller_state, move |ix, window, cx| crate::timeline::render_row(&model, ix, window, cx), sticky, cx))
            .into_any_element()
    }
}
