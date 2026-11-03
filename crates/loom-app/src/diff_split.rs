//! Diff 的左右对比视图:把行级 Diff 配成「左旧右新」的行对,并渲染两栏;
//! 顶部的「左右 / 统一」切换条也在这里(切换写进偏好,所有差异块一起变)。
//!
//! @author ixxxxoooo

use crate::diff::DiffLine;
use gpui_kit::component::{ActiveTheme, h_flex};
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
    fn 纯新增与纯删除一侧为空() {
        let (lines, _, _) = compute("a\n", "a\nb\n");
        let rows = pair_rows(&lines);
        assert!(rows[1].left.is_none() && rows[1].right.is_some());
        let (lines, _, _) = compute("a\nb\n", "a\n");
        let rows = pair_rows(&lines);
        assert!(rows[1].left.is_some() && rows[1].right.is_none());
    }
}
