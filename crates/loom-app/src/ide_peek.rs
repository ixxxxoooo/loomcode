//! 「转到引用」的内嵌预览窗,照 VS Code 的 Peek References 实现(逐项对过它的源码与样式):
//! 紧贴光标行下方,左边是只读代码预览(占 70%,语法高亮,本文件里所有引用都有底色),右边是引用树(占 30%,
//! 多个文件时按文件分组、带引用数徽标;每行是命中词前约 8 个字符的上下文 + 命中词 + 行尾);
//! 标题是「文件名 目录 - 引用 (N)」。单击树里的项预览,双击或回车在编辑器里打开,↑↓ 切换,Esc 关闭。
//! 渲染见 `ide_peek_view.rs`。
//!
//! @author ixxxxoooo

use crate::ide::IdeView;
use crate::ide_overlay::Overlay;
use gpui_kit::*;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

/// 命中词前面保留的上下文字符数(VS Code 的 `FilePreview.preview` 默认 8)。
const CONTEXT_BEFORE: usize = 8;
/// 超过这个大小的文件不做预览。
const MAX_PREVIEW_BYTES: usize = 2_000_000;

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

/// 预览里的文件内容。
pub struct Preview {
    pub path: PathBuf,
    pub lines: Arc<Vec<String>>,
}

/// 预览窗状态。
pub struct PeekState {
    pub items: Vec<RefItem>,
    pub selected: usize,
    pub preview: Option<Preview>,
    /// 打开时光标的偏移:预览窗锚在这一行下面
    pub anchor_offset: usize,
    /// 预览区的滚动句柄(选中项变化后把目标行滚到中间)
    pub scroll: ScrollHandle,
    /// 上次把预览滚到目标行时选中的序号(只在选中项变化时滚动,不抢用户自己的滚动)
    pub scrolled_for: std::cell::Cell<usize>,
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
    /// 由原始引用构造:按文件、行号排序,算好每行的预览文本,载入第一项的文件预览。
    ///
    /// 参数:`raw` 引用;`anchor_offset` 触发时的光标偏移。
    pub fn new(mut raw: Vec<RawRef>, anchor_offset: usize) -> Self {
        raw.sort_by(|a, b| (&a.path, a.line, a.col).cmp(&(&b.path, b.line, b.col)));
        let items = raw
            .into_iter()
            .map(|r| {
                let (text, hit) = preview_text(&r.line_text, r.col, r.end_col);
                RefItem { path: r.path, line: r.line, col: r.col, end_col: r.end_col, text, hit }
            })
            .collect();
        let mut st = Self { items, selected: 0, preview: None, anchor_offset, scroll: ScrollHandle::new(), scrolled_for: std::cell::Cell::new(usize::MAX) };
        st.load_preview();
        st
    }

    /// 载入选中引用所在文件的内容(同一文件不重复读)。
    pub fn load_preview(&mut self) {
        let Some(item) = self.items.get(self.selected) else { return };
        if self.preview.as_ref().is_some_and(|p| p.path == item.path) {
            return;
        }
        self.preview = std::fs::read_to_string(&item.path)
            .ok()
            .filter(|t| t.len() <= MAX_PREVIEW_BYTES)
            .map(|t| Preview { path: item.path.clone(), lines: Arc::new(t.lines().map(String::from).collect()) });
    }

    /// 引用涉及的文件数。
    pub fn file_count(&self) -> usize {
        let mut seen: Vec<&PathBuf> = Vec::new();
        for i in &self.items {
            if !seen.contains(&&i.path) {
                seen.push(&i.path);
            }
        }
        seen.len()
    }
}

impl IdeView {
    /// 选中第 `i` 项并更新预览。
    pub(crate) fn peek_select(&mut self, i: usize, cx: &mut Context<Self>) {
        if let Some(Overlay::Refs(st)) = &mut self.overlay {
            if i < st.items.len() {
                st.selected = i;
                st.load_preview();
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
