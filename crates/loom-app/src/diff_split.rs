//! Diff 的左右对比视图:把行级 Diff 配成「左旧右新」的行对,并渲染两栏;
//! 顶部的「左右 / 统一」切换条也在这里(切换写进偏好,所有差异块一起变)。
//!
//! @author ixxxxoooo

use crate::diff::{DiffLine, line_styles};
use crate::theme::diff_colors;
use crate::ui::MONO;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use similar::ChangeTag;

/// 左右对比的一行:左边是旧文件的行,右边是新文件的行;一边没有内容时为 `None`。
#[derive(Debug, Clone, PartialEq)]
pub struct SplitRow {
    pub left: Option<DiffLine>,
    pub right: Option<DiffLine>,
}

/// 把统一视图的行配成左右对比:相同行两边都放;一段连续的删除与紧随的新增按顺序逐行配对,多出的一侧留空。
///
/// 参数:`lines` 行级 Diff。返回:左右对比的行。
pub fn pair_rows(lines: &[DiffLine]) -> Vec<SplitRow> {
    let mut rows = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        match lines[i].tag {
            ChangeTag::Equal => {
                rows.push(SplitRow { left: Some(lines[i].clone()), right: Some(lines[i].clone()) });
                i += 1;
            }
            _ => {
                let start = i;
                while i < lines.len() && lines[i].tag == ChangeTag::Delete {
                    i += 1;
                }
                let dels = &lines[start..i];
                let ins_start = i;
                while i < lines.len() && lines[i].tag == ChangeTag::Insert {
                    i += 1;
                }
                let inss = &lines[ins_start..i];
                for k in 0..dels.len().max(inss.len()) {
                    rows.push(SplitRow { left: dels.get(k).cloned(), right: inss.get(k).cloned() });
                }
            }
        }
    }
    rows
}

/// 整段差异都是新增、或都是删除(没有任何未改动行 / 另一种改动):左右对比时有一侧永远是空的。
pub fn is_one_sided(lines: &[DiffLine]) -> bool {
    !lines.is_empty() && (lines.iter().all(|l| l.tag == ChangeTag::Insert) || lines.iter().all(|l| l.tag == ChangeTag::Delete))
}

/// 一侧的单元格:行号 + 带高亮的文本;`None` 画成留空的斜纹底(这里用淡底代替)。
fn side(line: Option<&DiffLine>, no: Option<usize>, bg: Option<Hsla>, st: Vec<(std::ops::Range<usize>, HighlightStyle)>, cx: &App) -> Div {
    let t = cx.theme();
    let cell = h_flex().flex_1().min_w_0().overflow_hidden();
    match line {
        None => cell.bg(t.muted.opacity(0.35)).child(div().w(px(36.)).flex_none()).child(div().flex_1().child(" ")),
        Some(l) => cell.when_some(bg, |d, b| d.bg(b))
            .child(div().w(px(36.)).flex_none().px_1().text_right().text_color(t.muted_foreground).child(no.map(|n| n.to_string()).unwrap_or_default()))
            .child(div().flex_1().min_w_0().px_1().pr_2().overflow_hidden().whitespace_nowrap().text_color(t.foreground).child(StyledText::new(l.text.clone()).with_highlights(st))),
    }
}

/// 渲染左右对比。`max_lines` 限制渲染的对数;`lang` 决定语法着色(左右两栏各自解析,旧 / 新文件互不干扰)。
pub fn render_split(lines: &[DiffLine], max_lines: usize, lang: &str, cx: &App) -> Div {
    // 纯新增(新文件)或纯删除:一侧永远是空的,不画空栏,整行只显示有内容的那一侧
    if is_one_sided(lines) {
        return crate::diff::render_with(lines, max_lines, false, lang, cx);
    }
    let t = cx.theme();
    let (add_bg, del_bg) = diff_colors(cx);
    let rows = pair_rows(lines);
    let shown = rows.len().min(max_lines);
    let left: Vec<DiffLine> = rows[..shown].iter().filter_map(|r| r.left.clone()).collect();
    let right: Vec<DiffLine> = rows[..shown].iter().filter_map(|r| r.right.clone()).collect();
    let (mut ls, mut rs) = (line_styles(&left, lang, cx).into_iter(), line_styles(&right, lang, cx).into_iter());
    let mut col = v_flex().w_full().overflow_hidden().font_family(MONO).text_size(px(12.));
    for r in &rows[..shown] {
        let lst = r.left.as_ref().and_then(|_| ls.next()).unwrap_or_default();
        let rst = r.right.as_ref().and_then(|_| rs.next()).unwrap_or_default();
        let (lbg, rbg) = (r.left.as_ref().filter(|l| l.tag == ChangeTag::Delete).map(|_| del_bg), r.right.as_ref().filter(|l| l.tag == ChangeTag::Insert).map(|_| add_bg));
        col = col.child(
            h_flex().w_full()
                .child(side(r.left.as_ref(), r.left.as_ref().and_then(|l| l.old_no), lbg, lst, cx))
                .child(div().w(px(1.)).flex_none().self_stretch().bg(t.border))
                .child(side(r.right.as_ref(), r.right.as_ref().and_then(|l| l.new_no), rbg, rst, cx)),
        );
    }
    if rows.len() > shown {
        col = col.child(div().px_2().py_1().text_color(t.muted_foreground).child(format!("… 还有 {} 行未显示", rows.len() - shown)));
    }
    col
}

/// 「左右 / 统一」切换条:点哪个就把偏好改成哪个(经工作台保存并让所有差异块一起变)。
pub fn mode_bar(split: bool, cx: &App) -> Div {
    let t = cx.theme();
    let btn = |id: &'static str, label: &'static str, on: bool, target: bool| {
        div().id(id).px_1p5().h(px(18.)).flex().items_center().rounded(px(4.)).cursor_pointer().text_size(px(11.))
            .when(on, |d| d.bg(t.list_active).text_color(t.foreground))
            .when(!on, |d| d.text_color(t.muted_foreground).hover(|d| d.bg(t.list_hover)))
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                if let Some(wb) = cx.try_global::<crate::WorkbenchRef>().and_then(|r| r.0.upgrade()) {
                    wb.update(cx, |wb, cx| wb.update_settings(cx, |s| s.scm.diff_split = target));
                }
            })
            .child(label)
    };
    h_flex().w_full().justify_end().gap_0p5().px_1().py_0p5().border_b_1().border_color(t.border)
        .child(btn("diff-mode-split", "左右对比", split, true))
        .child(btn("diff-mode-unified", "统一视图", !split, false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::compute;

    #[::core::prelude::v1::test]
    fn 删除与新增逐行配对_多出的留空() {
        let (lines, _, _) = compute("a\nb\nc\nd\n", "a\nX\nY\nZ\nd\n");
        let rows = pair_rows(&lines);
        // a | a,  b↔X,  c↔Y,  —↔Z,  d | d
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[1].left.as_ref().unwrap().text, "b");
        assert_eq!(rows[1].right.as_ref().unwrap().text, "X");
        assert!(rows[3].left.is_none() && rows[3].right.as_ref().unwrap().text == "Z");
    }

    #[::core::prelude::v1::test]
    fn 整段纯新增或纯删除只留一侧() {
        assert!(is_one_sided(&compute("", "a\nb\n").0));
        assert!(is_one_sided(&compute("a\nb\n", "").0));
        assert!(!is_one_sided(&compute("a\n", "a\nb\n").0));
        assert!(!is_one_sided(&compute("a\n", "b\n").0));
        assert!(!is_one_sided(&[]));
    }

    #[::core::prelude::v1::test]
    fn 纯新增与纯删除一侧为空() {
        let (lines, _, _) = compute("a\n", "a\nb\n");
        let rows = pair_rows(&lines);
        assert!(rows[1].left.is_none() && rows[1].right.is_some());
        let (lines, _, _) = compute("a\nb\n", "a\n");
        let rows = pair_rows(&lines);
        assert!(rows[1].left.is_some() && rows[1].right.is_none());
    }
}
