//! IDE 左栏的「项目内搜索」(⌘⇧F):在后台线程里逐文件做不区分大小写的子串匹配,
//! 结果按文件分组,点击一行跳到该文件的对应行。
//!
//! @author ixxxxoooo

use crate::file_icons::file_icon_img;
use crate::ui::{MONO, empty_state};
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::*;
use std::path::{Path, PathBuf};

/// 最多收集的命中行数。
const MAX_HITS: usize = 500;
/// 超过这个大小的文件不搜索。
const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// 一个文件里的命中。
#[derive(Debug, Clone, PartialEq)]
pub struct FileHits {
    pub path: PathBuf,
    /// (行号 1 起始, 该行文本)
    pub lines: Vec<(u32, String)>,
}

/// 搜索面板向窗口发出的事件。
pub enum SearchEvent {
    /// 打开某文件的某行
    Open(PathBuf, u32),
}

impl EventEmitter<SearchEvent> for SearchPanel {}

/// 在 `root` 下搜索 `query`(不区分大小写),最多 `max` 个命中行;返回 (结果, 是否被截断)。
pub fn search_dir(root: &Path, query: &str, max: usize) -> (Vec<FileHits>, bool) {
    let q = query.to_lowercase();
    let mut out = Vec::new();
    let (mut count, mut truncated) = (0usize, false);
    let mut stack = vec![root.to_path_buf()];
    'walk: while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| std::cmp::Reverse(e.file_name()));
        for e in entries {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || matches!(name.as_str(), "target" | "node_modules" | "DerivedData" | "dist") {
                continue;
            }
            let path = e.path();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                stack.push(path);
                continue;
            }
            if e.metadata().map(|m| m.len() > MAX_FILE_BYTES).unwrap_or(true) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let mut lines = Vec::new();
            for (i, l) in text.lines().enumerate() {
                if l.to_lowercase().contains(&q) {
                    lines.push((i as u32 + 1, l.trim().chars().take(160).collect::<String>()));
                    count += 1;
                    if count >= max {
                        truncated = true;
                        out.push(FileHits { path, lines });
                        break 'walk;
                    }
                }
            }
            if !lines.is_empty() {
                out.push(FileHits { path, lines });
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    (out, truncated)
}

/// 项目内搜索面板。
pub struct SearchPanel {
    root: String,
    pub input: Entity<InputState>,
    results: Vec<FileHits>,
    truncated: bool,
    running: bool,
    /// 已执行过搜索(用来区分「还没搜」与「没结果」)
    searched: bool,
    _sub: Subscription,
}

impl SearchPanel {
    /// 创建面板。
    pub fn new(root: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("在项目中搜索(回车)"));
        let sub = cx.subscribe_in(&input, window, |this: &mut Self, _, ev: &InputEvent, _, cx| {
            if let InputEvent::PressEnter { .. } = ev {
                this.run(cx);
            }
        });
        Self { root, input, results: Vec::new(), truncated: false, running: false, searched: false, _sub: sub }
    }

    /// 切换项目根目录并清空结果。
    pub fn set_root(&mut self, root: String, cx: &mut Context<Self>) {
        if self.root != root {
            self.root = root;
            self.results.clear();
            self.searched = false;
            cx.notify();
        }
    }

    /// 执行搜索(后台线程)。
    fn run(&mut self, cx: &mut Context<Self>) {
        let query = self.input.read(cx).value().to_string();
        if query.trim().is_empty() || self.root.is_empty() {
            return;
        }
        self.running = true;
        cx.notify();
        let root = PathBuf::from(&self.root);
        cx.spawn(async move |this, cx| {
            let (results, truncated) = cx.background_spawn(async move { search_dir(&root, &query, MAX_HITS) }).await;
            let _ = this.update(cx, |this, cx| {
                this.results = results;
                this.truncated = truncated;
                this.running = false;
                this.searched = true;
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for SearchPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let total: usize = self.results.iter().map(|f| f.lines.len()).sum();
        let root = PathBuf::from(&self.root);
        let body = if self.results.is_empty() {
            let (title, hint) = if self.running { ("搜索中…", "") } else if self.searched { ("没有结果", "换个关键词试试") } else { ("项目内搜索", "输入关键词后回车") };
            empty_state(IconName::Search, title, hint, cx).into_any_element()
        } else {
            v_flex().id("search-results").flex_1().min_h_0().overflow_y_scroll().pb_2()
                .child(div().px_3().py_1().text_size(px(11.)).text_color(t.muted_foreground).child(format!("{} 个文件,{total} 处匹配{}", self.results.len(), if self.truncated { "(已截断)" } else { "" })))
                .children(self.results.iter().enumerate().flat_map(|(fi, f)| {
                    let rel = f.path.strip_prefix(&root).unwrap_or(&f.path).display().to_string();
                    let name = f.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    let mut rows: Vec<AnyElement> = vec![h_flex().h(px(24.)).px_3().gap_1p5().items_center().text_size(px(12.5))
                        .child(file_icon_img(&name, false, false, 14.))
                        .child(div().flex_none().child(name))
                        .child(div().flex_1().min_w_0().truncate().text_size(px(11.)).text_color(t.muted_foreground).child(rel))
                        .into_any_element()];
                    for (li, (line, text)) in f.lines.iter().enumerate() {
                        let (path, line) = (f.path.clone(), *line);
                        rows.push(
                            h_flex().id(SharedString::from(format!("hit-{fi}-{li}"))).h(px(22.)).pl(px(30.)).pr_2().gap_2().items_center().cursor_pointer()
                                .hover(|d| d.bg(t.list_hover)).text_size(px(12.))
                                .on_click(cx.listener(move |_, _, _, cx| cx.emit(SearchEvent::Open(path.clone(), line))))
                                .child(div().w(px(32.)).flex_none().font_family(MONO).text_size(px(11.)).text_color(t.muted_foreground).child(line.to_string()))
                                .child(div().flex_1().min_w_0().truncate().child(text.clone()))
                                .into_any_element(),
                        );
                    }
                    rows
                }))
                .into_any_element()
        };
        v_flex().size_full().bg(t.sidebar)
            .child(div().px_3().pt_2().pb_1().child(Input::new(&self.input)))
            .child(div().flex_1().min_h_0().child(body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 搜索忽略大小写并给出行号() {
        let dir = std::env::temp_dir().join(format!("ucode-search-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::create_dir_all(dir.join("target")).unwrap();
        std::fs::write(dir.join("a.txt"), "hello\nWorld Foo\n").unwrap();
        std::fs::write(dir.join("sub/b.txt"), "foo bar\n").unwrap();
        std::fs::write(dir.join("target/c.txt"), "foo\n").unwrap();
        let (res, truncated) = search_dir(&dir, "FOO", 10);
        assert!(!truncated);
        assert_eq!(res.len(), 2);
        assert_eq!(res[0].lines, vec![(2, "World Foo".to_string())]);
        let (_, truncated) = search_dir(&dir, "foo", 1);
        assert!(truncated);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
