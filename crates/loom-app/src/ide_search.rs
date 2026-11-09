//! IDE 左栏「搜索」面板的状态与动作(对标 VS Code):边输入边搜(防抖 + 取消过期搜索)、
//! 区分大小写 / 全词 / 正则、包含与排除文件、替换(单处 / 单文件 / 全部)、忽略匹配、折叠分组。
//! 搜索与替换内核在 `loom-search`,渲染在 `ide_search_view.rs` / `ide_search_rows.rs`。
//!
//! @author ixxxxoooo

use crate::ide_search_model::{self as model, Row};
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use loom_search::{FileMatches, MatchKey, SearchOutcome, SearchQuery};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// 输入后等多久才开始搜(毫秒),与 VS Code 一致量级。
const DEBOUNCE_MS: u64 = 300;

actions!(ide_search, [ToggleCase, ToggleWholeWord, ToggleRegex, TogglePreserveCase, ToggleReplace, ReplaceAllMatches]);

/// 面板向窗口发出的事件。
pub enum SearchEvent {
    /// 在编辑器里打开并选中一处匹配(行号 1 起始,列为字符数 0 起始)
    Open { path: PathBuf, line: u32, start: u32, end: u32 },
}

impl EventEmitter<SearchEvent> for SearchPanel {}

/// 替换的范围。
#[derive(Clone, Copy)]
pub(crate) enum Scope {
    /// 一处匹配(文件下标, 匹配下标)
    One(usize, usize),
    /// 一个文件里当前列出的全部匹配
    File(usize),
    /// 全部结果
    All,
}

/// 搜索面板。字段对同模块的视图文件开放。
pub struct SearchPanel {
    pub(crate) root: String,
    /// 多根工作区里主目录之外的其余文件夹(一起搜索)
    pub(crate) extra_roots: Vec<String>,
    pub input: Entity<InputState>,
    pub(crate) replace_input: Entity<InputState>,
    pub(crate) include_input: Entity<InputState>,
    pub(crate) exclude_input: Entity<InputState>,
    pub(crate) case: bool,
    pub(crate) word: bool,
    pub(crate) regex: bool,
    pub(crate) preserve_case: bool,
    pub(crate) use_ignore: bool,
    pub(crate) replace_open: bool,
    pub(crate) details_open: bool,
    pub(crate) files: Vec<FileMatches>,
    pub(crate) rows: Vec<Row>,
    pub(crate) collapsed: HashSet<PathBuf>,
    pub(crate) truncated: bool,
    pub(crate) error: Option<String>,
    pub(crate) running: bool,
    /// 已执行过搜索(区分「还没搜」与「没结果」)
    pub(crate) searched: bool,
    /// 被点中的匹配:(文件, 行, 起列)
    pub(crate) active: Option<(PathBuf, u32, u32)>,
    pub(crate) scroll: UniformListScrollHandle,
    generation: u64,
    cancel: Arc<AtomicBool>,
    _subs: Vec<Subscription>,
}

impl SearchPanel {
    /// 创建面板。参数:`root` 项目根(可为空,之后用 `set_root` 设置)。
    pub fn new(root: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索"));
        let replace_input = cx.new(|cx| InputState::new(window, cx).placeholder("替换"));
        let include_input = cx.new(|cx| InputState::new(window, cx).placeholder("例如 *.rs, src/"));
        let exclude_input = cx.new(|cx| InputState::new(window, cx).placeholder("例如 *.min.js, dist"));
        let mut subs = Vec::new();
        for (ix, state) in [&input, &replace_input, &include_input, &exclude_input].into_iter().enumerate() {
            subs.push(cx.subscribe_in(state, window, move |this: &mut Self, _, ev: &InputEvent, _, cx| match ev {
                // 替换文本变化只刷新结果行里的替换预览
                InputEvent::Change if ix == 1 => cx.notify(),
                InputEvent::Change => this.schedule(DEBOUNCE_MS, cx),
                InputEvent::PressEnter { .. } if ix != 1 => this.schedule(0, cx),
                _ => {}
            }));
        }
        Self {
            root, extra_roots: Vec::new(), input, replace_input, include_input, exclude_input, case: false, word: false, regex: false, preserve_case: false, use_ignore: true,
            replace_open: false, details_open: false, files: Vec::new(), rows: Vec::new(), collapsed: HashSet::new(), truncated: false, error: None,
            running: false, searched: false, active: None, scroll: UniformListScrollHandle::new(), generation: 0, cancel: Arc::new(AtomicBool::new(false)), _subs: subs,
        }
    }

    /// 切换到一组根目录(多根工作区:第一个是主目录,其余一起搜索);没变就什么都不做。
    pub fn set_roots(&mut self, roots: Vec<String>, cx: &mut Context<Self>) {
        let Some((first, rest)) = roots.split_first() else { return };
        if self.root != *first || self.extra_roots != rest {
            self.root = first.clone();
            self.extra_roots = rest.to_vec();
            self.schedule(0, cx);
        }
    }

    /// 文件所在的根目录(多根时取包含它的那个,找不到退回主目录)。
    pub(crate) fn root_of(&self, path: &std::path::Path) -> &str {
        std::iter::once(&self.root).chain(&self.extra_roots).filter(|r| path.starts_with(r.as_str())).max_by_key(|r| r.len()).unwrap_or(&self.root)
    }

    /// 用选中文字作为搜索词(⌘⇧F 时编辑器有选区)并立即搜索。
    pub fn seed(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |i, cx| i.set_value(text, window, cx));
        self.schedule(0, cx);
    }

    /// 当前输入得到的搜索条件;「使用忽略文件」开启时并入设置里的忽略目录。
    fn query(&self, cx: &App) -> SearchQuery {
        let mut exclude = self.exclude_input.read(cx).value().to_string();
        if self.use_ignore {
            let extra: Vec<String> = crate::prefs(cx).intel.ignore.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect();
            exclude = std::iter::once(exclude).chain(extra).filter(|s| !s.trim().is_empty()).collect::<Vec<_>>().join(",");
        }
        SearchQuery {
            pattern: self.input.read(cx).value().to_string(),
            case_sensitive: self.case,
            whole_word: self.word,
            regex: self.regex,
            include: self.include_input.read(cx).value().to_string(),
            exclude,
            use_ignore_files: self.use_ignore,
        }
    }

    /// 安排一次搜索:取消还在跑的旧搜索,`delay_ms` 后在后台线程执行,只采纳最新一次的结果。关键字为空则清空结果。
    pub(crate) fn schedule(&mut self, delay_ms: u64, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        let query = self.query(cx);
        if query.pattern.is_empty() || self.root.is_empty() {
            (self.files, self.rows, self.error, self.running, self.searched, self.truncated, self.active) = (Vec::new(), Vec::new(), None, false, false, false, None);
            cx.notify();
            return;
        }
        let roots: Vec<PathBuf> = std::iter::once(&self.root).chain(&self.extra_roots).map(PathBuf::from).collect();
        let cancel = self.cancel.clone();
        self.running = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            if delay_ms > 0 {
                cx.background_executor().timer(Duration::from_millis(delay_ms)).await;
                if this.read_with(cx, |t, _| t.generation != generation).unwrap_or(true) {
                    return;
                }
            }
            let result = cx.background_spawn(async move { loom_search::search_many(&roots, &query, &cancel) }).await;
            let _ = this.update(cx, |t, cx| if t.generation == generation { t.apply(result, cx) });
        })
        .detach();
    }

    /// 采纳一次搜索的结果(已折叠的文件保持折叠)。
    fn apply(&mut self, result: Result<SearchOutcome, String>, cx: &mut Context<Self>) {
        match result {
            Ok(o) if o.cancelled => return,
            Ok(o) => (self.files, self.truncated, self.error) = (o.files, o.truncated, None),
            Err(e) => (self.files, self.truncated, self.error) = (Vec::new(), false, Some(e)),
        }
        (self.running, self.searched) = (false, true);
        self.rebuild();
        cx.notify();
    }

    /// 结果或折叠状态变了,重新拍平成行。
    pub(crate) fn rebuild(&mut self) {
        self.rows = model::flatten(&self.files, &self.collapsed);
    }

    /// 切换一个开关并立即重搜。
    fn flip(&mut self, f: impl FnOnce(&mut Self), cx: &mut Context<Self>) {
        f(self);
        self.schedule(0, cx);
    }
    pub(crate) fn toggle_case(&mut self, cx: &mut Context<Self>) { self.flip(|s| s.case = !s.case, cx) }
    pub(crate) fn toggle_word(&mut self, cx: &mut Context<Self>) { self.flip(|s| s.word = !s.word, cx) }
    pub(crate) fn toggle_regex(&mut self, cx: &mut Context<Self>) { self.flip(|s| s.regex = !s.regex, cx) }
    pub(crate) fn toggle_use_ignore(&mut self, cx: &mut Context<Self>) { self.flip(|s| s.use_ignore = !s.use_ignore, cx) }
    pub(crate) fn toggle_preserve_case(&mut self, cx: &mut Context<Self>) { self.preserve_case = !self.preserve_case; cx.notify() }
    pub(crate) fn toggle_replace(&mut self, cx: &mut Context<Self>) { self.replace_open = !self.replace_open; cx.notify() }
    pub(crate) fn toggle_details(&mut self, cx: &mut Context<Self>) { self.details_open = !self.details_open; cx.notify() }

    /// 清除搜索结果:清空全部输入框与结果。
    pub(crate) fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for s in [&self.input, &self.replace_input, &self.include_input, &self.exclude_input] {
            s.update(cx, |i, cx| i.set_value("", window, cx));
        }
        self.schedule(0, cx);
    }

    /// 全部折叠;已经都折叠时改为全部展开。
    pub(crate) fn toggle_collapse_all(&mut self, cx: &mut Context<Self>) {
        let all: HashSet<PathBuf> = self.files.iter().map(|f| f.path.clone()).collect();
        self.collapsed = if self.collapsed.is_superset(&all) { HashSet::new() } else { all };
        self.rebuild();
        cx.notify();
    }

    /// 折叠 / 展开一个文件。
    pub(crate) fn toggle_file(&mut self, fi: usize, cx: &mut Context<Self>) {
        let Some(f) = self.files.get(fi) else { return };
        if !self.collapsed.remove(&f.path) {
            self.collapsed.insert(f.path.clone());
        }
        self.rebuild();
        cx.notify();
    }

    /// 点击一处匹配:在编辑器里打开并选中。
    pub(crate) fn open_match(&mut self, fi: usize, mi: usize, cx: &mut Context<Self>) {
        let Some((f, m)) = self.files.get(fi).and_then(|f| f.matches.get(mi).map(|m| (f, m))) else { return };
        self.active = Some((f.path.clone(), m.line, m.start));
        cx.emit(SearchEvent::Open { path: f.path.clone(), line: m.line, start: m.start, end: m.end });
        cx.notify();
    }

    /// 从结果里忽略一处匹配(不改文件,之后的替换也不会碰它)。
    pub(crate) fn dismiss_match(&mut self, fi: usize, mi: usize, cx: &mut Context<Self>) {
        model::dismiss_match(&mut self.files, fi, mi);
        self.rebuild();
        cx.notify();
    }

    /// 从结果里忽略一个文件。
    pub(crate) fn dismiss_file(&mut self, fi: usize, cx: &mut Context<Self>) {
        model::dismiss_file(&mut self.files, fi);
        self.rebuild();
        cx.notify();
    }

    /// 全部替换前先确认(替换会直接改磁盘文件)。
    pub(crate) fn confirm_replace_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (nf, nm) = model::totals(&self.files);
        if nm == 0 {
            return;
        }
        let rep = self.replace_input.read(cx).value().to_string();
        let msg = if rep.is_empty() { format!("确定要从 {nf} 个文件中删除 {nm} 处匹配吗?此操作直接修改磁盘文件。") } else { format!("确定要把 {nf} 个文件中的 {nm} 处匹配替换为「{rep}」吗?此操作直接修改磁盘文件。") };
        let weak = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let weak = weak.clone();
            dialog.title("全部替换").child(div().text_size(px(13.)).child(msg.clone())).footer(crate::ui::dialog_footer("替换")).on_ok(move |_, window, cx| {
                let _ = weak.update(cx, |p, cx| p.start_replace(Scope::All, window, cx));
                true
            })
        });
    }

    /// 执行替换:只改当前结果里列出的匹配(被忽略的不动),在后台线程写盘,完成后提示并重搜。
    pub(crate) fn start_replace(&mut self, scope: Scope, window: &mut Window, cx: &mut Context<Self>) {
        let keys = |f: &FileMatches| -> HashSet<MatchKey> { f.matches.iter().map(|m| (m.line, m.start, m.end)).collect() };
        let targets: Vec<(PathBuf, HashSet<MatchKey>)> = match scope {
            Scope::One(fi, mi) => self.files.get(fi).and_then(|f| f.matches.get(mi).map(|m| (f.path.clone(), [(m.line, m.start, m.end)].into_iter().collect()))).into_iter().collect(),
            Scope::File(fi) => self.files.get(fi).map(|f| (f.path.clone(), keys(f))).into_iter().collect(),
            Scope::All => self.files.iter().map(|f| (f.path.clone(), keys(f))).collect(),
        };
        if targets.is_empty() {
            return;
        }
        let (mut query, replacement, preserve) = (self.query(cx), self.replace_input.read(cx).value().to_string(), self.preserve_case);
        query.include.clear();
        query.exclude.clear();
        cx.spawn_in(window, async move |this, cx| {
            let (done, errors) = cx.background_spawn(async move {
                let (mut done, mut errors) = (0usize, Vec::new());
                for (path, only) in targets {
                    match loom_search::replace_in_file(&path, &query, &replacement, preserve, Some(&only)) {
                        Ok(n) => done += n,
                        Err(e) => errors.push(e),
                    }
                }
                (done, errors)
            }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                match errors.first() {
                    Some(e) => crate::notice::error(window, cx, format!("替换出错:{e}")),
                    None => crate::notice::success(window, cx, format!("已替换 {done} 处")),
                }
                this.schedule(0, cx);
            });
        })
        .detach();
    }
}
