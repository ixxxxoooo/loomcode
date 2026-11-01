//! IDE 编辑器视图:多标签编辑器(参考 Zed 的 Pane:预览标签、MRU 关闭、就近插入),
//! 独立窗口与融合模式共用。文件由宿主通过 `open` 传入;⌘S 保存,⌘W 关闭标签。
//!
//! @author ixxxxoooo

use crate::file_tree::language_for;
use crate::ide_chrome::{TabItem, breadcrumbs, tab_bar, welcome};
use crate::ide_tabs::TabModel;
use crate::ui::empty_state;
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Editor, EditorState, InputEvent};
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

actions!(ide, [CloseTab, NextTab, PrevTab, QuickOpen, GoToLine, FindInProject, NavBack, NavForward, ShowScm, TogglePanel]);

/// 超过这个大小的文件不在编辑器里打开。

/// 一个已打开的文档。
pub struct IdeDoc {
    pub editor: Entity<EditorState>,
    /// 内容是否与磁盘一致
    pub saved: bool,
    /// 文件使用 CRLF 换行(仅用于状态显示)
    pub crlf: bool,
    /// 上次读取 / 保存时磁盘文件的修改时间(用来发现外部改动)
    pub mtime: Option<std::time::SystemTime>,
    /// 磁盘上的文件在本标签有未保存修改时被改动过(冲突)
    pub stale: bool,
    /// Git 行标记装饰集合(首次刷新时创建)
    pub(crate) marks: Option<gpui_kit::component::input::RangeDecorationCollection>,
    /// 迷你地图的着色色块
    pub minimap: std::rc::Rc<crate::minimap_model::MiniMap<Hsla>>,
    /// 迷你地图数据对应的 (语法主题, 是否着色);变了(切主题 / 改设置)就重建
    pub(crate) mm_key: (std::sync::Arc<gpui_kit::component::highlighter::HighlightTheme>, bool),
    /// 迷你地图重建的防抖序号
    pub(crate) mm_gen: u64,
    /// Git 改动行区间(迷你地图刻度用)
    pub hunks: std::rc::Rc<Vec<(std::ops::Range<usize>, crate::git_status::GitMark)>>,
    /// 括号配对高亮:上次的配对位置与装饰集合
    pub(crate) bracket_pair: Option<(usize, usize)>,
    pub(crate) bracket_marks: Option<gpui_kit::component::input::RangeDecorationCollection>,
    /// Git 追溯:每行最后一次修改的提交(下标 = 行号 - 1);没有仓库 / 未跟踪时为空
    pub(crate) blame: std::sync::Arc<Vec<loom_core::git_blame::BlameLine>>,
    /// 追溯刷新的防抖序号(也用来丢弃过期结果)
    pub(crate) blame_gen: u64,
    /// 追溯与当前内容一致(编辑后到刷新完成之间为假)
    pub(crate) blame_fresh: bool,
    /// 语言服务器同步句柄(没有对应服务器 / 未安装 / 功能关闭时为空)
    pub(crate) lsp: Option<std::sync::Arc<crate::lsp::DocSync>>,
    /// 同步给语言服务器的防抖序号
    pub(crate) lsp_gen: u64,
    /// 语言服务器给出的 (错误数, 警告数)
    pub(crate) diag: (usize, usize),
    _subs: Vec<Subscription>,
}

/// 编辑器视图:持有所有标签的文档,不依赖工作台,可被任意窗口嵌入。
pub struct IdeView {
    /// 当前项目根目录,用于显示相对路径
    pub(crate) root: String,
    pub(crate) docs: HashMap<PathBuf, IdeDoc>,
    pub(crate) tabs: TabModel,
    /// 打不开文件或保存失败时的说明
    pub(crate) note: Option<String>,
    /// 以图片方式打开的标签(没有文本文档)
    pub(crate) images: std::collections::HashSet<PathBuf>,
    /// 迷你地图的界面状态(区域、悬停、拖动抓取点、编辑区宽度)
    pub(crate) minimap_ui: std::rc::Rc<crate::ide_minimap::MinimapUi>,
    /// 已套用到所有文档的编辑器偏好(与全局不同就重新套用)
    applied: Option<loom_config::EditorPrefs>,
    /// 编辑计数:延迟自动保存靠它判断「这之后还有没有新的编辑」
    edit_gen: u64,
    /// 文件 / 行位置历史(IDE 标题栏的前进后退)
    pub(crate) nav: crate::nav_history::NavHistory<(PathBuf, u32)>,
    /// 正在按历史跳转或恢复标签:这期间不记录新的历史
    pub(crate) nav_quiet: bool,
    /// 刚打开文件、还没跳行:紧随其后的 `goto` 替换这一条而不是新增
    pub(crate) fresh_open: bool,
    /// 最近打开的文件,最近的在前
    pub(crate) recent: Vec<String>,
    /// 切换项目后待恢复的标签记忆(渲染时有窗口再恢复)
    pub(crate) pending_restore: Option<loom_config::IdeMemory>,
    /// Markdown 文件的预览 / 源码模式与就地编辑状态
    pub(crate) md: HashMap<PathBuf, Entity<crate::ide_markdown::MdDocView>>,
    /// 显示追溯栏(初值取自偏好,面包屑按钮 / 命令面板可临时切换)
    pub(crate) blame_gutter: bool,
    /// 追溯栏的界面状态
    pub(crate) blame_ui: std::rc::Rc<crate::ide_blame::BlameUi>,
    /// 已启动诊断监听
    pub(crate) lsp_listening: bool,
}

impl IdeView {
    /// 创建空的编辑器视图。
    pub fn new() -> Self {
        Self { root: String::new(), docs: HashMap::new(), tabs: TabModel::default(), note: None, images: Default::default(), minimap_ui: Default::default(), applied: None, edit_gen: 0, nav: Default::default(), nav_quiet: false, fresh_open: false, recent: Vec::new(), pending_restore: None, md: HashMap::new(), blame_gutter: false, blame_ui: Default::default(), lsp_listening: false }
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
        self.open_inner(path, preview, true, window, cx);
    }

    /// 打开文件的实现;`focus` 为假时不抢焦点(恢复标签时用)。
    pub(crate) fn open_inner(&mut self, path: PathBuf, preview: bool, focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        // 用户主动打开时,先把切换项目后挂起的标签记忆恢复好(否则稍后的恢复会把当前标签挤掉)
        if focus {
            self.restore_pending(window, cx);
        }
        self.note = None;
        if is_image(&path) {
            self.images.insert(path.clone());
        } else if !self.docs.contains_key(&path) {
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
            self.images.remove(&old);
        }
        if focus {
            self.focus_active(window, cx);
        }
        self.refresh_marks(&path, cx);
        self.refresh_blame(&path, cx);
        self.lsp_attach(&path, cx);
        let line = self.cursor_line(&path, cx);
        self.record_open(&path, line);
        cx.notify();
    }

    /// 激活已有标签(点击标签栏)。
    pub fn activate(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.activate(path);
        self.focus_active(window, cx);
        let line = self.cursor_line(path, cx);
        self.record_open(path, line);
        cx.notify();
    }

    /// 双击标签:固定预览。
    pub fn pin(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.tabs.pin(path);
        cx.notify();
    }

    /// 切换标签的固定状态。
    pub fn toggle_pin_tab(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.tabs.toggle_pin(path);
        cx.notify();
    }

    /// 拖动排序:把 `path` 放到 `target` 的位置。
    pub fn move_tab(&mut self, path: &Path, target: &Path, cx: &mut Context<Self>) {
        self.tabs.move_to(path, target);
        cx.notify();
    }

    /// 标签栏的数据;`active` 为当前标签。
    fn tab_items(&self, active: &Path) -> Vec<TabItem> {
        self.tabs
            .tabs()
            .iter()
            .map(|p| TabItem { path: p.clone(), dirty: self.docs.get(p).is_some_and(|d| !d.saved), preview: self.tabs.is_preview(p), active: p == active, pinned: self.tabs.is_pinned(p) })
            .collect()
    }

    /// 让当前标签的编辑器获得焦点。
    pub(crate) fn focus_active(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(doc) = self.tabs.active().and_then(|p| self.docs.get(p)) {
            doc.editor.update(cx, |e, cx| e.focus(window, cx));
        }
    }

    /// 编辑后 150 毫秒重建迷你地图(防抖:连续输入只重建一次)。
    fn schedule_minimap(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(doc) = self.docs.get_mut(path) else { return };
        doc.mm_gen += 1;
        let (path, gen_at) = (path.to_path_buf(), doc.mm_gen);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(150)).await;
            let _ = this.update(cx, |this, cx| this.rebuild_minimap(&path, Some(gen_at), cx));
        })
        .detach();
    }

    /// 立即重建某文档的迷你地图;`gen_at` 给出时,期间又有编辑(序号变了)就放弃。
    pub(crate) fn rebuild_minimap(&mut self, path: &Path, gen_at: Option<u64>, cx: &mut Context<Self>) {
        let prefs = crate::prefs(cx).editor;
        let theme = cx.theme().highlight_theme.clone();
        let Some(doc) = self.docs.get_mut(path).filter(|d| gen_at.is_none_or(|g| g == d.mm_gen)) else { return };
        let text = doc.editor.read(cx).value().to_string();
        doc.minimap = std::rc::Rc::new(crate::ide_minimap::build_for(&text, language_for(path), prefs.syntax_highlight, &theme));
        doc.mm_key = (theme, prefs.syntax_highlight);
        cx.notify();
    }

    /// 「延迟」自动保存:停止输入 1 秒后写盘;期间又有编辑就放弃这次(由最后一次编辑触发)。
    fn schedule_autosave(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.edit_gen += 1;
        if crate::prefs(cx).editor.auto_save != loom_config::AutoSave::Delay {
            return;
        }
        let (path, gen_at) = (path.to_path_buf(), self.edit_gen);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_secs(1)).await;
            let _ = this.update(cx, |this, cx| {
                if this.edit_gen == gen_at && this.docs.get(&path).is_some_and(|d| !d.saved) {
                    this.save_path(&path, cx);
                }
            });
        })
        .detach();
    }

    /// 偏好变了就套到所有已打开的文档(渲染时调用,代价只是一次相等比较)。
    fn sync_prefs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let p = crate::prefs(cx).editor;
        if self.applied.as_ref() == Some(&p) {
            return;
        }
        // 语法高亮开关只影响之后打开的文件;其余偏好即时生效
        for doc in self.docs.values() {
            doc.editor.update(cx, |ed, cx| crate::ide_prefs::apply(ed, &p, window, cx));
        }
        // 追溯栏偏好变了才覆盖面包屑按钮的临时状态
        if self.applied.as_ref().is_none_or(|a| a.blame_gutter != p.blame_gutter) {
            self.blame_gutter = p.blame_gutter;
        }
        self.applied = Some(p);
    }

    /// 在后台线程重新计算某文件的 Git 行标记,完成后铺到编辑器上。
    pub fn refresh_marks(&mut self, path: &Path, cx: &mut Context<Self>) {
        let path = path.to_path_buf();
        cx.spawn(async move |this, cx| {
            let p = path.clone();
            let hunks = cx.background_spawn(async move { crate::ide_gutter::read_hunks(&p) }).await;
            let _ = this.update(cx, |this, cx| {
                if let Some(doc) = this.docs.get_mut(&path) {
                    doc.hunks = std::rc::Rc::new(hunks.clone());
                    crate::ide_gutter::apply(&doc.editor, &mut doc.marks, hunks, cx);
                }
            });
        })
        .detach();
    }

    /// 保存当前文件到磁盘。
    pub fn save(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.tabs.active().map(Path::to_path_buf) {
            self.save_path(&path, cx);
        }
    }

    /// 保存指定标签;返回是否成功(没有该文档也算失败)。
    pub fn save_path(&mut self, path: &Path, cx: &mut Context<Self>) -> bool {
        let path = path.to_path_buf();
        let Some(doc) = self.docs.get_mut(&path) else { return false };
        let text = doc.editor.read(cx).value().to_string();
        match std::fs::write(&path, &text) {
            Ok(()) => {
                if let Some(sync) = &doc.lsp {
                    sync.did_save(&text);
                }
                doc.saved = true;
                doc.stale = false;
                doc.mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                self.note = None;
                self.tabs.pin(&path);
            }
            Err(e) => {
                loom_diag::error!("保存文件失败 {}:{e}", path.display());
                self.note = Some(format!("保存失败:{e}"));
                cx.notify();
                return false;
            }
        }
        self.refresh_marks(&path, cx);
        self.refresh_blame(&path, cx);
        cx.notify();
        true
    }

    /// 从磁盘重新载入当前文件(丢弃未保存的修改)。
    pub fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.tabs.active().map(Path::to_path_buf) else { return };
        self.docs.remove(&path);
        match load(&path, window, cx) {
            Ok(doc) => {
                self.docs.insert(path.clone(), doc);
                self.lsp_attach(&path, cx);
                self.focus_active(window, cx);
            }
            Err(note) => self.note = Some(note),
        }
        cx.notify();
    }

    /// 把光标移到指定行列(1 起始),并让编辑器获得焦点。
    pub fn goto(&mut self, line: u32, col: u32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.tabs.active().and_then(|p| self.docs.get(p)) else { return };
        let pos = gpui_kit::component::input::Position::new(line.saturating_sub(1), col.saturating_sub(1));
        doc.editor.update(cx, |e, cx| e.set_cursor_position(pos, window, cx));
        if let Some(path) = self.tabs.active().map(Path::to_path_buf) {
            self.record_jump(&path, line);
        }
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

/// 按扩展名判断是否图片(用图片查看而不是文本编辑)。
pub fn is_image(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_lowercase().as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ico" | "bmp"))
}

/// 读取文本文件并建立编辑器;二进制或过大的文件返回说明。
fn load(path: &Path, window: &mut Window, cx: &mut Context<IdeView>) -> Result<IdeDoc, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("无法读取 {}:{e}", path.display()))?;
    let prefs = crate::prefs(cx).editor;
    if meta.len() > u64::from(prefs.max_file_kb) * 1024 {
        return Err(format!("文件较大({} KB),不在编辑器中打开", meta.len() / 1024));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("无法读取 {}:{e}", path.display()))?;
    let text = String::from_utf8(bytes).map_err(|_| "二进制文件,不在编辑器中打开".to_string())?;
    let crlf = text.contains("\r\n");
    let theme = cx.theme().highlight_theme.clone();
    let minimap = std::rc::Rc::new(crate::ide_minimap::build_for(&text, language_for(path), prefs.syntax_highlight, &theme));
    let editor = crate::ide_prefs::new_editor(language_for(path), text, true, &prefs, window, cx);
    // 编辑后标记未保存并固定预览标签;光标移动时刷新状态条
    let key = path.to_path_buf();
    let sub = cx.subscribe(&editor, move |this: &mut IdeView, _, ev: &InputEvent, cx| {
        match ev {
            InputEvent::Change => {
                this.tabs.pin(&key);
                if let Some(doc) = this.docs.get_mut(&key) {
                    doc.saved = false;
                }
                this.schedule_minimap(&key, cx);
                this.schedule_blame(&key, cx);
                this.schedule_lsp_sync(&key, cx);
                this.schedule_autosave(&key, cx);
            }
            // 失焦自动保存:编辑器失去焦点(切标签 / 点到别处 / 窗口失焦)时写盘
            InputEvent::Blur if crate::prefs(cx).editor.auto_save == loom_config::AutoSave::Blur => {
                if this.docs.get(&key).is_some_and(|d| !d.saved) {
                    this.save_path(&key, cx);
                }
            }
            _ => {}
        }
        cx.notify();
    });
    // 光标移动 / 滚动时编辑器只会 notify(没有事件):刷新括号配对高亮与缩略图视口
    let key2 = path.to_path_buf();
    let sub2 = cx.observe(&editor, move |this: &mut IdeView, ed, cx| {
        if let Some(doc) = this.docs.get_mut(&key2).filter(|_| crate::prefs(cx).editor.bracket_match) {
            crate::ide_brackets::refresh(&ed, &mut doc.bracket_marks, &mut doc.bracket_pair, cx);
        }
        cx.notify();
    });
    let marks = None;
    let mtime = meta.modified().ok();
    Ok(IdeDoc { editor, saved: true, crlf, mtime, stale: false, marks, minimap, mm_key: (theme, prefs.syntax_highlight), mm_gen: 0, hunks: Default::default(), bracket_pair: None, bracket_marks: None, blame: Default::default(), blame_gen: 0, blame_fresh: false, lsp: None, lsp_gen: 0, diag: (0, 0), _subs: vec![sub, sub2] })
}

impl Render for IdeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.restore_pending(window, cx);
        self.sync_prefs(window, cx);
        let t = cx.theme().clone();
        let root = v_flex()
            .key_context("IdeView")
            .size_full()
            .bg(t.background)
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| this.close_active(window, cx)))
            .on_action(cx.listener(|this, _: &NextTab, window, cx| this.cycle(1, window, cx)))
            .on_action(cx.listener(|this, _: &PrevTab, window, cx| this.cycle(-1, window, cx)));
        let active = self.tabs.active().map(Path::to_path_buf);
        if let Some(img_path) = active.clone().filter(|p| self.images.contains(p)) {
            return root.child(tab_bar(self.tab_items(&img_path), cx)).child(breadcrumbs(&img_path, &self.root, true, None, None, cx)).child(
                div().flex_1().min_h_0().flex().items_center().justify_center().p_4().child(img(img_path).max_w_full().max_h_full()),
            ).into_any_element();
        }
        let Some(path) = active.filter(|p| self.docs.contains_key(p)) else {
            if let Some(note) = self.note.clone() {
                return root.child(empty_state(IconName::Code, "无法打开", &note, cx)).into_any_element();
            }
            return root.child(welcome(cx)).into_any_element();
        };
        // 切换主题 / 语法高亮开关后,当前文档的迷你地图配色要跟着重建
        let want = (cx.theme().highlight_theme.clone(), crate::prefs(cx).editor.syntax_highlight);
        if self.docs.get(&path).is_some_and(|d| !std::sync::Arc::ptr_eq(&d.mm_key.0, &want.0) || d.mm_key.1 != want.1) {
            self.rebuild_minimap(&path, None, cx);
        }
        let is_md = crate::ide_markdown::is_markdown(&path);
        let ed0 = self.docs[&path].editor.clone();
        let md_view = is_md.then(|| self.md.entry(path.clone()).or_insert_with(|| cx.new(|cx| crate::ide_markdown::MdDocView::new(ed0, None, window, cx))).clone());
        let md_mode = md_view.as_ref().map_or(crate::ide_markdown::MdMode::Source, |v| v.read(cx).mode);
        let doc = &self.docs[&path];
        let editor = doc.editor.clone();
        root.child(tab_bar(self.tab_items(&path), cx))
            .child(breadcrumbs(&path, &self.root, doc.saved, self.note.clone().or_else(|| doc.stale.then(|| "磁盘上的文件已被修改,点右侧刷新按钮载入(会丢弃你的修改)".to_string())), Some(self.blame_gutter), cx))
            .children(md_view.as_ref().map(|v| crate::ide_markdown::mode_bar(v, md_mode, cx)))
            .child({
                let prefs = crate::prefs(cx).editor;
                let gutter = self.blame_gutter.then(|| crate::ide_blame::gutter(doc, &self.blame_ui, cx)).flatten();
                let inline = crate::ide_blame::inline_blame(doc, cx);
                let ui = self.minimap_ui.clone();
                // 编辑区太窄时自动隐藏迷你地图(宽度取自上一帧的测量)
                let wide = ui.row_width.get() == 0. || ui.row_width.get() >= crate::ide_minimap::MIN_EDITOR_W;
                let mm = (prefs.minimap && wide).then(|| crate::ide_minimap::minimap(&editor, doc.minimap.clone(), doc.hunks.clone(), ui.clone(), crate::ide_minimap::MinimapOpts::from_prefs(&prefs), cx));
                let source = h_flex().relative().flex_1().min_w_0().min_h_0().w_full().h_full()
                    .child(canvas(move |b, _, _| ui.row_width.set(f32::from(b.size.width)), |_, _, _, _| {}).absolute().size_full())
                    .children(gutter)
                    // 迷你地图兼作滚动条:把编辑器多撑出 16px 并裁掉,盖住它自带的纵向滚动条(自动换行时保留,否则折行会被裁)
                    .child({
                        let hide = mm.is_some() && !prefs.soft_wrap;
                        div().flex_1().min_w_0().h_full().relative().overflow_hidden()
                            .child(div().absolute().top_0().left_0().bottom_0().right(px(if hide { -16. } else { 0. }))
                                .child(Editor::new(&editor).bordered(false).appearance(false).size_full()))
                    })
                    .children(mm)
                    .children(inline);
                match md_mode {
                    crate::ide_markdown::MdMode::Source => h_flex().flex_1().min_h_0().w_full().child(source).into_any_element(),
                    crate::ide_markdown::MdMode::Preview => h_flex().flex_1().min_h_0().w_full().child(md_view.clone().unwrap()).into_any_element(),
                    crate::ide_markdown::MdMode::Split => h_flex().flex_1().min_h_0().w_full().child(div().flex_1().min_w_0().h_full().child(source)).child(div().w(px(1.)).h_full().bg(t.border)).child(div().flex_1().min_w_0().h_full().child(md_view.clone().unwrap())).into_any_element(),
                }
            })
            .into_any_element()
    }
}
