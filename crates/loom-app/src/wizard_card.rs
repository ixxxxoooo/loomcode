//! 提问向导卡(输入框上方,对标 OhMyAgent):可折叠标题 + 逐题步骤 + 字母徽标选项 + 页码与跳过 / 下一题 / 提交。
//! 单选点选后自动进入下一题;选项多于 6 个时分两列。
//!
//! @author ixxxxoooo

use crate::dock_cards::letter_badge;
use crate::model::SessionModel;
use crate::ui::{caption, icon, pill};
use crate::wizard_input as wi;
use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::*;

/// 渲染向导卡。
///
/// 参数:`req` 提问;`queued` 排在后面的提问数。返回:卡片元素。
pub fn render(session: &Entity<SessionModel>, req: &QuestionRequest, queued: usize, cx: &App) -> AnyElement {
    let t = cx.theme();
    let n = req.questions.len();
    if n == 0 {
        return div().into_any_element();
    }
    let collapse_key = format!("wz-{}", req.id);
    let collapsed = session.read(cx).is_expanded(&collapse_key);
    let step = session.read(cx).wizard_step.get(&req.id).copied().unwrap_or(0).min(n - 1);
    let draft = session.read(cx).wizard_draft.get(&req.id).cloned().unwrap_or_default();
    let answered = |i: usize| wi::answered(session, &req.id, i, cx);
    let unanswered = (0..n).filter(|i| !answered(*i)).count();
    let last = step + 1 == n;
    let can_advance = if last { unanswered == 0 } else { answered(step) };
    let q = &req.questions[step];
    let picked = draft.get(step).cloned().unwrap_or_default();

    let toggle = {
        let (s, k) = (session.clone(), collapse_key.clone());
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| { s.update(cx, |m, cx| m.toggle(&k, cx)); }
    };
    let header = h_flex().id(SharedString::from(format!("wz-h-{}", req.id))).h(px(32.)).px_3().gap_1p5().items_center().cursor_pointer()
        .text_color(t.muted_foreground).on_click(toggle)
        .child(icon(if collapsed { IconName::ChevronRight } else { IconName::ChevronDown }, 11., t.muted_foreground))
        .child(icon(IconName::CircleQuestionMark, 12., t.muted_foreground))
        .child(div().text_size(px(12.)).font_weight(FontWeight::MEDIUM).child("提问"))
        .when(n > 1, |d| d.child(caption(format!("{n} 题"), cx)))
        .child(div().flex_1())
        .when(queued > 0, |d| d.child(pill(format!("还有 {queued} 个待回答"), t.primary, t.muted)));

    let two_cols = q.options.len() > 6;
    let rows: Vec<AnyElement> = q.options.iter().enumerate().map(|(oi, opt)| {
        let on = picked.contains(&opt.label);
        let (s, rid, label) = (session.clone(), req.id.clone(), opt.label.clone());
        h_flex().id(SharedString::from(format!("{}-q{step}-o{oi}", req.id)))
            .when(two_cols, |d| d.w(relative(0.49))).when(!two_cols, |d| d.w_full())
            .gap_2().px_2().py(px(7.)).items_start().rounded(px(8.)).border_1().cursor_pointer()
            .border_color(if on { t.primary.opacity(0.5) } else { t.border }).bg(if on { t.accent } else { t.muted.opacity(0.5) })
            .hover(|d| d.bg(t.list_hover))
            // 单选点选后自动进入下一题(最后一题不自动提交)
            .on_click(move |_, window, cx| wi::pick(&s, &rid, step, &label, window, cx))
            .child(letter_badge(oi, on, cx))
            .child(v_flex().flex_1().min_w_0()
                .child(div().text_size(px(13.)).child(opt.label.clone()))
                .when(!opt.description.is_empty(), |d| d.child(caption(opt.description.clone(), cx))))
            .when(on, |d| d.child(icon(IconName::Check, 12., t.primary)))
            .into_any_element()
    }).collect();

    let other = session.read(cx).wizard_inputs.get(&req.id).and_then(|v| v.get(step)).cloned();
    let typed = !wi::custom(session, &req.id, step, cx).is_empty();
    let mut rows = rows;
    if let Some(input) = other {
        rows.push(
            h_flex().when(two_cols, |d| d.w(relative(0.49))).when(!two_cols, |d| d.w_full())
                .gap_2().px_2().py(px(4.)).items_center().rounded(px(8.)).border_1()
                .border_color(if typed { t.primary.opacity(0.5) } else { t.border }).bg(if typed { t.accent } else { t.muted.opacity(0.5) })
                .child(letter_badge(q.options.len(), typed, cx))
                .child(div().flex_1().min_w_0().child(Input::new(&input).appearance(false).bordered(false)))
                .into_any_element(),
        );
    }
    let body = v_flex().w_full().p_3().gap_2()
        .child(h_flex().gap_2().items_start()
            .when(!q.header.is_empty(), |d| d.child(pill(q.header.clone(), t.primary, t.muted)))
            .child(div().flex_1().min_w_0().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(q.question.clone()))
            .when(q.multi_select, |d| d.child(pill("可多选", t.muted_foreground, t.muted))))
        .child(h_flex().id(SharedString::from(format!("wz-o-{}", req.id))).flex_wrap().gap(px(6.)).max_h(px(260.)).overflow_y_scroll().children(rows));

    let pager = h_flex().gap_1().items_center().text_size(px(12.)).text_color(t.muted_foreground)
        .child({
            let (s, id) = (session.clone(), req.id.clone());
            Button::new(SharedString::from(format!("{}-prev", req.id))).xsmall().ghost().icon(IconName::ChevronLeft).disabled(step == 0)
                .on_click(move |_, _, cx| wi::go(&s, &id, step as isize - 1, cx))
        })
        .child(format!("{}/{n}", step + 1))
        .child({
            let (s, id) = (session.clone(), req.id.clone());
            Button::new(SharedString::from(format!("{}-fwd", req.id))).xsmall().ghost().icon(IconName::ChevronRight).disabled(last)
                .on_click(move |_, _, cx| wi::go(&s, &id, step as isize + 1, cx))
        });
    let footer = h_flex().w_full().px_3().py_2().gap_2().items_center()
        .when(n > 1, |d| d.child(pager))
        .child(div().flex_1())
        .when(last && unanswered > 0, |d| d.child(caption(format!("还有 {unanswered} 题未回答"), cx)))
        .child({
            let (s, id) = (session.clone(), req.id.clone());
            Button::new(SharedString::from(format!("{}-skip", req.id))).small().label("跳过 ⎋")
                .on_click(move |_, _, cx| wi::skip(&s, &id, cx))
        })
        .child({
            let (s, id) = (session.clone(), req.id.clone());
            Button::new(SharedString::from(format!("{}-next", req.id))).small().primary().label(if last { "提交 ⏎" } else { "下一题 ⏎" }).disabled(!can_advance)
                .on_click(move |_, _, cx| wi::advance(&s, &id, cx))
        });

    let focus = session.read(cx).wizard_focus.clone();
    let key_session = session.clone();
    v_flex().w_full().rounded(px(12.)).border_1().border_color(t.border).bg(t.background).shadow_sm()
        .key_context("QuestionWizard")
        .when_some(focus, |d, f| d.track_focus(&f))
        // 键盘:A–Z 选择、⏎ 下一题 / 提交、Esc 跳过、← → 翻页
        .on_key_down(move |ev, window, cx| {
            let m = ev.keystroke.modifiers;
            if wi::on_key(&key_session, &ev.keystroke.key, !m.control && !m.platform && !m.alt, window, cx) {
                cx.stop_propagation();
            }
        })
        .child(header)
        .when(!collapsed, |d| {
            d.child(div().h(px(1.)).bg(t.border.opacity(0.5))).child(body).child(div().h(px(1.)).bg(t.border.opacity(0.5))).child(footer)
        })
        .into_any_element()
}
