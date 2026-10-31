//! IDE 编辑器视图:单文件编辑器(不做多标签),独立窗口与融合模式共用。
//! 文件由宿主(IDE 窗口或融合模式的工作台)通过 `open` 传入;编辑后 ⌘S 保存。
//!
//! @author ixxxxoooo

use crate::file_tree::language_for;
use crate::ui::{MONO, empty_state, icon};
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

/// 编辑器视图:持有当前文档,不依赖工作台,可被任意窗口嵌入。
pub struct IdeView {
    /// 当前项目根目录,用于显示相对路径
    root: String,
    doc: Option<IdeDoc>,
    /// 打不开文件或保存失败时的说明
    note: Option<String>,
}

impl IdeView {
    /// 创建空的编辑器视图。
    pub fn new() -> Self {
        Self { root: String::new(), doc: None, note: None }
    }

    /// 设置项目根目录(会话切换时由宿主调用)。
    pub fn set_root(&mut self, root: String, cx: &mut Context<Self>) {
        if self.root != root {
            self.root = root;
            cx.notify();
        }
    }

    /// 读取文本文件并建立编辑器;二进制或过大的文件给出说明。
    pub fn open(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        match load(&path, window, cx) {
            Ok(doc) => {
                self.doc = Some(doc);
                self.note = None;
            }
            Err(note) => {
                self.doc = None;
                self.note = Some(note);
            }
        }
        cx.notify();
    }

    /// 保存到磁盘。
    pub fn save(&mut self, cx: &mut Context<Self>) {
        let Some(doc) = self.doc.as_mut() else { return };
        let text = doc.editor.read(cx).value().to_string();
        match std::fs::write(&doc.path, text) {
            Ok(()) => {
                doc.saved = true;
                self.note = None;
            }
            Err(e) => self.note = Some(format!("保存失败:{e}")),
        }
        cx.notify();
    }

    /// 从磁盘重新载入当前文件。
    pub fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self.doc.as_ref().map(|d| d.path.clone()) {
            self.open(path, window, cx);
        }
    }

    /// 关闭当前文件。
    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.doc = None;
        cx.notify();
    }
}

/// 读取文本文件并建立编辑器;二进制或过大的文件返回说明。
fn load(path: &Path, window: &mut Window, cx: &mut Context<IdeView>) -> Result<IdeDoc, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("无法读取 {}:{e}", path.display()))?;
    if meta.len() > MAX_EDIT_BYTES {
        return Err(format!("文件较大({} KB),不在编辑器中打开", meta.len() / 1024));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("无法读取 {}:{e}", path.display()))?;
    let text = String::from_utf8(bytes).map_err(|_| "二进制文件,不在编辑器中打开".to_string())?;
    let lang = language_for(path);
    let editor = cx.new(|cx| EditorState::new(window, cx).language(lang).default_value(text));
    // 编辑后标记为未保存
    let sub = cx.subscribe(&editor, |this: &mut IdeView, _, ev: &InputEvent, cx| {
        if matches!(ev, InputEvent::Change)
            && let Some(doc) = this.doc.as_mut()
            && doc.saved
        {
            doc.saved = false;
            cx.notify();
        }
    });
    Ok(IdeDoc { path: path.to_path_buf(), editor, saved: true, _sub: sub })
}

impl Render for IdeView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let Some(doc) = &self.doc else {
            let hint = self.note.clone().unwrap_or_else(|| "在左侧「文件」面板里选择一个文件".into());
            return empty_state(IconName::Code, "IDE", &hint, cx).into_any_element();
        };
        let shown = doc.path.strip_prefix(&self.root).unwrap_or(&doc.path).display().to_string();
        let saved = doc.saved;
        v_flex().size_full().bg(t.background)
            .child(
                h_flex().h(px(34.)).px_3().gap_2().items_center().border_b_1().border_color(t.border)
                    .child(icon(IconName::FileText, 14., t.muted_foreground))
                    .child(div().min_w_0().truncate().font_family(MONO).text_size(px(12.)).child(shown))
                    .when(!saved, |d| d.child(div().size(px(7.)).rounded_full().bg(t.warning)))
                    .child(div().flex_1())
                    .child(Button::new("ide-save").small().ghost().icon(IconName::Save).label("保存").disabled(saved).on_click(cx.listener(|this, _, _, cx| this.save(cx))))
                    .child(Button::new("ide-reload").small().ghost().icon(IconName::RefreshCw).tooltip("从磁盘重新载入").on_click(cx.listener(|this, _, window, cx| this.reload(window, cx))))
                    .child(Button::new("ide-close").small().ghost().icon(IconName::X).on_click(cx.listener(|this, _, _, cx| this.close(cx)))),
            )
            .child(div().flex_1().min_h_0().child(Editor::new(&doc.editor).bordered(false).appearance(false).size_full()))
            .into_any_element()
    }
}
