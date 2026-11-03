//! 右侧「计划」标签:计划全文 + 执行进度(待办),待批准时头部固定 Build 按钮(对标 Cursor 的计划页)。
//!
//! @author ixxxxoooo

use crate::plan_card;
use crate::right_panel::{RightPanel, walk_items};
use crate::ui::{caption, empty_state, icon, pill};
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::*;

impl RightPanel {
    /// 打开(或聚焦)「计划」标签并选中某个计划。
    ///
    /// 参数:`id` 计划条目 id;`None` 只打开标签(显示最新的计划)。
    pub fn open_plan(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        if id.is_some() {
            self.plan = id;
        }
        self.open_tab(crate::workbench::RightTab::Plan, cx);
    }

    /// 「计划」标签内容。
    pub(crate) fn plan_view(&self, items: &[TimelineItem], cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let mut todos: Option<&Vec<TodoItem>> = None;
        let mut plan: Option<&PlanApproval> = None;
        walk_items(items, &mut |item| match item {
            TimelineItem::Todos { items, .. } => todos = Some(items),
            TimelineItem::PlanApproval(p) if plan.is_none() || self.plan.as_deref().is_none_or(|sel| sel == p.id) => plan = Some(p),
            _ => {}
        });
        let Some(model) = self.model.clone().filter(|_| todos.is_some() || plan.is_some()) else {
            return empty_state(IconName::ListTodo, "还没有计划", "待办与计划文档会显示在这里", cx).into_any_element();
        };
        // 头部固定:状态与决定按钮(只在待批准时出现按钮)
        let head = plan.map(|p| {
            let (text, fg) = plan_card::status(p, cx);
            v_flex().w_full().px_3().py_2().gap_2().border_b_1().border_color(t.border)
                .child(h_flex().gap_2().items_center()
                    .child(icon(IconName::ListTodo, 14., if p.approved.is_none() { t.warning } else { t.primary }))
                    .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(plan_card::plan_title(&p.markdown)))
                    .child(pill(text, fg, t.muted)))
                .when(p.approved.is_none(), |d| d.child(plan_card::actions(&model, p, "tab")))
        });
        let progress = todos.map(|todos| {
            let done = todos.iter().filter(|i| i.status == TodoStatus::Completed).count();
            v_flex().gap_1p5()
                .child(h_flex().gap_2().items_center()
                    .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child("执行进度"))
                    .child(caption(format!("{done}/{}", todos.len()), cx)))
                .children(todos.iter().map(|todo| {
                    let (ic, color) = match todo.status {
                        TodoStatus::Completed => (IconName::CircleCheck, t.success),
                        TodoStatus::InProgress => (IconName::CircleDot, t.primary),
                        TodoStatus::Pending => (IconName::Circle, t.muted_foreground),
                    };
                    h_flex().gap_2().items_start().text_size(px(13.))
                        .child(div().mt(px(2.)).child(icon(ic, 14., color)))
                        .child(div().flex_1().min_w_0()
                            .text_color(if todo.status == TodoStatus::Completed { t.muted_foreground } else { t.foreground })
                            .child(todo.content.clone()))
                }))
        });
        // 计划全文走与主会话助手正文同一套渲染(Markdown、代码块、图表)
        let doc = plan.map(|p| crate::rich_text::render_assistant(&model, &format!("plan-doc-{}", p.id), &p.markdown, cx));
        v_flex().size_full()
            .children(head)
            .child(
                v_flex().id("plan-scroll").flex_1().min_h_0().overflow_y_scroll().p_3().gap_3()
                    .children(progress)
                    .children(doc),
            )
            .into_any_element()
    }
}
