//! 行级 Diff 渲染(统一视图)。

use gpui_kit::component::{ActiveTheme, v_flex};
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

/// 渲染 Diff(统一视图,带圆角外框;工具卡里用);`max_lines` 限制渲染行数(超长 Diff 先显示前 N 行)。
/// 自绘、可选中复制(见 `diff_code::inline`)。
pub fn render(lines: &[DiffLine], max_lines: usize, lang: &str, cx: &App) -> Div {
    let t = cx.theme();
    div().w_full().overflow_hidden().rounded(px(8.)).border_1().border_color(t.border).child(crate::diff_code::inline_with(lines, max_lines, lang, false, cx))
}

/// 无外框版本(嵌在已有卡片里时用,避免框中框):整行底色直接铺到卡片边缘。
/// 按偏好(设置 → 源代码管理)选左右对比或统一视图,顶部带切换条。
/// 自绘、可选中复制(拖选 / 双击选词 / 三击选行 / ⌘C),见 `diff_code::inline`。
pub fn render_flat(lines: &[DiffLine], max_lines: usize, lang: &str, cx: &App) -> Div {
    let split = crate::prefs(cx).scm.diff_split;
    v_flex().w_full().child(crate::diff_split::mode_bar(split, cx)).child(crate::diff_code::inline(lines, max_lines, lang, cx))
}

/// 给要显示的各行算语法高亮:把这些行拼成一份文本整体解析,再按行切回(每行一组相对偏移的样式)。
///
/// 参数:`lines` 要显示的行;`lang` 语言名(`text` 或解析失败则不着色);`cx` 取语法主题。
pub(crate) fn line_styles(lines: &[DiffLine], lang: &str, cx: &App) -> Vec<Vec<(std::ops::Range<usize>, HighlightStyle)>> {
    let mut out = vec![Vec::new(); lines.len()];
    let text = lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n");
    if lang == "text" || text.is_empty() || text.len() > 200_000 {
        return out;
    }
    let styles = crate::code_hl::styles(&text, lang, cx);
    let mut start = 0;
    for (i, l) in lines.iter().enumerate() {
        let end = start + l.text.len();
        for (r, st) in &styles {
            let (a, b) = (r.start.max(start), r.end.min(end));
            if a < b {
                out[i].push((a - start..b - start, *st));
            }
        }
        start = end + 1;
    }
    out
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
