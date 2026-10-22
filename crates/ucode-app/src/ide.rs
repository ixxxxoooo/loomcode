//! IDE 模式:中栏左侧是单文件编辑器(不做多标签),右侧保留对话。
//! 文件从右侧「文件」面板选择;编辑后 ⌘S 保存。

use crate::file_tree::language_for;
use crate::ui::{MONO, empty_state, icon};
use crate::workbench::Workbench;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Editor, EditorState, InputEvent};
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::path::{Path, PathBuf};

/// 超过这个大小的文件不在编辑器里打开。
const MAX_EDIT_BYTES: u64 = 1024 * 1024;

/// 当前在编辑器里打开的文件。
pub struct IdeDoc {
    pub path: PathBuf,
    pub editor: Entity<EditorState>,
    /// 内容是否与磁盘一致
    pub saved: bool,
    _sub: Subscription,
}

/// 读取文本文件并建立编辑器;二进制或过大的文件返回说明。
pub fn load(path: &Path, window: &mut Window, cx: &mut Context<Workbench>) -> Result<IdeDoc, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("无法读取 {}:{e}", path.display()))?;
    if meta.len() > MAX_EDIT_BYTES {
        return Err(format!("文件较大({} KB),不在编辑器中打开", meta.len() / 1024));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("无法读取 {}:{e}", path.display()))?;
    let text = String::from_utf8(bytes).map_err(|_| "二进制文件,不在编辑器中打开".to_string())?;
    let lang = language_for(path);
    let editor = cx.new(|cx| EditorState::new(window, cx).language(lang).default_value(text));
    // 编辑后标记为未保存
    let sub = cx.subscribe(&editor, |this: &mut Workbench, _, ev: &InputEvent, cx| {
        if matches!(ev, InputEvent::Change)
            && let Some(doc) = this.ide_doc.as_mut()
            && doc.saved
        {
            doc.saved = false;
            cx.notify();
        }
    });
    Ok(IdeDoc { path: path.to_path_buf(), editor, saved: true, _sub: sub })
}

/// 渲染编辑器面板。
pub fn render(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let Some(doc) = &wb.ide_doc else {
        let hint = wb.ide_note.clone().unwrap_or_else(|| "在右侧「文件」面板里选择一个文件".into());
        return empty_state(IconName::Code, "IDE 模式", &hint, cx).into_any_element();
    };
    let root = wb.active_project_path().unwrap_or_default();
    let shown = doc.path.strip_prefix(&root).unwrap_or(&doc.path).display().to_string();
    let saved = doc.saved;
    v_flex().size_full().bg(t.background)
        .child(
            h_flex().h(px(34.)).px_3().gap_2().items_center().border_b_1().border_color(t.border)
                .child(icon(IconName::FileText, 14., t.muted_foreground))
                .child(div().min_w_0().truncate().font_family(MONO).text_size(px(12.)).child(shown))
                .when(!saved, |d| d.child(div().size(px(7.)).rounded_full().bg(t.warning)))
                .child(div().flex_1())
                .child(Button::new("ide-save").small().ghost().icon(IconName::Save).label("保存").disabled(saved).on_click(cx.listener(|this, _, _, cx| this.save_ide(cx))))
                .child(Button::new("ide-reload").small().ghost().icon(IconName::RefreshCw).tooltip("从磁盘重新载入").on_click(cx.listener(|this, _, window, cx| this.reload_ide(window, cx))))
                .child(Button::new("ide-close").small().ghost().icon(IconName::X).on_click(cx.listener(|this, _, _, cx| this.close_ide(cx)))),
        )
        .child(div().flex_1().min_h_0().child(Editor::new(&doc.editor).bordered(false).appearance(false).size_full()))
        .into_any_element()
}
