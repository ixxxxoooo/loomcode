//! IDE 编辑器视图:多标签编辑器(参考 Zed 的 Pane:预览标签、MRU 关闭、就近插入),
//! 独立窗口与融合模式共用。文件由宿主通过 `open` 传入;⌘S 保存,⌘W 关闭标签。
//!
//! @author ixxxxoooo

use crate::file_tree::language_for;
use crate::ide_chrome::{breadcrumbs, tab_bar, welcome};
use crate::ide_tabs::TabModel;
use crate::ui::empty_state;
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Editor, EditorState, InputEvent};
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::ButtonVariant;
use gpui_kit::component::{ActiveTheme, v_flex};
use gpui_kit::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

actions!(ide, [CloseTab, NextTab, PrevTab, QuickOpen, GoToLine, FindInProject]);

/// 超过这个大小的文件不在编辑器里打开。
const MAX_EDIT_BYTES: u64 = 1024 * 1024;

/// 一个已打开的文档。
pub struct IdeDoc {
    pub editor: Entity<EditorState>,
    /// 内容是否与磁盘一致
    pub saved: bool,
    /// 文件使用 CRLF 换行(仅用于状态显示)
    pub crlf: bool,
    /// Git 行标记装饰集合(首次刷新时创建)
    marks: Option<gpui_kit::component::input::RangeDecorationCollection>,
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
                crate::ide_gutter::refresh(&doc.editor, &mut doc.marks, &path, cx);
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

    /// 立即关闭一个标签并丢弃其文档。
    fn drop_tab(&mut self, path: &Path) {
        self.docs.remove(path);
        self.tabs.close(path);
    }

    /// 请求关闭一批标签:有未保存修改时先弹确认,否则直接关闭。
    ///
    /// 参数:`paths` 要关闭的标签。
    pub fn request_close(&mut self, paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        let dirty: Vec<String> = paths
            .iter()
            .filter(|p| self.docs.get(*p).is_some_and(|d| !d.saved))
            .map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())
            .collect();
        if dirty.is_empty() {
            self.close_now(&paths, window, cx);
            return;
        }
        let (view, list) = (cx.entity().downgrade(), dirty.join("、"));
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (view, paths) = (view.clone(), paths.clone());
            alert
                .confirm()
                .title("放弃未保存的修改?")
                .description(format!("「{list}」有未保存的修改,关闭后将丢失。可取消后先按 ⌘S 保存。"))
                .ok_text("放弃并关闭")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    let _ = view.update(cx, |this, cx| this.close_now(&paths, window, cx));
                    true
                })
        });
    }

    /// 关闭一批标签并让编辑器聚焦到新的当前标签。
    fn close_now(&mut self, paths: &[PathBuf], window: &mut Window, cx: &mut Context<Self>) {
        for p in paths {
            self.drop_tab(p);
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    /// 关闭一个标签(有未保存修改会先确认)。
    pub fn close_tab(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        self.request_close(vec![path.to_path_buf()], window, cx);
    }

    /// 关闭除 `keep` 之外的所有标签。
    pub fn close_others(&mut self, keep: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let list = self.tabs.others(keep);
        self.request_close(list, window, cx);
    }

    /// 关闭 `path` 右侧的所有标签。
    pub fn close_right(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let list = self.tabs.right_of(path);
        self.request_close(list, window, cx);
    }

    /// 关闭当前标签。
    pub fn close_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(p) = self.tabs.active().map(Path::to_path_buf) {
            self.close_tab(&p, window, cx);
        }
    }

    /// 把光标移到指定行列(1 起始),并让编辑器获得焦点。
    pub fn goto(&mut self, line: u32, col: u32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.tabs.active().and_then(|p| self.docs.get(p)) else { return };
        let pos = gpui_kit::component::input::Position::new(line.saturating_sub(1), col.saturating_sub(1));
        doc.editor.update(cx, |e, cx| e.set_cursor_position(pos, window, cx));
    }

    /// 当前是否有打开的文件。
    pub fn has_active(&self) -> bool {
        self.tabs.active().is_some()
    }

    /// 状态栏要显示的光标与文件信息:(行, 列, 语言, 是否 CRLF);没有打开文件时为 `None`。
    pub fn cursor_info(&self, cx: &App) -> Option<(u32, u32, &'static str, bool)> {
        let path = self.tabs.active()?;
        let doc = self.docs.get(path)?;
        let pos = doc.editor.read(cx).cursor_position();
        Some((pos.line + 1, pos.character + 1, language_for(path), doc.crlf))
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
    let mut marks = None;
    crate::ide_gutter::refresh(&editor, &mut marks, path, cx);
    Ok(IdeDoc { editor, saved: true, crlf, marks, _sub: sub })
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
        let items: Vec<_> = self
            .tabs
            .tabs()
            .iter()
            .map(|p| (p.clone(), self.docs.get(p).is_some_and(|d| !d.saved), self.tabs.is_preview(p), *p == path))
            .collect();
        root.child(tab_bar(items, cx))
            .child(breadcrumbs(&path, &self.root, doc.saved, self.note.clone(), cx))
            .child(div().flex_1().min_h_0().child(Editor::new(&editor).bordered(false).appearance(false).size_full()))
            .into_any_element()
    }
}
