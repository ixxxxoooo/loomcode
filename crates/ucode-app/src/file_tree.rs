//! 「文件」面板:项目文件树 + 只读代码预览(带语法高亮)。

use crate::right_panel::RightPanel;
use crate::ui::{MONO, empty_state, icon};
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Editor, EditorState};
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// 超过这个大小的文件不预览。
const MAX_PREVIEW_BYTES: u64 = 512 * 1024;

/// 目录里的一项。
#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

/// 文件树状态。
pub struct FileTree {
    root: PathBuf,
    expanded: HashSet<PathBuf>,
    children: HashMap<PathBuf, Vec<Node>>,
    selected: Option<PathBuf>,
    preview: Option<Entity<EditorState>>,
    /// 预览不可用时的说明(二进制、过大、读取失败)
    note: Option<String>,
}

/// 不在树里显示的目录与文件。
fn hidden(name: &str) -> bool {
    matches!(name, ".git" | ".DS_Store" | "target" | "node_modules" | ".build" | "DerivedData")
}

/// 读取一个目录:文件夹在前,按名称排序。
pub fn read_dir_sorted(dir: &Path) -> Vec<Node> {
    let mut nodes: Vec<Node> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    if hidden(&name) {
                        return None;
                    }
                    let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                    Some(Node { name, path: e.path(), is_dir })
                })
                .collect()
        })
        .unwrap_or_default();
    nodes.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    nodes
}

/// 按扩展名推断语法高亮语言。
pub fn language_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "rs" => "rust",
        "py" => "python",
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "ts" => "typescript",
        "tsx" => "tsx",
        "sh" | "zsh" | "bash" => "bash",
        "md" | "markdown" => "markdown",
        "toml" => "toml",
        "yml" | "yaml" => "yaml",
        "json" | "jsonc" => "json",
        "css" => "css",
        "html" | "htm" => "html",
        "go" => "go",
        "diff" | "patch" => "diff",
        "swift" => "swift",
        _ => "text",
    }
}

impl FileTree {
    pub fn new(root: String) -> Self {
        let mut t = Self {
            root: PathBuf::from(root),
            expanded: HashSet::new(),
            children: HashMap::new(),
            selected: None,
            preview: None,
            note: None,
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

    fn load_preview(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<RightPanel>) {
        self.selected = Some(path.clone());
        self.preview = None;
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
                        self.preview = Some(cx.new(|cx| EditorState::new(window, cx).language(lang).default_value(text)));
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
        let has_preview = self.selected.is_some();
        let root_name = self.root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let tree = v_flex().id("file-tree-scroll").w_full().overflow_y_scroll().py_1()
            .when(has_preview, |d| d.h(relative(0.38)).flex_none().border_b_1().border_color(t.border))
            .when(!has_preview, |d| d.flex_1().min_h_0())
            .children(rows.into_iter().enumerate().map(|(ix, (depth, node))| {
                let selected = self.selected.as_deref() == Some(node.path.as_path());
                let open = node.is_dir && self.expanded.contains(&node.path);
                let path = node.path.clone();
                let is_dir = node.is_dir;
                h_flex().id(SharedString::from(format!("ft-{ix}"))).w_full().h(px(24.)).pr_2().gap_1p5().items_center().cursor_pointer()
                    .pl(px(8. + depth as f32 * 14.))
                    .text_size(px(12.5))
                    .when(selected, |d| d.bg(t.list_active))
                    .hover(|d| d.bg(t.list_hover))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if is_dir {
                            this.files.toggle_dir(&path);
                            cx.notify();
                        } else {
                            this.files.load_preview(path.clone(), window, cx);
                        }
                    }))
                    .child(if is_dir {
                        icon(if open { IconName::FolderOpen } else { IconName::Folder }, 13., t.muted_foreground)
                    } else {
                        icon(IconName::FileText, 13., t.muted_foreground)
                    })
                    .child(div().flex_1().min_w_0().truncate().child(node.name))
            }));
        v_flex().size_full()
            .child(
                h_flex().h(px(30.)).px_3().gap_2().items_center().text_size(px(11.)).text_color(t.muted_foreground)
                    .child(icon(IconName::Folder, 12., t.muted_foreground))
                    .child(div().flex_1().min_w_0().truncate().child(root_name))
                    .child(
                        div().id("ft-refresh").p_1().rounded(px(4.)).cursor_pointer().hover(|d| d.bg(t.list_hover))
                            .on_click(cx.listener(|this, _, _, cx| { this.files.reload(); cx.notify(); }))
                            .child(icon(IconName::RefreshCw, 12., t.muted_foreground)),
                    ),
            )
            .child(tree)
            .when(has_preview, |d| {
                let name = self.selected.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                d.child(
                    v_flex().flex_1().min_h_0()
                        .child(h_flex().h(px(26.)).px_3().items_center().text_size(px(12.)).font_family(MONO).text_color(t.muted_foreground).child(name))
                        .child(match (&self.preview, &self.note) {
                            (Some(state), _) => div().flex_1().min_h_0().child(Editor::new(state).readonly(true).bordered(false).appearance(false).size_full()).into_any_element(),
                            (None, Some(note)) => div().flex_1().flex().items_center().justify_center().text_size(px(12.)).text_color(t.muted_foreground).child(note.clone()).into_any_element(),
                            _ => div().into_any_element(),
                        }),
                )
            })
            .into_any_element()
    }
}
