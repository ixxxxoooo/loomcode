//! 用户消息编辑卡里的模型选择(对标 OhMyAgent 的编辑卡:底部左侧模型按钮,可换模型后重新发送)。
//!
//! @author ixxxxoooo

use crate::model::SessionModel;
use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::*;

/// 当前模型的显示名与展开状态。返回:`(名称, 是否展开)`。
fn current(session: &Entity<SessionModel>, id: &str, cx: &App) -> (String, bool) {
    let m = session.read(cx);
    let st = &m.state;
    let name = st.models.iter().find(|x| x.id == st.model).map(|x| x.name.clone()).unwrap_or_else(|| st.model.clone());
    (name, m.editing.iter().any(|e| e.id == id && e.model_open))
}

/// 底部左侧的模型按钮(品牌图标 + 名称,宽度自适应)和向下弹出的模型列表,放进同一个相对定位容器。
/// 没有可选模型(演示 / 未握手)时返回 None。
///
/// 参数:`session` 会话;`id` 正在编辑的消息 id。
pub fn picker(session: &Entity<SessionModel>, edit_id: &str, cx: &App) -> Option<AnyElement> {
    if session.read(cx).state.models.is_empty() {
        return None;
    }
    let t = cx.theme();
    let (name, open) = current(session, edit_id, cx);
    let (models, cur) = {
        let st = &session.read(cx).state;
        (st.models.clone(), st.model.clone())
    };
    let s = session.clone();
    let eid = edit_id.to_string();
    let chip = h_flex().id(SharedString::from(format!("umsg-model-{edit_id}"))).h(px(28.)).px_2().gap_1p5().items_center().rounded(px(6.)).cursor_pointer()
        .text_size(px(12.)).text_color(t.muted_foreground)
        .hover(|d| d.bg(t.foreground.opacity(0.08))).active(|d| d.bg(t.foreground.opacity(0.16)))
        .on_click(move |_, _, cx| { s.update(cx, |m, cx| m.toggle_edit_model(&eid, cx)); })
        .child(crate::brand_icons::brand_icon(&cur, &name, 13., t.muted_foreground))
        .child(name)
        .child(icon(if open { IconName::ChevronUp } else { IconName::ChevronDown }, 12., t.muted_foreground));
    let list = open.then(|| {
        v_flex().absolute().top(px(32.)).left_0().min_w(px(150.)).p_1().gap_0p5().rounded(px(10.)).bg(t.popover).border_1().border_color(t.border).shadow_md().occlude()
            .children(models.into_iter().map(|mi| {
                let s = session.clone();
                let eid = edit_id.to_string();
                let id = mi.id.clone();
                let on = mi.id == cur;
                h_flex().id(SharedString::from(format!("umsg-model-{edit_id}-{}", mi.id))).px_2().py(px(5.)).gap_2().items_center().rounded(px(6.)).cursor_pointer()
                    .hover(|d| d.bg(t.foreground.opacity(0.08)))
                    .on_click(move |_, _, cx| { s.update(cx, |m, cx| { m.set_model(id.clone(), cx); m.toggle_edit_model(&eid, cx); }); })
                    .child(crate::brand_icons::brand_icon(&mi.id, &mi.name, 13., t.foreground))
                    .child(div().flex_1().whitespace_nowrap().text_size(px(13.)).child(mi.name))
                    .child(div().w(px(14.)).child(if on { icon(IconName::Check, 13., t.primary).into_any_element() } else { div().into_any_element() }))
            }))
    });
    Some(div().relative().child(chip).children(list).into_any_element())
}
