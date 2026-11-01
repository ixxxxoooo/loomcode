//! Markdown 文件的预览 / 源码 / 分栏切换,以及类似 Typora 的「就地编辑」:
//! 预览按块(段落、标题、列表、代码块…)渲染,点哪一块,那一块就地变成可编辑的源码,失焦后写回全文。
//!
//! @author ixxxxoooo

use gpui_kit::component::input::{Editor, EditorState, InputEvent};
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

/// 一个 Markdown 文档的预览视图(IDE 与 Agent 右侧面板共用)。
///
/// 文本的唯一来源是 `editor`;预览按块渲染,点块就地编辑,失焦或按 Esc 写回 `editor`。
pub struct MdDocView {
    pub editor: Entity<EditorState>,
    pub mode: MdMode,
    edit: Option<MdEdit>,
    /// 写回后同步保存到这个路径(右侧面板用;IDE 自己管保存时为 `None`)
    save_to: Option<PathBuf>,
    id: SharedString,
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

impl MdDocView {
    /// 创建视图。`editor` 文本来源;`save_to` 写回后自动落盘的路径(IDE 传 `None`)。
    pub fn new(editor: Entity<EditorState>, save_to: Option<PathBuf>) -> Self {
        let id = SharedString::from(format!("md-{}", save_to.as_ref().map_or(0, |p| p.as_os_str().len())));
        Self { editor, mode: MdMode::Preview, edit: None, save_to, id }
    }

    /// 切换显示模式;离开预览时先把正在编辑的块写回。
    pub fn set_mode(&mut self, mode: MdMode, window: &mut Window, cx: &mut Context<Self>) {
        self.commit(window, cx);
        self.mode = mode;
        cx.notify();
    }

    /// 开始就地编辑 `range` 这一块;`expect` 是点击时看到的块内容,与现在不一致(文本已变)就忽略。
    fn start_edit(&mut self, range: Range<usize>, expect: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.editor.read(cx).value();
        if text.get(range.clone()).is_none_or(|s| s != expect) {
            return;
        }
        let input = cx.new(|cx| EditorState::new(window, cx).language("markdown").default_value(expect.to_string()));
        let sub = cx.subscribe_in(&input, window, move |this: &mut MdDocView, _, ev: &InputEvent, window, cx| {
            if matches!(ev, InputEvent::Blur) {
                this.commit(window, cx);
            }
        });
        input.update(cx, |i, cx| i.focus(window, cx));
        self.edit = Some(MdEdit { range, input, _sub: sub });
        cx.notify();
    }

    /// 把正在编辑的块写回全文(进撤销栈);没有改动则只退出编辑。
    pub fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.edit.take() else { return };
        let new = edit.input.read(cx).value().to_string();
        let text = self.editor.read(cx).value().to_string();
        let Some(old) = text.get(edit.range.clone()) else { return };
        if old != new {
            let joined = if edit.range.is_empty() && !text.is_empty() && !new.is_empty() {
                format!("{}\n\n{new}", text.trim_end())
            } else {
                format!("{}{new}{}", &text[..edit.range.start], &text[edit.range.end..])
            };
            if let Some(path) = &self.save_to {
                let _ = std::fs::write(path, joined.as_bytes());
            }
            self.editor.update(cx, |e, cx| e.replace_all(joined, window, cx));
        }
        cx.notify();
    }

    /// 就地编辑框:没有边框与底色,只在左侧留一条强调线,看起来像在文档里直接改字。
    fn edit_box(&self, e: &MdEdit, lines: usize, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let h = (lines.max(1) as f32 * 22. + 12.).max(40.);
        div().w_full().h(px(h)).pl_2().border_l_2().border_color(t.primary.opacity(0.7))
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                if ev.keystroke.key == "escape" {
                    this.commit(window, cx);
                }
            }))
            .child(Editor::new(&e.input).bordered(false).appearance(false).size_full()).into_any_element()
    }

    /// 预览(按块,点击就地编辑)。
    fn preview(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let text = self.editor.read(cx).value().to_string();
        let blocks = split_blocks(&text);
        let mut children: Vec<AnyElement> = Vec::new();
        for (i, r) in blocks.iter().enumerate() {
            let src = text[r.clone()].to_string();
            if let Some(e) = self.edit.as_ref().filter(|e| e.range == *r) {
                children.push(self.edit_box(e, src.lines().count(), cx));
                continue;
            }
            let (range, s2) = (r.clone(), src.clone());
            children.push(div().id(SharedString::from(format!("{}-b{i}", self.id))).w_full().px_2().py_1().rounded(px(6.)).cursor_text()
                .hover(|d| d.bg(t.muted.opacity(0.3)))
                .on_click(cx.listener(move |this, _, window, cx| this.start_edit(range.clone(), &s2, window, cx)))
                .child(TextView::markdown(SharedString::from(format!("{}-v{i}", self.id)), src).selectable(true)).into_any_element());
        }
        // 文末空白:点击追加新块
        let end = text.len();
        if let Some(e) = self.edit.as_ref().filter(|e| e.range.is_empty() && e.range.start == end) {
            children.push(self.edit_box(e, 1, cx));
        }
        children.push(div().id(SharedString::from(format!("{}-tail", self.id))).w_full().min_h(px(120.)).flex_1().cursor_text()
            .on_click(cx.listener(move |this, _, window, cx| this.start_edit(end..end, "", window, cx))).into_any_element());
        div().id(SharedString::from(format!("{}-scroll", self.id))).flex_1().min_w_0().h_full().overflow_y_scroll()
            .child(v_flex().w_full().max_w(px(860.)).mx_auto().px_6().py_4().gap_1().min_h_full().children(children)).into_any_element()
    }
}

impl Render for MdDocView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.preview(cx)
    }
}

/// 预览 / 源码 / 分栏切换条(IDE 用)。
pub fn mode_bar<T: 'static>(view: &Entity<MdDocView>, cur: MdMode, cx: &mut Context<T>) -> AnyElement {
    let t = cx.theme().clone();
    h_flex().w_full().h(px(28.)).flex_none().px_3().gap_1().justify_end().items_center().border_b_1().border_color(t.border.opacity(0.6))
        .children([(MdMode::Preview, "预览"), (MdMode::Source, "源码"), (MdMode::Split, "分栏")].into_iter().map(|(m, label)| {
            let v = view.clone();
            let active = m == cur;
            div().id(SharedString::from(format!("md-mode-{label}"))).px_2().h(px(20.)).flex().items_center().rounded(px(4.)).cursor_pointer().text_size(px(12.))
                .when(active, |d| d.bg(t.muted).text_color(t.foreground))
                .when(!active, |d| d.text_color(t.muted_foreground).hover(|d| d.text_color(t.foreground)))
                .on_mouse_down(MouseButton::Left, move |_, window, cx| { v.update(cx, |v, cx| v.set_mode(m, window, cx)); })
                .child(label)
        })).into_any_element()
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
