//! 右栏「文件」标签左侧的文件预览:只读代码预览(带语法高亮),Markdown 用与 IDE 共用的预览视图。
//!
//! @author ixxxxoooo

use crate::file_tree::language_for;
use crate::right_panel::RightPanel;
use gpui_kit::component::input::{Editor, EditorState};
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;
use std::path::PathBuf;

/// 超过这个大小的文件不预览。
const MAX_PREVIEW_BYTES: u64 = 512 * 1024;

/// 一个已打开的文件预览标签。
pub struct FilePreview {
    pub path: PathBuf,
    editor: Option<Entity<EditorState>>,
    md_view: Option<Entity<crate::ide_markdown::MdDocView>>,
    /// 预览不可用时的说明(二进制、过大、读取失败)
    note: Option<String>,
}

impl FilePreview {
    /// 读取文件并建立预览。
    ///
    /// 参数:`path` 绝对路径。
    pub fn load(path: PathBuf, window: &mut Window, cx: &mut Context<RightPanel>) -> Self {
        let mut this = Self { path: path.clone(), editor: None, md_view: None, note: None };
        match std::fs::metadata(&path) {
            Err(e) => this.note = Some(format!("无法读取:{e}")),
            Ok(m) if m.len() > MAX_PREVIEW_BYTES => this.note = Some(format!("文件较大({} KB),不预览", m.len() / 1024)),
            Ok(_) => match std::fs::read(&path).map(String::from_utf8) {
                Err(e) => this.note = Some(format!("无法读取:{e}")),
                Ok(Err(_)) => this.note = Some("二进制文件,不预览".into()),
                Ok(Ok(text)) => {
                    let editor = cx.new(|cx| EditorState::new(window, cx).language(language_for(&path)).default_value(text));
                    if crate::ide_markdown::is_markdown(&path) {
                        this.md_view = Some(cx.new(|cx| crate::ide_markdown::MdDocView::new(editor.clone(), Some(path.clone()), window, cx)));
                    }
                    this.editor = Some(editor);
                }
            },
        }
        this
    }

    /// 预览内容。
    pub fn render(&self, cx: &App) -> AnyElement {
        let t = cx.theme();
        match (&self.editor, &self.md_view, &self.note) {
            (Some(_), Some(md), _) => div().size_full().child(md.clone()).into_any_element(),
            (Some(ed), _, _) => div().size_full().child(Editor::new(ed).readonly(true).bordered(false).appearance(false).size_full()).into_any_element(),
            (_, _, Some(note)) => div().size_full().flex().items_center().justify_center().text_size(px(12.)).text_color(t.muted_foreground).child(note.clone()).into_any_element(),
            _ => div().into_any_element(),
        }
    }
}
