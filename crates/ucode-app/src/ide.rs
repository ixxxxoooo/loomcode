//! IDE 编辑器视图:多标签编辑器(参考 Zed 的 Pane:预览标签、MRU 关闭、就近插入),
//! 独立窗口与融合模式共用。文件由宿主通过 `open` 传入;⌘S 保存,⌘W 关闭标签。
//!
//! @author ixxxxoooo

use crate::file_tree::language_for;
use crate::ide_chrome::{breadcrumbs, info_strip, tab_bar, welcome};
use crate::ide_tabs::TabModel;
use crate::ui::empty_state;
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Editor, EditorState, InputEvent};
use gpui_kit::component::{ActiveTheme, v_flex};
use gpui_kit::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

actions!(ide, [CloseTab, NextTab, PrevTab]);

/// 超过这个大小的文件不在编辑器里打开。
const MAX_EDIT_BYTES: u64 = 1024 * 1024;

/// 一个已打开的文档。
pub struct IdeDoc {
    pub editor: Entity<EditorState>,
    /// 内容是否与磁盘一致
    pub saved: bool,
    /// 文件使用 CRLF 换行(仅用于状态显示)
    pub crlf: bool,
    _sub: Subscription,
}

/// 编辑器视图:持有所有标签的文档,不依赖工作台,可被任意窗口嵌入。
pub struct IdeView {
    /// 当前项目根目录,用于显示相对路径
    root: String,
    docs: HashMap<PathBuf, IdeDoc>,
    tabs: TabModel,
    /// 打不开文件或保存失败时的说明
    note: Option<String>,
}

impl IdeView {
    /// 创建空的编辑器视图。
    pub fn new() -> Self {
        Self { root: String::new(), docs: HashMap::new(), tabs: TabModel::default(), note: None }
    }

    /// 设置项目根目录(会话切换时由宿主调用)。
    pub fn set_root(&mut self, root: String, cx: &mut Context<Self>) {
        if self.root != root {
            self.root = root;
            cx.notify();
        }
    }

    /// 打开文件:`preview` 为真时作为预览标签(文件树单击),否则固定。
    /// 已打开的文件只激活;读取失败时在编辑区给出说明。
    pub fn open(&mut self, path: PathBuf, preview: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.note = None;
        if !self.docs.contains_key(&path) {
            match load(&path, window, cx) {
                Ok(doc) => {
                    self.docs.insert(path.clone(), doc);
                }
                Err(note) => {
                    self.note = Some(note);
                    cx.notify();
                    return;
                }
            }
        }
        let out = self.tabs.open(&path, preview);
        if let Some(old) = out.replaced {
            self.docs.remove(&old);
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    /// 激活已有标签(点击标签栏)。
    pub fn activate(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.activate(path);
        self.focus_active(window, cx);
        cx.notify();
    }

    /// 双击标签:固定预览。
    pub fn pin(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.tabs.pin(path);
        cx.notify();
    }

    /// 让当前标签的编辑器获得焦点。
    fn focus_active(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(doc) = self.tabs.active().and_then(|p| self.docs.get(p)) {
            doc.editor.update(cx, |e, cx| e.focus(window, cx));
        }
    }

    /// 保存当前文件到磁盘。
    pub fn save(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.tabs.active().map(Path::to_path_buf) else { return };
        let Some(doc) = self.docs.get_mut(&path) else { return };
        let text = doc.editor.read(cx).value().to_string();
        match std::fs::write(&path, text) {
            Ok(()) => {
                doc.saved = true;
                self.note = None;
                self.tabs.pin(&path);
            }
            Err(e) => self.note = Some(format!("保存失败:{e}")),
        }
        cx.notify();
    }

    /// 从磁盘重新载入当前文件(丢弃未保存的修改)。
    pub fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.tabs.active().map(Path::to_path_buf) else { return };
        self.docs.remove(&path);
        match load(&path, window, cx) {
            Ok(doc) => {
                self.docs.insert(path.clone(), doc);
                self.focus_active(window, cx);
            }
            Err(note) => self.note = Some(note),
        }
        cx.notify();
    }

    /// 关闭一个标签(有未保存修改时同样直接关闭,与 Zed 的「丢弃」一致由用户先保存)。
    pub fn close_tab(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        self.docs.remove(path);
        self.tabs.close(path);
        self.focus_active(window, cx);
        cx.notify();
    }

    /// 关闭当前标签。
    pub fn close_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(p) = self.tabs.active().map(Path::to_path_buf) {
            self.close_tab(&p, window, cx);
        }
    }

    /// 切换到相邻标签。
    fn cycle(&mut self, step: isize, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.cycle(step);
        self.focus_active(window, cx);
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
    let crlf = text.contains("\r\n");
    let editor = cx.new(|cx| EditorState::new(window, cx).language(language_for(path)).searchable(true).default_value(text));
    // 编辑后标记未保存并固定预览标签;光标移动时刷新状态条
    let key = path.to_path_buf();
    let sub = cx.subscribe(&editor, move |this: &mut IdeView, _, ev: &InputEvent, cx| {
        if matches!(ev, InputEvent::Change) {
            this.tabs.pin(&key);
            if let Some(doc) = this.docs.get_mut(&key) {
                doc.saved = false;
            }
        }
        cx.notify();
    });
    Ok(IdeDoc { editor, saved: true, crlf, _sub: sub })
}

impl Render for IdeView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let root = v_flex()
            .key_context("IdeView")
            .size_full()
            .bg(t.background)
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| this.close_active(window, cx)))
            .on_action(cx.listener(|this, _: &NextTab, window, cx| this.cycle(1, window, cx)))
            .on_action(cx.listener(|this, _: &PrevTab, window, cx| this.cycle(-1, window, cx)));
        let active = self.tabs.active().map(Path::to_path_buf);
        let Some(path) = active.filter(|p| self.docs.contains_key(p)) else {
            if let Some(note) = self.note.clone() {
                return root.child(empty_state(IconName::Code, "无法打开", &note, cx)).into_any_element();
            }
            return root.child(welcome(cx)).into_any_element();
        };
        let doc = &self.docs[&path];
        let editor = doc.editor.clone();
        let pos = editor.read(cx).cursor_position();
        let lang = language_for(&path);
        let items: Vec<_> = self
            .tabs
            .tabs()
            .iter()
            .map(|p| (p.clone(), self.docs.get(p).is_some_and(|d| !d.saved), self.tabs.is_preview(p), *p == path))
            .collect();
        root.child(tab_bar(items, cx))
            .child(breadcrumbs(&path, &self.root, doc.saved, self.note.clone(), cx))
            .child(div().flex_1().min_h_0().child(Editor::new(&editor).bordered(false).appearance(false).size_full()))
            .child(info_strip(pos.line + 1, pos.character + 1, lang, doc.crlf, cx))
            .into_any_element()
    }
}
