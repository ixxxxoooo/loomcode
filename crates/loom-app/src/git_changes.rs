//! 「改动」分段:顶部在「Claude 改动 / Git 更改」之间切换。
//! 「Git 更改」就是全工作台共用的源代码管理视图(`scm_view.rs`,对标 VS Code),
//! IDE 窗口的活动栏「源代码管理」直接显示同一个视图(不带 Claude 改动切换)。
//!
//! @author ixxxxoooo

use crate::right_panel::RightPanel;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::TimelineItem;

/// 「改动」分段的两种内容。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChangesMode {
    /// Claude 在本会话里改过的文件
    #[default]
    Claude,
    /// 工作区的 Git 更改(源代码管理视图)
    Git,
}

impl ChangesMode {
    /// 初始模式。调试变量 `LOOM_CHANGES=git` 直接进入 Git 模式。
    pub fn initial() -> Self {
        if std::env::var("LOOM_CHANGES").is_ok_and(|v| v.starts_with("git")) { Self::Git } else { Self::Claude }
    }
}

impl RightPanel {
    /// 「改动」分段内容:IDE 窗口(没有分段栏)直接是源代码管理;Agent 窗口是切换条 + 对应内容。
    pub(crate) fn changes_tab(&self, items: &[TimelineItem], cx: &mut Context<Self>) -> AnyElement {
        // 宿主决定点文件行的行为:IDE(没有分段栏,旁边就是编辑器)打开文件,Agent 右栏就地展开 diff
        let open_on_click = !self.show_tabs;
        self.scm.update(cx, |s, _| s.open_on_click = open_on_click);
        if !self.show_tabs {
            return self.scm.clone().into_any_element();
        }
        let body = match self.changes_mode {
            ChangesMode::Claude => self.changes_view(items, cx),
            ChangesMode::Git => self.scm.clone().into_any_element(),
        };
        v_flex().size_full().child(self.changes_switch(cx)).child(div().flex_1().min_h_0().child(body)).into_any_element()
    }

    fn changes_switch(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let count = self.scm.read(cx).count();
        let btn = |id: &'static str, label: &'static str, mode: ChangesMode, badge: usize, cx: &mut Context<Self>| {
            let on = self.changes_mode == mode;
            div().id(id).h(px(24.)).px_2().gap_1().flex().items_center().rounded(px(6.)).cursor_pointer().text_size(px(12.))
                .when(on, |d| d.bg(t.list_active).font_weight(FontWeight::MEDIUM))
                .when(!on, |d| d.text_color(t.muted_foreground).hover(|d| d.bg(t.list_hover)))
                .on_click(cx.listener(move |this, _, _, cx| this.set_changes_mode(mode, cx)))
                .child(label)
                .when(badge > 0, |d| d.child(crate::ui::pill(badge.to_string(), t.primary_foreground, t.primary)))
        };
        h_flex().h(px(34.)).px_2().gap_1().items_center().border_b_1().border_color(t.border)
            .child(btn("chg-mode-claude", "Claude 改动", ChangesMode::Claude, 0, cx))
            .child(btn("chg-mode-git", "Git 更改", ChangesMode::Git, count, cx))
    }

    /// 切换「Claude 改动 / Git 更改」;切到 Git 时立刻刷新一次。
    pub(crate) fn set_changes_mode(&mut self, mode: ChangesMode, cx: &mut Context<Self>) {
        self.changes_mode = mode;
        if mode == ChangesMode::Git {
            self.scm.update(cx, |s, cx| s.refresh(cx));
        }
        cx.notify();
    }
}
