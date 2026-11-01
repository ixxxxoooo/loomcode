//! 表格查找(⌘F):在当前可见行里查找包含关键词的单元格,回车 / ⌘G / ⇧⌘G 在命中之间跳转。

use crate::sheet_view::SheetView;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex};
use gpui_kit::*;
use std::collections::HashSet;
use std::sync::Arc;

impl SheetView {
    /// 打开 / 关闭查找条(打开时聚焦输入框)。
    pub(crate) fn toggle_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find_open {
            self.close_find(window, cx);
        } else {
            self.find_open = true;
            self.find_input.update(cx, |i, cx| i.focus(window, cx));
            self.refresh_matches(cx);
            cx.notify();
        }
    }

    /// 关闭查找条并清掉高亮。
    pub(crate) fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find_open = false;
        self.matches = Arc::default();
        self.match_set = Arc::default();
        self.focus.focus(window, cx);
        cx.notify();
    }

    /// 按输入框里的关键词重新匹配(在当前可见行里),保持当前序号不越界。
    pub(crate) fn refresh_matches(&mut self, cx: &mut Context<Self>) {
        if !self.find_open {
            return;
        }
        let q = self.find_input.read(cx).value().to_string();
        let found = loom_sheet::find(&self.sheet, &self.visible, &q);
        self.match_set = Arc::new(found.iter().map(|m| (m.row, m.col)).collect::<HashSet<_>>());
        self.matches = Arc::new(found);
        self.current_match = self.current_match.min(self.matches.len().saturating_sub(1));
        cx.notify();
    }

    /// 跳到下一处(`step` 为 1)/ 上一处(-1)命中:选中该单元格并滚动到可视区。
    pub(crate) fn step_match(&mut self, step: isize, cx: &mut Context<Self>) {
        let n = self.matches.len();
        if n == 0 {
            return;
        }
        self.current_match = (self.current_match as isize + step).rem_euclid(n as isize) as usize;
        let m = self.matches[self.current_match];
        self.selected = Some((m.row, m.col));
        if let Some(ix) = self.visible.iter().position(|r| *r == m.row) {
            self.scroll.scroll_to_item(ix, ScrollStrategy::Center);
        }
        cx.notify();
    }

    /// 查找条:输入框 + 命中计数 + 上一处 / 下一处 + 关闭。
    pub(crate) fn find_bar(&self, cx: &mut Context<Self>) -> Div {
        let t = cx.theme().clone();
        let count = if self.matches.is_empty() { "无结果".to_string() } else { format!("{} / {}", self.current_match + 1, self.matches.len()) };
        h_flex().h(px(34.)).w_full().flex_none().px_2().gap_2().items_center().border_b_1().border_color(t.border).bg(t.sidebar)
            .child(div().w(px(260.)).child(Input::new(&self.find_input)))
            .child(div().text_size(px(11.5)).text_color(t.muted_foreground).child(count))
            .child(Button::new("sheet-find-prev").xsmall().ghost().icon(IconName::ChevronUp).tooltip("上一处(⇧⌘G)").on_click(cx.listener(|this, _, _, cx| this.step_match(-1, cx))))
            .child(Button::new("sheet-find-next").xsmall().ghost().icon(IconName::ChevronDown).tooltip("下一处(⌘G)").on_click(cx.listener(|this, _, _, cx| this.step_match(1, cx))))
            .child(div().flex_1())
            .child(Button::new("sheet-find-close").xsmall().ghost().icon(IconName::X).tooltip("关闭(Esc)").on_click(cx.listener(|this, _, window, cx| this.close_find(window, cx))))
    }
}
