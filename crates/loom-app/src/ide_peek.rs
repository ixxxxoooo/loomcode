//! 「转到引用」的内嵌预览窗,照 VS Code 的 Peek References 实现(逐项对过它的源码与样式):
//! 紧贴光标行下方,左边是只读代码预览(占 70%,语法高亮,本文件里所有引用都有底色),右边是引用树(占 30%,
//! 多个文件时按文件分组、带引用数徽标;每行是命中词前约 8 个字符的上下文 + 命中词 + 行尾);
//! 标题是「文件名 目录 - 引用 (N)」。单击树里的项预览,双击或回车在编辑器里打开,↑↓ 切换,Esc 关闭。
//! 渲染见 `ide_peek_view.rs`。
//!
//! @author ixxxxoooo

use crate::ide::IdeView;
use crate::ide_overlay::Overlay;
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

/// 命中词前面保留的上下文字符数(VS Code 的 `FilePreview.preview` 默认 8)。
const CONTEXT_BEFORE: usize = 8;
/// 超过这个大小的文件不做预览。
const MAX_PREVIEW_BYTES: usize = 2_000_000;
/// 预览里目标行前后各载入多少行(只载入、着色这一段;渲染是虚拟列表,只画可见行)。
const WINDOW: usize = 1000;

/// 服务器给的一处引用(行 / 列都是 0 起的字符位置)。
#[derive(Debug, Clone, PartialEq)]
pub struct RawRef {
    pub path: PathBuf,
    pub line: u32,
    pub col: u32,
    pub end_col: u32,
    pub line_text: String,
}

/// 树里的一项:预览文本与其中命中词的字节区间。
#[derive(Debug, Clone, PartialEq)]
pub struct RefItem {
    pub path: PathBuf,
    pub line: u32,
    pub col: u32,
    pub end_col: u32,
    pub text: String,
    pub hit: Range<usize>,
}

/// 预览里的一行:行号、文本、已算好的语法高亮与引用底色(只在载入时算一次)。
#[derive(Clone)]
pub struct PLine {
    pub ln: usize,
    pub text: String,
    pub hl: Vec<(Range<usize>, HighlightStyle)>,
}

/// 预览里的文件内容(目标行前后一段)。
pub struct Preview {
    pub path: PathBuf,
    /// `lines[0]` 对应文件第 `first` 行
    pub first: usize,
    pub lines: Arc<Vec<PLine>>,
}

/// 树里的一行:文件头(名称、目录、引用数)或一处引用(`items` 下标)。
#[derive(Clone)]
pub enum TreeRow {
    File { name: String, dir: String, count: usize },
    Ref(usize),
}

/// 预览窗状态。
pub struct PeekState {
    pub items: Vec<RefItem>,
    pub selected: usize,
    pub preview: Option<Preview>,
    /// 树的行(文件头 + 引用),打开时算好
    pub rows: Arc<Vec<TreeRow>>,
    /// 树行文本与命中区间(渲染时共享,不每帧复制)
    pub texts: Arc<Vec<(String, Range<usize>)>>,
    pub file_count: usize,
    /// 打开时光标的偏移:预览窗锚在这一行下面
    pub anchor_offset: usize,
    pub scroll_tree: UniformListScrollHandle,
    pub scroll_preview: UniformListScrollHandle,
}

/// 命中词前的上下文起点:从 `col - 8` 往前吃掉连着的单词字符(VS Code 的 `getWordUntilPosition`)。
fn context_start(chars: &[char], col: usize) -> usize {
    let p = col.saturating_sub(CONTEXT_BEFORE).min(chars.len());
    let mut s = p;
    while s > 0 && (chars[s - 1].is_alphanumeric() || chars[s - 1] == '_') {
        s -= 1;
    }
    s
}

/// 树里一行的文本:`去掉行首空白的前文 + 命中词 + 去掉行尾空白的后文`,以及命中词在其中的字节区间。
///
/// 参数:`line` 整行;`col` / `end_col` 命中词的字符列区间。
pub fn preview_text(line: &str, col: u32, end_col: u32) -> (String, Range<usize>) {
    let chars: Vec<char> = line.chars().collect();
    let (c, e) = ((col as usize).min(chars.len()), (end_col as usize).min(chars.len()).max(col as usize));
    let start = context_start(&chars, c);
    let before: String = chars[start..c].iter().collect::<String>().trim_start().to_string();
    let inside: String = chars[c..e.max(c)].iter().collect();
    let after: String = chars[e.max(c)..].iter().collect::<String>().trim_end().to_string();
    let hit = before.len()..before.len() + inside.len();
    (format!("{before}{inside}{after}"), hit)
}

impl PeekState {
    /// 由原始引用构造:按文件、行号排序,算好树的行与每行的预览文本,载入第一项的文件预览。
    ///
    /// 参数:`raw` 引用;`anchor_offset` 触发时的光标偏移;`dark` 是否深色主题(预览底色用)。
    pub fn new(mut raw: Vec<RawRef>, anchor_offset: usize, dark: bool, cx: &App) -> Self {
        raw.sort_by(|a, b| (&a.path, a.line, a.col).cmp(&(&b.path, b.line, b.col)));
        let items: Vec<RefItem> = raw
            .into_iter()
            .map(|r| {
                let (text, hit) = preview_text(&r.line_text, r.col, r.end_col);
                RefItem { path: r.path, line: r.line, col: r.col, end_col: r.end_col, text, hit }
            })
            .collect();
        let mut counts: Vec<(PathBuf, usize)> = Vec::new();
        for it in &items {
            match counts.last_mut() {
                Some((p, n)) if *p == it.path => *n += 1,
                _ => counts.push((it.path.clone(), 1)),
            }
        }
        let multi = counts.len() > 1;
        let mut rows = Vec::with_capacity(items.len() + counts.len());
        let mut last: Option<&PathBuf> = None;
        for (i, it) in items.iter().enumerate() {
            if multi && last != Some(&it.path) {
                last = Some(&it.path);
                let n = counts.iter().find(|(p, _)| *p == it.path).map_or(0, |c| c.1);
                rows.push(TreeRow::File {
                    name: it.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                    dir: it.path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                    count: n,
                });
            }
            rows.push(TreeRow::Ref(i));
        }
        let texts = Arc::new(items.iter().map(|i| (i.text.clone(), i.hit.clone())).collect::<Vec<_>>());
        let mut st = Self { items, selected: 0, preview: None, rows: Arc::new(rows), texts, file_count: counts.len(), anchor_offset, scroll_tree: UniformListScrollHandle::new(), scroll_preview: UniformListScrollHandle::new() };
        st.select(0, dark, cx);
        st
    }

    /// 树里第 `i` 项引用所在的行号(含文件头)。
    pub fn row_of(&self, i: usize) -> usize {
        self.rows.iter().position(|r| matches!(r, TreeRow::Ref(j) if *j == i)).unwrap_or(0)
    }

    /// 选中第 `i` 项:换文件(或目标超出已载入的范围)才重新读文件并算一次语法高亮与底色,然后把预览滚到目标行、树滚到选中行。
    pub fn select(&mut self, i: usize, dark: bool, cx: &App) {
        let Some(item) = self.items.get(i).cloned() else { return };
        self.selected = i;
        let in_range = self.preview.as_ref().is_some_and(|p| p.path == item.path && (item.line as usize) >= p.first && (item.line as usize) < p.first + p.lines.len());
        if !in_range {
            self.preview = load_preview(&item, &self.items, dark, cx);
        }
        if let Some(p) = &self.preview {
            self.scroll_preview.scroll_to_item((item.line as usize).saturating_sub(p.first), ScrollStrategy::Center);
        }
        self.scroll_tree.scroll_to_item(self.row_of(i), ScrollStrategy::Nearest);
    }
}

/// 读文件并算好目标行前后 `WINDOW` 行的语法高亮与引用底色。
fn load_preview(item: &RefItem, items: &[RefItem], dark: bool, cx: &App) -> Option<Preview> {
    let text = std::fs::read_to_string(&item.path).ok().filter(|t| t.len() <= MAX_PREVIEW_BYTES)?;
    let all: Vec<&str> = text.lines().collect();
    let first = (item.line as usize).saturating_sub(WINDOW);
    let last = (item.line as usize + WINDOW).min(all.len());
    let slice = &all[first.min(last)..last];
    let styles = crate::code_hl::styles(&slice.join("\n"), crate::file_tree::language_for(&item.path), cx);
    let per = crate::code_hl::split_by_line(slice, &styles);
    let bg = crate::ide_peek_view::match_background(dark);
    let lines = slice
        .iter()
        .enumerate()
        .map(|(k, l)| {
            let ln = first + k;
            let mut hl = per[k].clone();
            // 本文件里的每处引用都加底色(VS Code 的 reference-decoration)
            for r in items.iter().filter(|r| r.path == item.path && r.line as usize == ln) {
                let byte = |col: u32| l.char_indices().nth(col as usize).map_or(l.len(), |(i, _)| i);
                let (a, b) = (byte(r.col), byte(r.end_col));
                hl = crate::code_hl::overlay_background(&hl, a..b.max(a), bg);
            }
            PLine { ln, text: l.to_string(), hl }
        })
        .collect();
    Some(Preview { path: item.path.clone(), first, lines: Arc::new(lines) })
}

impl IdeView {
    /// 选中第 `i` 项并更新预览。
    pub(crate) fn peek_select(&mut self, i: usize, cx: &mut Context<Self>) {
        if let Some(Overlay::Refs(st)) = &mut self.overlay {
            if i < st.items.len() {
                let dark = cx.theme().is_dark();
                st.select(i, dark, cx);
                cx.notify();
            }
        }
    }

    /// ↑ / ↓ 切换选中项(首尾循环)。
    pub(crate) fn peek_move(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(Overlay::Refs(st)) = &self.overlay {
            let n = st.items.len() as isize;
            if n > 0 {
                let next = (st.selected as isize + delta).rem_euclid(n) as usize;
                self.peek_select(next, cx);
            }
        }
    }

    /// 在编辑器里打开第 `i` 项并关闭预览窗。
    pub(crate) fn peek_open(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Overlay::Refs(st)) = &self.overlay else { return };
        let Some(r) = st.items.get(i).cloned() else { return };
        self.close_overlay(window, cx);
        self.open_at(r.path, r.line, r.col, window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 预览文本取命中词前约八个字符的单词边界() {
        // 命中词 target 在第 20 列:往前 8 个字符是 `rst_arg_` 中间,再吃掉连着的单词字符
        let line = "    let x = first_arg_target(1);";
        let col = line.find("target").unwrap() as u32;
        let (text, hit) = preview_text(line, col, col + 6);
        assert_eq!(&text[hit.clone()], "target");
        assert!(text.starts_with("first_arg_target"), "{text}");
        assert!(text.ends_with("(1);"));
    }

    #[::core::prelude::v1::test]
    fn 行首空白与行尾空白被去掉() {
        let (text, hit) = preview_text("        foo()   ", 8, 11);
        assert_eq!(text, "foo()");
        assert_eq!(&text[hit], "foo");
    }

    #[::core::prelude::v1::test]
    fn 中文行按字符列取() {
        let (text, hit) = preview_text("变量 名称 = 1", 3, 5);
        assert_eq!(&text[hit], "名称");
    }
}
