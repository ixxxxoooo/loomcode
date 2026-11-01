//! 右侧「子 Agent」标签:展示某个子 Agent 的完整对话(对标 OhMyAgent 在标签页里打开子代理)。
//!
//! @author ixxxxoooo

use crate::right_panel::{RightPanel, walk_items};
use crate::timeline;
use crate::ui::{empty_state, icon, pill};
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::*;

impl RightPanel {
    /// 打开(或聚焦)某个子 Agent 的对话标签。
    ///
    /// 参数:`id` 子 Agent 条目 id。
    pub fn open_subagent(&mut self, id: String, cx: &mut Context<Self>) {
        self.subagent = Some(id);
        self.open_tab(crate::workbench::RightTab::Subagent, cx);
    }

    /// 子 Agent 标签内容:顶部是类型 / 标题 / 状态,下面是它自己的时间线。
    ///
    /// 参数:`items` 当前会话时间线。返回:标签内容元素。
    pub(crate) fn subagent_view(&self, items: &[TimelineItem], window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let mut found: Option<&SubagentCard> = None;
        walk_items(items, &mut |i| {
            if let TimelineItem::Subagent(s) = i && Some(&s.id) == self.subagent.as_ref() {
                found = Some(s);
            }
        });
        let (Some(card), Some(model)) = (found, self.model.clone()) else {
            return empty_state(IconName::Bot, "没有可显示的子 Agent", "在对话里点击子 Agent 卡片,完整对话会显示在这里", cx).into_any_element();
        };
        let (text, color) = crate::task_ui::subagent_style(card.status, cx);
        let now = now_ms();
        let meta = crate::task_ui::meta_text(card.elapsed_ms(now), card.tools(), card.tokens);
        let stop = card.task_id.clone().filter(|_| card.is_active());
        let model_for_stop = self.model.clone();
        let body: Vec<AnyElement> = card.children.iter().map(|c| timeline::render_item(&model, c, 1, window, cx)).collect();
        let head = h_flex().w_full().px_3().py_2().gap_2().items_center().border_b_1().border_color(t.border)
            .child(icon(IconName::Bot, 14., t.primary))
            .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(card.description.clone()))
            .child(div().text_size(px(12.)).text_color(t.muted_foreground).child(card.agent_type.clone()))
            .child(pill(text, color, t.muted))
            .when_some(stop.zip(model_for_stop), |d, (task, m)| d.child(
                gpui_kit::component::button::Button::new("sub-tab-stop").xsmall().ghost().icon(IconName::Square).label("停止")
                    .on_click(move |_, _, cx| { m.update(cx, |m, cx| m.stop_task(&task, cx)); }),
            ));
        let stats = v_flex().w_full().px_3().py_1().gap_0p5().border_b_1().border_color(t.border).text_size(px(11.)).text_color(t.muted_foreground)
            .when(!meta.is_empty(), |d| d.child(meta))
            .when(!card.summary.is_empty(), |d| d.child(div().text_size(px(12.)).text_color(t.foreground).child(card.summary.clone())));
        v_flex().size_full()
            .child(head)
            .child(stats)
            .child(
                v_flex().id("subagent-scroll").flex_1().min_h_0().overflow_y_scroll().p_3().gap_2()
                    .children(body)
                    .when(card.children.is_empty(), |d| d.child(div().text_size(px(12.)).text_color(t.muted_foreground).child("暂无活动"))),
            )
            .into_any_element()
    }
}
