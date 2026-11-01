//! Markdown 文件的所见即所得视图(IDE 与 Agent 右侧面板共用)。
//!
//! 基于开源的 `gpui-kit-markdown-editor`(块级所见即所得:标题、列表、任务列表、引用、
//! 围栏代码、表格、链接与粗 / 斜 / 删除线 / 行内代码,键入 `# `、`- `、``` 等自动转换)。
//! 文本的唯一来源是源码编辑器 `editor`:所见即所得里的改动写回它(IDE 由此标记未保存,右侧面板直接落盘),
//! 在源码里改动则同步到所见即所得。注意:第一次在所见即所得里编辑后,整份文档会按规范 Markdown 重新输出。
//!
//! @author ixxxxoooo

use gpui_kit::component::input::{EditorState, InputEvent};
use gpui_kit::component::{ActiveTheme, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use markdown_editor::{BlockEditor, BlockEditorEvent, BlockEditorState, EditOrigin};
use std::path::{Path, PathBuf};

/// Markdown 的显示模式。
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum MdMode {
    /// 所见即所得(默认)
    #[default]
    Preview,
    /// 纯源码编辑器
    Source,
    /// 左源码右所见即所得
    Split,
}

/// 是否 Markdown 文件。
pub fn is_markdown(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"))
}

/// 一个 Markdown 文档的所见即所得视图。
pub struct MdDocView {
    /// 源码编辑器:文本的唯一来源
    pub editor: Entity<EditorState>,
    pub mode: MdMode,
    block: Entity<BlockEditorState>,
    scroll: ScrollHandle,
    /// 写回后同步保存到这个路径(右侧面板用;IDE 自己管保存时为 `None`)
    save_to: Option<PathBuf>,
    _subs: Vec<Subscription>,
}

impl MdDocView {
    /// 创建视图。`editor` 文本来源;`save_to` 写回后自动落盘的路径(IDE 传 `None`)。
    pub fn new(editor: Entity<EditorState>, save_to: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let scroll = ScrollHandle::new();
        let text = editor.read(cx).value().to_string();
        let block = cx.new(|cx| {
            let mut s = BlockEditorState::new(window, cx).with_source(&text);
            s.set_scroll_handle(scroll.clone());
            s
        });
        // 所见即所得里的用户编辑 → 写回源码编辑器(撤销 / 重做与程序化载入不回写,避免循环)
        let to_editor = cx.subscribe_in(&block, window, |this: &mut MdDocView, block, ev: &BlockEditorEvent, window, cx| {
            let BlockEditorEvent::Changed { origin, .. } = ev else { return };
            if matches!(origin, EditOrigin::Api) {
                return;
            }
            let src = block.read(cx).source();
            if this.editor.read(cx).value().as_ref() == src.as_str() {
                return;
            }
            if let Some(path) = &this.save_to {
                let _ = std::fs::write(path, src.as_bytes());
            }
            this.editor.update(cx, |e, cx| e.replace_all(src, window, cx));
        });
        // 源码里的改动 → 同步到所见即所得(内容相同就跳过,所以不会来回触发)
        let to_block = cx.subscribe(&editor, |this: &mut MdDocView, editor, ev: &InputEvent, cx| {
            if !matches!(ev, InputEvent::Change) {
                return;
            }
            let text = editor.read(cx).value().to_string();
            if this.block.read(cx).source() != text {
                this.block.update(cx, |b, cx| { let _ = b.set_source(&text, cx); });
            }
        });
        Self { editor, mode: MdMode::Preview, block, scroll, save_to, _subs: vec![to_editor, to_block] }
    }

    /// 切换显示模式。
    pub fn set_mode(&mut self, mode: MdMode, cx: &mut Context<Self>) {
        self.mode = mode;
        cx.notify();
    }
}

impl Render for MdDocView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().id("md-wysiwyg").flex_1().min_w_0().h_full().overflow_y_scroll().track_scroll(&self.scroll)
            .child(div().w_full().max_w(px(860.)).mx_auto().min_h_full().child(BlockEditor::new(&self.block).min_h_full().px_6().py_4()))
            .bg(cx.theme().background)
    }
}

/// 所见即所得 / 源码 / 分栏切换条(IDE 用)。
pub fn mode_bar<T: 'static>(view: &Entity<MdDocView>, cur: MdMode, cx: &mut Context<T>) -> AnyElement {
    let t = cx.theme().clone();
    h_flex().w_full().h(px(28.)).flex_none().px_3().gap_1().justify_end().items_center().border_b_1().border_color(t.border.opacity(0.6))
        .children([(MdMode::Preview, "预览"), (MdMode::Source, "源码"), (MdMode::Split, "分栏")].into_iter().map(|(m, label)| {
            let v = view.clone();
            let active = m == cur;
            div().id(SharedString::from(format!("md-mode-{label}"))).px_2().h(px(20.)).flex().items_center().rounded(px(4.)).cursor_pointer().text_size(px(12.))
                .when(active, |d| d.bg(t.muted).text_color(t.foreground))
                .when(!active, |d| d.text_color(t.muted_foreground).hover(|d| d.text_color(t.foreground)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| { v.update(cx, |v, cx| v.set_mode(m, cx)); })
                .child(label)
        })).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn recognizes_markdown_extensions() {
        assert!(is_markdown(Path::new("a/B.MD")));
        assert!(is_markdown(Path::new("x.markdown")));
        assert!(!is_markdown(Path::new("x.rs")));
    }
}
