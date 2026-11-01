//! 时间线里的「处理结果」:权限已处理一行字、提问回答摘要、计划卡(无按钮,决定在输入框上方)。
//!
//! @author ixxxxoooo

use crate::ui::{icon, pill};
use gpui_kit::assets::IconName;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::*;

/// 权限结果:只留一行纯文字「已允许 / 已拒绝 · 工具标题」;待处理的在输入框上方,这里不显示。
pub fn render_permission(req: &PermissionRequest, cx: &App) -> AnyElement {
    let t = cx.theme();
    let Some(chosen) = req.options.iter().find(|o| Some(&o.id) == req.resolved.as_ref()) else { return div().into_any_element() };
    let ok = matches!(chosen.kind, PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways);
    h_flex().w_full().h(px(24.)).px_2().gap_2().items_center().text_size(px(12.)).text_color(t.muted_foreground)
        .child(icon(if ok { IconName::ShieldCheck } else { IconName::ShieldX }, 12., if ok { t.success } else { t.danger }))
        .child(chosen.label.clone())
        .child(div().flex_1().min_w_0().truncate().child(req.tool.title.clone()))
        .into_any_element()
}

/// 提问结果:每题一块——短标签 + 题干,下面是所选答案(带对勾)。未回答的不显示(在输入框上方)。
pub fn render_question(req: &QuestionRequest, cx: &App) -> AnyElement {
    let t = cx.theme();
    let Some(answers) = &req.answers else { return div().into_any_element() };
    let skipped = answers.iter().all(|a| a.is_empty());
    v_flex().w_full().p_3().gap_2().rounded(px(12.)).border_1().border_color(t.border).bg(t.background)
        .child(h_flex().gap_1p5().items_center().text_color(t.muted_foreground)
            .child(icon(if skipped { IconName::CircleX } else { IconName::CircleCheck }, 13., t.muted_foreground))
            .child(div().text_size(px(12.)).font_weight(FontWeight::MEDIUM).child(if skipped { "已跳过提问" } else { "已回答" })))
        .children(req.questions.iter().zip(answers).map(|(q, a)| {
            v_flex().gap_1p5()
                .child(h_flex().gap_2().items_start()
                    .when(!q.header.is_empty(), |d| d.child(pill(q.header.clone(), t.primary, t.muted)))
                    .child(div().flex_1().min_w_0().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(q.question.clone()))
                    .when(q.multi_select, |d| d.child(pill("可多选", t.muted_foreground, t.muted))))
                .children(if a.is_empty() {
                    vec![div().text_size(px(12.)).text_color(t.muted_foreground).child(if skipped { "—" } else { "未回答" }).into_any_element()]
                } else {
                    a.iter().map(|x| {
                        h_flex().gap_2().px_2().py(px(6.)).items_start().rounded(px(8.)).border_1().border_color(t.primary.opacity(0.4)).bg(t.accent)
                            .child(icon(IconName::Check, 12., t.primary))
                            .child(div().flex_1().min_w_0().text_size(px(13.)).child(x.clone()))
                            .into_any_element()
                    }).collect()
                })
        }))
        .into_any_element()
}

/// 计划卡:正文始终在时间线里(默认展开),状态标签标明待批准 / 已批准 / 已拒绝;批准按钮在输入框上方。
pub fn render_plan(plan: &PlanApproval, cx: &App) -> AnyElement {
    let t = cx.theme();
    let (text, fg) = match plan.approved {
        None => ("待批准", t.warning),
        Some(true) => ("已批准", t.success),
        Some(false) => ("已拒绝 · 继续规划", t.danger),
    };
    v_flex().w_full().p_3().gap_2().rounded(px(12.)).border_1().border_color(if plan.approved.is_none() { t.warning.opacity(0.6) } else { t.border }).bg(t.background)
        .child(h_flex().gap_2().items_center()
            .child(icon(IconName::ListTodo, 14., t.muted_foreground))
            .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child("计划"))
            .child(div().flex_1())
            .child(pill(text, fg, t.muted)))
        .child(crate::code_actions::with_copy(TextView::markdown(SharedString::from(format!("plan-{}", plan.id)), plan.markdown.clone()).selectable(true)))
        .into_any_element()
}
