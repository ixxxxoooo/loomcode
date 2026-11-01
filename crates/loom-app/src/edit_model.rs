//! 用户消息编辑卡里的模型选择(对标 OhMyAgent 的编辑卡:底部左侧模型按钮,可换模型后重新发送)。
//!
//! @author ixxxxoooo

use crate::model::SessionModel;
use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::*;

/// 当前模型的显示名与展开状态。返回:`(名称, 是否展开)`。
fn current(session: &Entity<SessionModel>, cx: &App) -> (String, bool) {
    let m = session.read(cx);
    let st = &m.state;
    let name = st.models.iter().find(|x| x.id == st.model).map(|x| x.name.clone()).unwrap_or_else(|| st.model.clone());
    (name, m.editing.as_ref().is_some_and(|e| e.model_open))
}

/// 底部左侧的模型按钮;没有可选模型(演示 / 未握手)时返回 None。
///
/// 参数:`session` 会话。
pub fn chip(session: &Entity<SessionModel>, cx: &App) -> Option<AnyElement> {
    if session.read(cx).state.models.is_empty() {
        return None;
    }
    let t = cx.theme();
    let (name, open) = current(session, cx);
    let s = session.clone();
    Some(
        h_flex().id("umsg-model").h(px(26.)).px_2().gap_1().items_center().rounded(px(6.)).cursor_pointer()
            .text_size(px(12.)).text_color(t.muted_foreground)
            .hover(|d| d.bg(t.foreground.opacity(0.08))).active(|d| d.bg(t.foreground.opacity(0.16)))
            .on_click(move |_, _, cx| { s.update(cx, |m, cx| m.toggle_edit_model(cx)); })
            .child(name)
            .child(icon(if open { IconName::ChevronUp } else { IconName::ChevronDown }, 12., t.muted_foreground))
            .into_any_element(),
    )
}

/// 展开的模型列表(在编辑卡内、按钮行下方);未展开返回 None。选中后切换会话模型并收起。
pub fn list(session: &Entity<SessionModel>, cx: &App) -> Option<AnyElement> {
    let (_, open) = current(session, cx);
    if !open {
        return None;
    }
    let t = cx.theme();
    let (models, cur) = {
        let st = &session.read(cx).state;
        (st.models.clone(), st.model.clone())
    };
    Some(
        v_flex().w_full().gap_0p5().pt_1().border_t_1().border_color(t.border).children(models.into_iter().map(|mi| {
            let s = session.clone();
            let id = mi.id.clone();
            let on = mi.id == cur;
            h_flex().id(SharedString::from(format!("umsg-model-{}", mi.id))).w_full().px_2().py(px(5.)).gap_2().items_center().rounded(px(6.)).cursor_pointer()
                .hover(|d| d.bg(t.foreground.opacity(0.08)))
                .on_click(move |_, _, cx| { s.update(cx, |m, cx| { m.set_model(id.clone(), cx); m.toggle_edit_model(cx); }); })
                .child(div().w(px(14.)).child(if on { icon(IconName::Check, 13., t.primary).into_any_element() } else { div().into_any_element() }))
                .child(div().text_size(px(13.)).child(mi.name))
                .child(div().flex_1().min_w_0().truncate().text_size(px(11.)).text_color(t.muted_foreground).child(mi.description))
        })).into_any_element(),
    )
}
