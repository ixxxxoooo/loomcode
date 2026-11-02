//! 搜索面板的渲染(对标 VS Code):标题栏按钮、搜索框与三个开关、替换行、包含 / 排除文件、
//! 汇总行、虚拟滚动的结果列表。
//!
//! @author ixxxxoooo

use crate::ide_search::{ReplaceAllMatches, SearchPanel, ToggleCase, TogglePreserveCase, ToggleRegex, ToggleReplace, ToggleWholeWord};
use crate::ide_search_model::{self as model, Row};
use crate::ide_search_rows::{chip, file_row, match_row};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::component::{ActiveTheme, Disableable, Selectable, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

impl SearchPanel {
    /// 标题栏:「搜索」+ 刷新 / 清除结果 / 全部折叠。
    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let none = self.files.is_empty();
        let all_collapsed = !none && self.files.iter().all(|f| self.collapsed.contains(&f.path));
        h_flex().h(px(crate::tab_style::TAB_H)).px_3().gap_0p5().items_center().flex_none()
            .child(div().flex_1().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(t.muted_foreground).child("搜索"))
            .child(Button::new("sr-refresh").xsmall().ghost().icon(IconName::RefreshCw).tooltip("刷新").on_click(cx.listener(|this, _, _, cx| this.schedule(0, cx))))
            .child(Button::new("sr-clear").xsmall().ghost().icon(IconName::ListX).tooltip("清除搜索结果").on_click(cx.listener(|this, _, window, cx| this.clear(window, cx))))
            .child(Button::new("sr-collapse").xsmall().ghost().icon(if all_collapsed { IconName::ChevronsUpDown } else { IconName::ChevronsDownUp }).disabled(none)
                .tooltip(if all_collapsed { "全部展开" } else { "全部折叠" }).on_click(cx.listener(|this, _, _, cx| this.toggle_collapse_all(cx))))
    }

    /// 搜索框(带 Aa / ab / .* 三个开关)、替换行与「包含 / 排除文件」。
    fn inputs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let (case, word, regex) = (self.case, self.word, self.regex);
        let toggles = h_flex().gap_0p5()
            .child(chip("sr-case", "Aa", "区分大小写(⌥⌘C)", case, false, cx, cx.listener(|this, _, _, cx| this.toggle_case(cx))))
            .child(chip("sr-word", "ab", "全字匹配(⌥⌘W)", word, true, cx, cx.listener(|this, _, _, cx| this.toggle_word(cx))))
            .child(chip("sr-regex", ".*", "使用正则表达式(⌥⌘R)", regex, false, cx, cx.listener(|this, _, _, cx| this.toggle_regex(cx))));
        let search = Input::new(&self.input).small().suffix(toggles);
        let replace = self.replace_open.then(|| {
            let preserve = chip("sr-preserve", "AB", "保留大小写(⌥⌘P)", self.preserve_case, false, cx, cx.listener(|this, _, _, cx| this.toggle_preserve_case(cx)));
            h_flex().gap_1().items_center()
                .child(div().flex_1().min_w_0().child(Input::new(&self.replace_input).small().suffix(preserve)))
                .child(Button::new("sr-replace-all").xsmall().ghost().icon(IconName::Replace).disabled(self.files.is_empty()).tooltip("全部替换(⌘⌥↩)")
                    .on_click(cx.listener(|this, _, window, cx| this.confirm_replace_all(window, cx))))
        });
        let details = self.details_open.then(|| {
            let label = |s: &'static str| div().text_size(px(11.)).text_color(t.muted_foreground).child(s);
            let ignore_on = self.use_ignore;
            v_flex().gap_1()
                .child(label("要包含的文件"))
                .child(Input::new(&self.include_input).small())
                .child(label("要排除的文件"))
                .child(Input::new(&self.exclude_input).small())
                .child(h_flex().id("sr-use-ignore").gap_1p5().items_center().cursor_pointer().py_0p5().text_size(px(11.5)).text_color(t.muted_foreground)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_use_ignore(cx)))
                    .child(div().size(px(14.)).rounded(px(3.)).border_1().border_color(if ignore_on { t.primary } else { t.border }).when(ignore_on, |d| d.bg(t.primary)).flex().items_center().justify_center()
                        .when(ignore_on, |d| d.child(crate::ui::icon(IconName::Check, 10., t.primary_foreground))))
                    .child("使用排除设置与忽略文件(.gitignore)"))
        });
        h_flex().px_2().gap_1().items_start().flex_none()
            .child(
                div().id("sr-toggle-replace").flex_none().mt(px(5.)).size(px(18.)).flex().items_center().justify_center().rounded(px(4.)).cursor_pointer().hover(|d| d.bg(t.list_hover))
                    .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("切换替换(⌥⌘F)").build(window, cx))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_replace(cx)))
                    .child(crate::ui::icon(if self.replace_open { IconName::ChevronDown } else { IconName::ChevronRight }, 14., t.muted_foreground)),
            )
            .child(
                v_flex().flex_1().min_w_0().gap_1()
                    .child(search)
                    .children(replace)
                    .child(h_flex().justify_end().child(Button::new("sr-details").xsmall().ghost().icon(IconName::Ellipsis).selected(self.details_open).tooltip("切换搜索详细信息").on_click(cx.listener(|this, _, _, cx| this.toggle_details(cx)))))
                    .children(details),
            )
    }

    /// 汇总行 / 错误 / 空结果提示。
    fn status_line(&self, cx: &App) -> Option<AnyElement> {
        let t = cx.theme();
        let base = || div().px_3().py_1().flex_none().text_size(px(12.));
        if let Some(e) = &self.error {
            return Some(base().text_color(t.danger).child(e.clone()).into_any_element());
        }
        if self.running && self.files.is_empty() {
            return Some(base().text_color(t.muted_foreground).child("搜索中…").into_any_element());
        }
        if !self.searched {
            return None;
        }
        let (nf, nm) = model::totals(&self.files);
        let text = if nm == 0 { "未找到结果。可检查「包含 / 排除文件」与忽略文件设置。".to_string() } else { format!("{nf} 个文件中有 {nm} 个结果{}", if self.truncated { "(仅显示前 20000 处)" } else { "" }) };
        Some(base().text_color(t.muted_foreground).child(text).into_any_element())
    }
}

impl Render for SearchPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let list = uniform_list("search-rows", self.rows.len(), cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
            range.filter_map(|i| this.rows.get(i).copied()).map(|row| match row {
                Row::File(fi) => file_row(this, fi, cx),
                Row::Match(fi, mi) => match_row(this, fi, mi, cx),
            }).collect::<Vec<_>>()
        })).track_scroll(&self.scroll).flex_1().min_h_0().w_full();
        v_flex().id("ide-search").key_context("IdeSearch").size_full().bg(t.sidebar)
            .on_action(cx.listener(|this, _: &ToggleCase, _, cx| this.toggle_case(cx)))
            .on_action(cx.listener(|this, _: &ToggleWholeWord, _, cx| this.toggle_word(cx)))
            .on_action(cx.listener(|this, _: &ToggleRegex, _, cx| this.toggle_regex(cx)))
            .on_action(cx.listener(|this, _: &TogglePreserveCase, _, cx| this.toggle_preserve_case(cx)))
            .on_action(cx.listener(|this, _: &ToggleReplace, _, cx| this.toggle_replace(cx)))
            .on_action(cx.listener(|this, _: &ReplaceAllMatches, window, cx| if this.replace_open { this.confirm_replace_all(window, cx) }))
            .child(self.header(cx))
            .child(self.inputs(cx))
            .children(self.status_line(cx))
            .child(div().h(px(4.)).flex_none())
            .child(list)
    }
}
