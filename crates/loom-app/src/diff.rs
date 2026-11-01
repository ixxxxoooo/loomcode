//! 行级 Diff 渲染(统一视图)。

use crate::theme::diff_colors;
use crate::ui::MONO;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use similar::{ChangeTag, TextDiff};

/// 一行 Diff。
#[derive(Debug, Clone, PartialEq)]
pub struct DiffLine {
    pub tag: ChangeTag,
    pub old_no: Option<usize>,
    pub new_no: Option<usize>,
    pub text: String,
}

/// 计算行级 Diff,并统计增删数。
pub fn compute(old: &str, new: &str) -> (Vec<DiffLine>, usize, usize) {
    let diff = TextDiff::from_lines(old, new);
    let (mut added, mut removed) = (0, 0);
    let lines = diff
        .iter_all_changes()
        .map(|c| {
            match c.tag() {
                ChangeTag::Insert => added += 1,
                ChangeTag::Delete => removed += 1,
                ChangeTag::Equal => {}
            }
            DiffLine {
                tag: c.tag(),
                old_no: c.old_index().map(|i| i + 1),
                new_no: c.new_index().map(|i| i + 1),
                text: c.value().trim_end_matches('\n').to_string(),
            }
        })
        .collect();
    (lines, added, removed)
}

/// 渲染 Diff;`max_lines` 限制渲染行数(超长 Diff 先显示前 N 行)。
pub fn render(lines: &[DiffLine], max_lines: usize, cx: &App) -> Div {
    render_with(lines, max_lines, true, cx)
}

/// 无外框版本(嵌在已有卡片里时用,避免框中框):整行底色直接铺到卡片边缘。
pub fn render_flat(lines: &[DiffLine], max_lines: usize, cx: &App) -> Div {
    render_with(lines, max_lines, false, cx)
}

fn render_with(lines: &[DiffLine], max_lines: usize, bordered: bool, cx: &App) -> Div {
    let t = cx.theme();
    let (add_bg, del_bg) = diff_colors(cx);
    let shown = lines.len().min(max_lines);
    let mut col = v_flex().w_full().overflow_hidden().font_family(MONO).text_size(px(12.))
        .when(bordered, |d| d.rounded(px(8.)).border_1().border_color(t.border));
    for line in &lines[..shown] {
        let (bg, mark, fg) = match line.tag {
            ChangeTag::Insert => (Some(add_bg), "+", t.success),
            ChangeTag::Delete => (Some(del_bg), "−", t.danger),
            ChangeTag::Equal => (None, " ", t.muted_foreground),
        };
        let no = line.new_no.or(line.old_no).map(|n| n.to_string()).unwrap_or_default();
        col = col.child(
            h_flex()
                .w_full()
                .when_some(bg, |d, b| d.bg(b))
                .child(div().w(px(36.)).flex_none().px_1().text_right().text_color(t.muted_foreground).child(no))
                .child(div().w(px(14.)).flex_none().text_color(fg).child(mark))
                .child(div().flex_1().min_w_0().pr_2().text_color(t.foreground).child(line.text.clone())),
        );
    }
    if lines.len() > shown {
        col = col.child(
            div().px_2().py_1().text_color(t.muted_foreground).child(format!("… 还有 {} 行未显示", lines.len() - shown)),
        );
    }
    col
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 统计增删行() {
        let (lines, a, r) = compute("a\nb\n", "a\nc\nd\n");
        assert_eq!((a, r), (2, 1));
        assert_eq!(lines.first().unwrap().tag, ChangeTag::Equal);
    }
}
