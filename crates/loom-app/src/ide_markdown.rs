//! Markdown 文件的预览 / 源码 / 分栏切换,以及类似 Typora 的「就地编辑」:
//! 预览按块(段落、标题、列表、代码块…)渲染,点哪一块,那一块就地变成可编辑的源码,失焦后写回全文。
//!
//! @author ixxxxoooo

use crate::ide::IdeView;
use gpui_kit::component::input::{EditorState, InputEvent};
use gpui_kit::component::text::TextView;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::ops::Range;
use std::path::{Path, PathBuf};

/// Markdown 的显示模式。
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum MdMode {
    /// 渲染预览(默认),点击块就地编辑
    #[default]
    Preview,
    /// 纯源码编辑器
    Source,
    /// 左源码右预览
    Split,
}

/// 正在就地编辑的一个块。
pub struct MdEdit {
    /// 该块在全文中的字节区间(不含块后的空行)
    pub range: Range<usize>,
    pub input: Entity<EditorState>,
    _sub: Subscription,
}

/// 单个 Markdown 文件的界面状态。
#[derive(Default)]
pub struct MdUi {
    pub mode: MdMode,
    pub edit: Option<MdEdit>,
}

/// 是否 Markdown 文件。
pub fn is_markdown(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"))
}

/// 把全文切成顶层块:以空行分隔,围栏代码块(``` / ~~~)内的空行不分隔。
///
/// 参数:`text` 全文。返回:每块的字节区间(不含末尾换行)。
pub fn split_blocks(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let (mut start, mut fence, mut offset) = (None::<usize>, None::<String>, 0usize);
    let mut last_end = 0usize;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        let t = trimmed.trim_start();
        let mark = if t.starts_with("```") { Some("```") } else if t.starts_with("~~~") { Some("~~~") } else { None };
        match (&fence, mark) {
            (None, Some(m)) => fence = Some(m.to_string()),
            (Some(f), Some(m)) if f == m => fence = None,
            _ => {}
        }
        if trimmed.trim().is_empty() && fence.is_none() {
            if let Some(s) = start.take() {
                out.push(s..last_end);
            }
        } else {
            start.get_or_insert(offset);
            last_end = offset + trimmed.len();
        }
        offset += line.len();
    }
    if let Some(s) = start {
        out.push(s..last_end);
    }
    out
}

impl IdeView {
    /// 当前文件的 Markdown 显示模式。
    pub(crate) fn md_mode(&self, path: &Path) -> MdMode {
        self.md.get(path).map_or(MdMode::Preview, |u| u.mode)
    }

    /// 切换显示模式;离开预览时先把正在编辑的块写回。
    pub(crate) fn set_md_mode(&mut self, path: &Path, mode: MdMode, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_md_edit(path, window, cx);
        self.md.entry(path.to_path_buf()).or_default().mode = mode;
        cx.notify();
    }

    /// 开始就地编辑 `range` 这一块;`expect` 是点击时看到的块内容,与现在不一致(文本已变)就忽略。
    pub(crate) fn start_md_edit(&mut self, path: &Path, range: Range<usize>, expect: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.docs.get(path).map(|d| d.editor.clone()) else { return };
        let text = editor.read(cx).value();
        if text.get(range.clone()).is_none_or(|s| s != expect) {
            return;
        }
        let lines = expect.lines().count().max(1);
        let _ = lines;
        let input = cx.new(|cx| EditorState::new(window, cx).language("markdown").default_value(expect.to_string()));
        let key = path.to_path_buf();
        let sub = cx.subscribe_in(&input, window, move |this: &mut IdeView, _, ev: &InputEvent, window, cx| {
            if matches!(ev, InputEvent::Blur) {
                this.commit_md_edit(&key, window, cx);
            }
        });
        input.update(cx, |i, cx| i.focus(window, cx));
        self.md.entry(path.to_path_buf()).or_default().edit = Some(MdEdit { range, input, _sub: sub });
        cx.notify();
    }

    /// 把正在编辑的块写回全文(进撤销栈);没有改动则只退出编辑。
    pub(crate) fn commit_md_edit(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.md.get_mut(path).and_then(|u| u.edit.take()) else { return };
        let Some(editor) = self.docs.get(path).map(|d| d.editor.clone()) else { return };
        let new = edit.input.read(cx).value().to_string();
        let text = editor.read(cx).value().to_string();
        let Some(old) = text.get(edit.range.clone()) else { return };
        if old != new {
            let joined = if edit.range.is_empty() && !text.is_empty() && !new.is_empty() {
                format!("{}\n\n{new}", text.trim_end())
            } else {
                format!("{}{new}{}", &text[..edit.range.start], &text[edit.range.end..])
            };
            editor.update(cx, |e, cx| e.replace_all(joined, window, cx));
        }
        cx.notify();
    }

    /// 渲染 Markdown 预览(按块,点击就地编辑)。
    pub(crate) fn md_preview(&self, path: &Path, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let Some(doc) = self.docs.get(path) else { return div().into_any_element() };
        let text = doc.editor.read(cx).value().to_string();
        let edit = self.md.get(path).and_then(|u| u.edit.as_ref());
        let blocks = split_blocks(&text);
        let mut children: Vec<AnyElement> = Vec::new();
        for (i, r) in blocks.iter().enumerate() {
            let src = text[r.clone()].to_string();
            if let Some(e) = edit.filter(|e| e.range == *r) {
                let h = (src.lines().count().max(1) as f32 * 22. + 24.).max(52.);
                children.push(div().w_full().h(px(h)).rounded(px(6.)).border_1().border_color(t.primary.opacity(0.5)).bg(t.muted.opacity(0.4))
                    .child(gpui_kit::component::input::Editor::new(&e.input).bordered(false).appearance(false).size_full()).into_any_element());
                continue;
            }
            let (p, range, s2) = (path.to_path_buf(), r.clone(), src.clone());
            children.push(div().id(SharedString::from(format!("mdb-{i}"))).w_full().px_2().py_1().rounded(px(6.)).cursor_text()
                .hover(|d| d.bg(t.muted.opacity(0.35)))
                .on_click(cx.listener(move |this, _, window, cx| this.start_md_edit(&p, range.clone(), &s2, window, cx)))
                .child(TextView::markdown(SharedString::from(format!("mdv-{}-{i}", path.display())), src).selectable(true)).into_any_element());
        }
        // 底部空白:点击在文末追加新块
        let (p, end) = (path.to_path_buf(), text.len());
        let tail_editing = edit.is_some_and(|e| e.range.start == end && e.range.is_empty());
        if tail_editing {
            if let Some(e) = edit {
                children.push(div().w_full().h(px(52.)).rounded(px(6.)).border_1().border_color(t.primary.opacity(0.5))
                    .child(gpui_kit::component::input::Editor::new(&e.input).bordered(false).appearance(false).size_full()).into_any_element());
            }
        }
        children.push(div().id("md-tail").w_full().min_h(px(120.)).flex_1().cursor_text()
            .on_click(cx.listener(move |this, _, window, cx| this.start_md_edit(&p, end..end, "", window, cx))).into_any_element());
        div().id("md-preview").flex_1().min_w_0().h_full().overflow_y_scroll()
            .child(v_flex().w_full().max_w(px(860.)).mx_auto().px_8().py_5().gap_1().min_h_full().children(children)).into_any_element()
    }

    /// 预览 / 源码 / 分栏切换条。
    pub(crate) fn md_mode_bar(&self, path: &Path, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let cur = self.md_mode(path);
        h_flex().w_full().h(px(28.)).flex_none().px_3().gap_1().justify_end().items_center().border_b_1().border_color(t.border.opacity(0.6))
            .children([(MdMode::Preview, "预览"), (MdMode::Source, "源码"), (MdMode::Split, "分栏")].into_iter().map(|(m, label)| {
                let p: PathBuf = path.to_path_buf();
                let active = m == cur;
                div().id(SharedString::from(format!("md-mode-{label}"))).px_2().h(px(20.)).flex().items_center().rounded(px(4.)).cursor_pointer().text_size(px(12.))
                    .when(active, |d| d.bg(t.muted).text_color(t.foreground))
                    .when(!active, |d| d.text_color(t.muted_foreground).hover(|d| d.text_color(t.foreground)))
                    .on_click(cx.listener(move |this, _, window, cx| this.set_md_mode(&p, m, window, cx)))
                    .child(label)
            })).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn splits_on_blank_lines() {
        let t = "# 标题\n\n段落一\n接着\n\n- a\n- b\n";
        let b: Vec<&str> = split_blocks(t).into_iter().map(|r| &t[r]).collect();
        assert_eq!(b, vec!["# 标题", "段落一\n接着", "- a\n- b"]);
    }

    #[::core::prelude::v1::test]
    fn fenced_code_keeps_blank_lines() {
        let t = "前\n\n```rs\nlet a = 1;\n\nlet b = 2;\n```\n\n后";
        let b: Vec<&str> = split_blocks(t).into_iter().map(|r| &t[r]).collect();
        assert_eq!(b.len(), 3);
        assert!(b[1].starts_with("```rs") && b[1].ends_with("```") && b[1].contains("\n\n"));
    }

    #[::core::prelude::v1::test]
    fn empty_and_blank_text() {
        assert!(split_blocks("").is_empty());
        assert!(split_blocks("\n\n  \n").is_empty());
    }
}
