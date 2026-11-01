//! 「文件」面板:项目文件树 + 只读代码预览(带语法高亮)。

use crate::file_icons::file_icon_img;
use crate::file_ops::row_menu;
use crate::git_status::{GitMark, GitStatus};
use crate::right_panel::{PanelEvent, RightPanel};
use crate::ui::{MONO, empty_state, icon};
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Editor, EditorState};
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// 超过这个大小的文件不预览。
const MAX_PREVIEW_BYTES: u64 = 512 * 1024;

/// 文件树状态。
pub struct FileTree {
    root: PathBuf,
    expanded: HashSet<PathBuf>,
    children: HashMap<PathBuf, Vec<Node>>,
    selected: Option<PathBuf>,
    preview: Option<Entity<EditorState>>,
    /// Markdown 文件用与 IDE 共用的预览视图(点块就地编辑,写回磁盘)
    md_view: Option<Entity<crate::ide_markdown::MdDocView>>,
    /// 预览不可用时的说明(二进制、过大、读取失败)
    note: Option<String>,
    /// 文件与目录的 Git 改动标记
    git: GitStatus,
}

pub use crate::file_lang::{Node, language_for, read_dir_sorted};

impl FileTree {
    /// 展开着的目录(绝对路径,排序后,保存记忆用)。
    pub fn expanded_dirs(&self) -> Vec<String> {
        let mut v: Vec<String> = self.expanded.iter().map(|p| p.to_string_lossy().to_string()).collect();
        v.sort();
        v
    }

    /// 按记忆展开目录(只展开仍在项目内且存在的目录)。
    pub fn expand_dirs(&mut self, dirs: &[String]) {
        for d in dirs {
            let p = PathBuf::from(d);
            if p.is_dir() && p.starts_with(&self.root) && self.expanded.insert(p.clone()) {
                self.children.insert(p.clone(), read_dir_sorted(&p));
            }
        }
    }

    /// 项目根目录。
    pub fn root_path(&self) -> &Path {
        &self.root
    }

    /// 写入后台读到的 Git 状态。
    pub fn set_git(&mut self, git: GitStatus) {
        self.git = git;
    }

    /// 新建条目时的默认父目录:选中的目录,或选中文件所在目录,否则项目根。
    pub fn target_dir(&self) -> PathBuf {
        match &self.selected {
            Some(p) if p.is_dir() => p.clone(),
            Some(p) => p.parent().map(Path::to_path_buf).unwrap_or_else(|| self.root.clone()),
            None => self.root.clone(),
        }
    }

    /// 全部折叠。
    pub fn collapse_all(&mut self) {
        self.expanded.clear();
        self.reload();
    }

    /// 展开到某个路径的所有上级目录(不选中)。
    pub fn expand_to(&mut self, path: &Path) {
        let mut cur = path.parent();
        while let Some(dir) = cur {
            if dir == self.root || !dir.starts_with(&self.root) {
                break;
            }
            self.expanded.insert(dir.to_path_buf());
            cur = dir.parent();
        }
    }

    pub fn new(root: String) -> Self {
        let mut t = Self {
            root: PathBuf::from(root),
            expanded: HashSet::new(),
            children: HashMap::new(),
            selected: None,
            preview: None,
            md_view: None,
            note: None,
            git: GitStatus::default(),
        };
        t.reload();
        t
    }

    /// 切换项目根目录。
    pub fn set_root(&mut self, root: String) {
        let root = PathBuf::from(root);
        if root == self.root {
            return;
        }
        self.root = root;
        self.expanded.clear();
        self.selected = None;
        self.preview = None;
        self.md_view = None;
        self.note = None;
        self.reload();
    }

    /// 重新读取根目录与所有已展开目录。
    pub fn reload(&mut self) {
        self.children.clear();
        self.children.insert(self.root.clone(), read_dir_sorted(&self.root));
        for dir in self.expanded.clone() {
            self.children.insert(dir.clone(), read_dir_sorted(&dir));
        }
    }

    fn toggle_dir(&mut self, dir: &Path) {
        if !self.expanded.remove(dir) {
            self.expanded.insert(dir.to_path_buf());
            self.children.insert(dir.to_path_buf(), read_dir_sorted(dir));
        }
    }

    /// 打开文件到预览;`path` 可以是相对项目根的路径。同时展开其所有上级目录。
    pub fn open(&mut self, path: &str, window: &mut Window, cx: &mut Context<RightPanel>) {
        let p = PathBuf::from(path);
        let full = if p.is_absolute() { p } else { self.root.join(p) };
        // 展开上级目录,让选中的文件在树里可见
        let mut cur = full.parent();
        while let Some(dir) = cur {
            if dir == self.root || !dir.starts_with(&self.root) {
                break;
            }
            if self.expanded.insert(dir.to_path_buf()) {
                self.children.insert(dir.to_path_buf(), read_dir_sorted(dir));
            }
            cur = dir.parent();
        }
        self.load_preview(full, window, cx);
    }

    /// 在树里选中并展开到某个文件,但不加载预览(IDE 模式下文件在中栏编辑器里打开)。
    pub fn reveal(&mut self, path: &Path) {
        let mut cur = path.parent();
        while let Some(dir) = cur {
            if dir == self.root || !dir.starts_with(&self.root) {
                break;
            }
            if self.expanded.insert(dir.to_path_buf()) {
                self.children.insert(dir.to_path_buf(), read_dir_sorted(dir));
            }
            cur = dir.parent();
        }
        self.selected = Some(path.to_path_buf());
        self.preview = None;
        self.md_view = None;
        self.note = None;
    }

    pub fn load_preview(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<RightPanel>) {
        self.selected = Some(path.clone());
        self.preview = None;
        self.md_view = None;
        self.md_view = None;
        self.note = None;
        match std::fs::metadata(&path) {
            Err(e) => self.note = Some(format!("无法读取:{e}")),
            Ok(m) if m.len() > MAX_PREVIEW_BYTES => self.note = Some(format!("文件较大({} KB),不预览", m.len() / 1024)),
            Ok(_) => match std::fs::read(&path) {
                Err(e) => self.note = Some(format!("无法读取:{e}")),
                Ok(bytes) => match String::from_utf8(bytes) {
                    Err(_) => self.note = Some("二进制文件,不预览".into()),
                    Ok(text) => {
                        let lang = language_for(&path);
                        let editor = cx.new(|cx| EditorState::new(window, cx).language(lang).default_value(text));
                        if crate::ide_markdown::is_markdown(&path) {
                            self.md_view = Some(cx.new(|_| crate::ide_markdown::MdDocView::new(editor.clone(), Some(path.clone()))));
                        }
                        self.preview = Some(editor);
                    }
                },
            },
        }
        cx.notify();
    }

    /// 递归把可见节点摊平成 (深度, 节点) 列表。
    fn flatten(&self) -> Vec<(usize, Node)> {
        fn go(t: &FileTree, dir: &Path, depth: usize, out: &mut Vec<(usize, Node)>) {
            if let Some(nodes) = t.children.get(dir) {
                for n in nodes {
                    out.push((depth, n.clone()));
                    if n.is_dir && t.expanded.contains(&n.path) {
                        go(t, &n.path, depth + 1, out);
                    }
                }
            }
        }
        let mut out = Vec::new();
        go(self, &self.root, 0, &mut out);
        out
    }

    pub fn render(&self, cx: &mut Context<RightPanel>) -> AnyElement {
        let t = cx.theme().clone();
        let rows = self.flatten();
        if rows.is_empty() {
            return empty_state(IconName::Folder, "目录为空", &self.root.display().to_string(), cx).into_any_element();
        }
        let has_preview = self.preview.is_some() || self.note.is_some();
        let md_view = self.md_view.clone();
        let root_name = self.root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let tree = v_flex().id("file-tree-scroll").w_full().overflow_y_scroll().py_1()
            .when(has_preview, |d| d.h(relative(0.38)).flex_none().border_b_1().border_color(t.border))
            .when(!has_preview, |d| d.flex_1().min_h_0())
            .children(rows.into_iter().enumerate().map(|(ix, (depth, node))| {
                let selected = self.selected.as_deref() == Some(node.path.as_path());
                let open = node.is_dir && self.expanded.contains(&node.path);
                let path = node.path.clone();
                let is_dir = node.is_dir;
                let mark = self.git.get(&node.path);
                let color = match mark {
                    Some(GitMark::Modified) => t.warning,
                    Some(GitMark::Added) => t.success,
                    Some(GitMark::Deleted) => t.danger,
                    None => t.foreground,
                };
                h_flex().id(SharedString::from(format!("ft-{ix}"))).w_full().h(px(24.)).pr_2().gap_1p5().items_center().cursor_pointer()
                    .pl(px(6. + depth as f32 * 14.))
                    .text_size(px(12.5))
                    .when(selected, |d| d.bg(t.list_active))
                    .hover(|d| d.bg(t.list_hover))
                    .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| {
                        if is_dir {
                            // 目录只在单击时展开 / 收起,双击的第二下不再翻回去
                            if ev.click_count() < 2 {
                                this.files.toggle_dir(&path);
                                cx.notify();
                            }
                        } else if ev.click_count() >= 2 {
                            // 双击:固定为正式标签(IDE);右侧面板里等同单击
                            cx.emit(PanelEvent::OpenFilePinned(path.clone()));
                        } else {
                            // 单击:由工作台决定是在右侧预览还是在 IDE 编辑器里开预览标签
                            cx.emit(PanelEvent::OpenFile(path.clone()));
                        }
                    }))
                    .relative()
                    .children((0..depth).map(|i| div().absolute().top_0().bottom_0().left(px(14. + i as f32 * 14.)).w(px(1.)).bg(t.border.opacity(0.7))))
                    .child(div().w(px(12.)).flex_none().when(is_dir, |d| d.child(icon(if open { IconName::ChevronDown } else { IconName::ChevronRight }, 12., t.muted_foreground))))
                    .child(file_icon_img(&node.name, is_dir, open, 16.))
                    .context_menu(row_menu(cx.entity().downgrade(), node.path.clone(), is_dir))
                    .child(div().flex_1().min_w_0().truncate().text_color(color).child(node.name))
                    .when_some(mark.filter(|_| !is_dir), |d, m| {
                        d.child(div().text_size(px(11.)).font_family(MONO).text_color(color).child(match m {
                            GitMark::Modified => "M",
                            GitMark::Added => "U",
                            GitMark::Deleted => "D",
                        }))
                    })
                    .when(mark.is_some() && is_dir, |d| d.child(div().size(px(6.)).rounded_full().bg(color)))
            }));
        v_flex().size_full()
            .child(
                h_flex().h(px(30.)).px_3().gap_1().items_center().text_size(px(11.)).text_color(t.muted_foreground)
                    .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).child(root_name.to_uppercase()))
                    .children([
                        ("ft-new-file", IconName::FilePlus, 0u8),
                        ("ft-new-dir", IconName::FolderPlus, 1),
                        ("ft-collapse", IconName::ChevronsDownUp, 2),
                        ("ft-refresh", IconName::RefreshCw, 3),
                    ].map(|(id, ic, kind)| {
                        div().id(id).p_1().rounded(px(4.)).cursor_pointer().hover(|d| d.bg(t.list_hover))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let dir = this.files.target_dir();
                                match kind {
                                    0 => this.new_entry_dialog(dir, false, window, cx),
                                    1 => this.new_entry_dialog(dir, true, window, cx),
                                    2 => this.files.collapse_all(),
                                    _ => this.reload_files(cx),
                                }
                                cx.notify();
                            }))
                            .child(icon(ic, 13., t.muted_foreground))
                    })),
            )
            .child(tree)
            .when(has_preview, |d| {
                let name = self.selected.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                d.child(
                    v_flex().flex_1().min_h_0()
                        .child(h_flex().h(px(26.)).px_3().items_center().text_size(px(12.)).font_family(MONO).text_color(t.muted_foreground).child(name))
                        .child(match (&self.preview, &self.note) {
                            (Some(_), _) if md_view.is_some() => div().flex_1().min_h_0().child(md_view.clone().unwrap()).into_any_element(),
                            (Some(state), _) => div().flex_1().min_h_0().child(Editor::new(state).readonly(true).bordered(false).appearance(false).size_full()).into_any_element(),
                            (None, Some(note)) => div().flex_1().flex().items_center().justify_center().text_size(px(12.)).text_color(t.muted_foreground).child(note.clone()).into_any_element(),
                            _ => div().into_any_element(),
                        }),
                )
            })
            .into_any_element()
    }
}
