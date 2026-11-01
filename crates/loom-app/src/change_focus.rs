//! 「改动」标签的单文件视图:点时间线里的某个改动文件,右侧只展示该文件的完整 Diff(Cursor 风格)。
//!
//! @author ixxxxoooo

use crate::diff::{self, DiffLine};
use crate::right_panel::{FileChange, RightPanel, collect_changes};
use crate::theme::diff_colors;
use crate::ui::{MONO, icon};
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use crate::workbench::RightTab;
use loom_core::TimelineItem;
use similar::ChangeTag;

/// 在改动列表里按路径找文件(时间线卡片里的路径可能是相对或绝对路径,按后缀兜底)。
///
/// 参数:`changes` 改动列表;`path` 要找的路径。返回:命中的下标。
pub fn find_change(changes: &[FileChange], path: &str) -> Option<usize> {
    changes.iter().position(|c| c.path == path).or_else(|| changes.iter().position(|c| c.path.ends_with(path) || path.ends_with(&c.path)))
}

/// 渲染整文件 Diff:整行底色、行号按增删着色、无外框。
///
/// 参数:`lines` Diff 行;返回:Diff 列。
fn render_flat(lines: &[DiffLine], cx: &App) -> Div {
    let t = cx.theme();
    let (add_bg, del_bg) = diff_colors(cx);
    v_flex().w_full().font_family(MONO).text_size(px(12.5)).children(lines.iter().map(|line| {
        let (bg, fg) = match line.tag {
            ChangeTag::Insert => (Some(add_bg), t.success),
            ChangeTag::Delete => (Some(del_bg), t.danger),
            ChangeTag::Equal => (None, t.muted_foreground),
        };
        let no = line.new_no.or(line.old_no).map(|n| n.to_string()).unwrap_or_default();
        h_flex().w_full().min_h(px(22.)).when_some(bg, |d, b| d.bg(b))
            .child(div().w(px(44.)).flex_none().pr_2().text_right().text_color(fg).child(no))
            .child(div().flex_1().min_w_0().pr_3().text_color(t.foreground).child(line.text.clone()))
    }))
}

impl RightPanel {
    /// 请求在「改动」标签里只看某个文件的 Diff。
    ///
    /// 参数:`path` 文件路径。
    pub fn focus_change_file(&mut self, path: String, cx: &mut Context<Self>) {
        self.focus_change = Some(path);
        self.open_tab(RightTab::Changes, cx);
    }

    /// 单文件 Diff 视图;找不到对应改动时返回 `None`(回到列表)。
    pub(crate) fn focus_change_view(&self, items: &[TimelineItem], cx: &mut Context<Self>) -> Option<AnyElement> {
        let path = self.focus_change.clone()?;
        let changes = collect_changes(items);
        let c = changes.get(find_change(&changes, &path)?)?;
        let t = cx.theme().clone();
        let (lines, a, r) = diff::compute(&c.old, &c.new);
        let name = c.path.rsplit('/').next().unwrap_or(&c.path).to_string();
        let is_new = c.old.is_empty();
        Some(v_flex().size_full()
            .child(
                h_flex().id("focus-back").h(px(30.)).px_3().gap_1p5().items_center().cursor_pointer().text_size(px(12.)).text_color(t.muted_foreground)
                    .hover(|d| d.text_color(t.foreground))
                    .on_click(cx.listener(|this, _, _, cx| { this.focus_change = None; cx.notify(); }))
                    .child(icon(IconName::ChevronLeft, 12., t.muted_foreground))
                    .child(format!("全部改动({} 个文件)", changes.len())),
            )
            .child(
                h_flex().h(px(38.)).px_3().gap_2().items_center().border_y_1().border_color(t.border)
                    .child(crate::file_icons::file_icon_img(&name, false, false, 15.))
                    .child(div().min_w_0().truncate().text_size(px(13.)).child(c.path.clone()))
                    .when(a > 0, |d| d.child(div().text_size(px(12.)).font_family(MONO).text_color(t.success).child(format!("+{a}"))))
                    .when(r > 0, |d| d.child(div().text_size(px(12.)).font_family(MONO).text_color(t.danger).child(format!("−{r}"))))
                    .child(div().flex_1())
                    .when(is_new, |d| d.child(div().text_size(px(12.)).text_color(t.success).child("新文件"))),
            )
            .child(div().id("focus-scroll").flex_1().min_h_0().overflow_y_scroll().child(render_flat(&lines, cx).pb_3()))
            .into_any_element())
    }
}
