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
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

actions!(ide, [GoToImplementation, FindReferences, RenameSymbol, ChangeAllOccurrences, ShowRefactor, ShowSourceActions, ToggleBlameAnnotations, CompareWithSaved, CompareWithClipboard, ToggleWordWrap, ShowFileHistory, OpenCommandPalette, GoToTypeDefinition, FormatDocument, CloseTab, NextTab, PrevTab, QuickOpen, GoToLine, FindInProject, NavBack, NavForward, ShowScm, TogglePanel]);

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
    pub(crate) pairs: crate::ide_pairs::PairState,
    pub(crate) bracket_pair: Option<(usize, usize)>,
    pub(crate) bracket_marks: Option<gpui_kit::component::input::RangeDecorationCollection>,
    /// 括号配对着色与竖线:扫描缓存与底色块
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
    /// 诊断所在行与是否错误(迷你地图刻度用)
    pub(crate) diag_marks: std::rc::Rc<Vec<(usize, bool)>>,
    /// 悬停卡片(语言服务器的悬停结果,自己渲染)
    pub(crate) hover: Option<crate::ide_hover::HoverCard>,
    /// 大纲(符号树)与刷新的防抖序号
    pub(crate) outline: std::rc::Rc<Vec<crate::outline::OutlineItem>>,
    pub(crate) outline_gen: u64,
    _subs: Vec<Subscription>,
}

/// 编辑器视图:持有所有标签的文档,不依赖工作台,可被任意窗口嵌入。
pub struct IdeView {
    /// 当前项目根目录,用于显示相对路径
    pub(crate) root: String,
    pub(crate) docs: HashMap<PathBuf, IdeDoc>,
    pub(crate) tabs: TabModel,
    /// 鼠标当前悬停在哪个窗口里的编辑器上(⌘W 优先关悬停处的标签,见 `app_menu::close_tab_under_mouse`)
    pub(crate) hover_win: Option<WindowId>,
    /// 面包屑下拉(打开时)
    pub(crate) crumb: Option<(Entity<crate::crumb_picker::CrumbPicker>, Subscription)>,
    /// ⌘ 单击后的 (文件, 选区),抬起时据此补发「转到定义」(见 `ide_link`)
    pub(crate) cmd_click: Option<(PathBuf, std::ops::Range<usize>)>,
    /// 打不开文件或保存失败时的说明
    pub(crate) note: Option<String>,
    /// 以图片方式打开的标签(没有文本文档)
    pub(crate) images: std::collections::HashSet<PathBuf>,
    /// 表格标签(csv / xlsx…)的视图
    pub(crate) sheets: HashMap<PathBuf, Entity<crate::sheet_view::SheetView>>,
    pub(crate) sheet_subs: HashMap<PathBuf, Subscription>,
    /// 被选成「以文本打开」的分隔文本文件
    pub(crate) as_text: std::collections::HashSet<PathBuf>,
    /// 差异标签(源代码管理里单击文件打开,伪路径 `diff:…`)
    pub(crate) diffs: HashMap<PathBuf, crate::ide_diff::DiffTab>,
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
    /// 临时记事本编号计数
    pub(crate) untitled_seq: u32,
    /// 标签栏放不下时换行显示(对标 VS Code `wrapTabs`),在标签栏「···」菜单里切换
    pub(crate) wrap_tabs: bool,
    /// 拆分:次编辑器组(仅主组持有)
    pub(crate) split: Option<crate::ide_split::Split>,
    /// 本视图是次组时,指向主组
    pub(crate) parent: Option<WeakEntity<IdeView>>,
    /// 外部操作落在次组(最近点击的是次组)
    pub(crate) active_second: bool,
    /// 已另存、待换成真实文件标签的 (临时路径, 新路径)
    pub(crate) saved_as: Vec<(PathBuf, PathBuf)>,
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
    /// 右键菜单结果浮层(引用 / 重命名 / 代码操作 / 差异)
    pub(crate) overlay: Option<crate::ide_overlay::Overlay>,
    /// 光标处的小消息(找不到定义等)
    pub(crate) cursor_msg: Option<crate::ide_message::CursorMsg>,
    pub(crate) overlay_sub: Option<Subscription>,
    /// 悬停卡片的收起计时与鼠标靠近判断
    pub(crate) hover_ui: crate::ide_hover::HoverUi,
    /// 标签条的滚动句柄:标签放不下时横向滚动,当前标签变化后滚到可见处
    pub(crate) tabs_scroll: ScrollHandle,
    /// 上一次已滚到可见处的 (当前标签, 标签数):变了才重新定位,不干扰用户手动滚动
    tabs_revealed: Option<(PathBuf, usize)>,
}

impl IdeView {
    /// 创建空的编辑器视图。
    pub fn new() -> Self {
        Self { root: String::new(), docs: HashMap::new(), tabs: TabModel::default(), hover_win: None, crumb: None, cmd_click: None, note: None, images: Default::default(), sheets: HashMap::new(), sheet_subs: HashMap::new(), diffs: HashMap::new(), as_text: Default::default(), minimap_ui: Default::default(), applied: None, edit_gen: 0, nav: Default::default(), nav_quiet: false, fresh_open: false, recent: Vec::new(), pending_restore: None, md: HashMap::new(), blame_gutter: false, blame_ui: Default::default(), lsp_listening: false, hover_ui: Default::default(), overlay: None, cursor_msg: None, overlay_sub: None, tabs_scroll: ScrollHandle::new(), tabs_revealed: None, untitled_seq: 0, wrap_tabs: true, split: None, parent: None, active_second: false, saved_as: Vec::new() }
    }

    /// 设置项目根目录(会话切换时由宿主调用)。
    pub fn set_root(&mut self, root: String, cx: &mut Context<Self>) {
        if self.root != root {
            if let Some(sp) = &self.split {
                let r = root.clone();
                sp.child.update(cx, |c, cx| c.set_root(r, cx));
            }
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
        if let Some(c) = self.focused_child() {
            c.update(cx, |c, cx| c.open_inner(path, preview, focus, window, cx));
            return;
        }
        // 用户主动打开时,先把切换项目后挂起的标签记忆恢复好(否则稍后的恢复会把当前标签挤掉)
        if focus {
            self.restore_pending(window, cx);
        }
        self.note = None;
        if is_image(&path) {
            self.images.insert(path.clone());
        } else if self.wants_sheet(&path) {
            self.open_sheet(&path, window, cx);
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
            self.drop_sheet(&old);
            self.drop_diff(&old);
        }
        if focus {
            self.focus_active(window, cx);
        }
        self.refresh_marks(&path, cx);
        self.refresh_blame(&path, cx);
        self.lsp_attach(&path, cx);
        self.refresh_outline(&path, cx);
        let line = self.cursor_line(&path, cx);
        self.record_open(&path, line);
        cx.notify();
    }

    /// 激活已有标签(点击标签栏)。
    pub fn activate(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.activate(path);
        // 点到被遮挡的标签:重新滚到完全可见
        self.tabs_revealed = None;
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

    /// 当前标签或标签数变了就让标签条滚到当前标签(参考 VS Code:激活的标签始终可见)。
    fn reveal_active_tab(&mut self, active: &Path) {
        let sig = (active.to_path_buf(), self.tabs.tabs().len());
        if self.tabs_revealed.as_ref() == Some(&sig) {
            return;
        }
        if let Some(ix) = self.tabs.tabs().iter().position(|p| p == active) {
            self.tabs_scroll.scroll_to_item(ix);
        }
        self.tabs_revealed = Some(sig);
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
        } else if let Some(sv) = self.tabs.active().and_then(|p| self.sheets.get(p)) {
            let focus = sv.read(cx).focus.clone();
            focus.focus(window, cx);
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
        if crate::prefs(cx).editor.auto_save != loom_config::AutoSave::Delay || crate::ide_untitled::is_untitled(path) {
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
        if crate::ide_untitled::is_untitled(path) {
            return;
        }
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
        if let Some(c) = self.focused_child() {
            c.update(cx, |c, cx| c.save(cx));
            return;
        }
        if let Some(path) = self.tabs.active().map(Path::to_path_buf) {
            self.save_path(&path, cx);
        }
    }

    /// 保存指定标签;返回是否成功(没有该文档也算失败)。
    pub fn save_path(&mut self, path: &Path, cx: &mut Context<Self>) -> bool {
        if crate::ide_untitled::is_untitled(path) {
            return self.save_untitled(path, cx);
        }
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
                self.sync_saved(&path, cx);
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
        let Some(path) = self.tabs.active().map(Path::to_path_buf).filter(|p| !crate::ide_diff::is_diff(p) && !crate::ide_untitled::is_untitled(p)) else { return };
        self.docs.remove(&path);
        match load(&path, window, cx) {
            Ok(doc) => {
                self.docs.insert(path.clone(), doc);
                self.lsp_attach(&path, cx);
                self.refresh_outline(&path, cx);
                self.focus_active(window, cx);
            }
            Err(note) => self.note = Some(note),
        }
        cx.notify();
    }

    /// 把光标移到指定行列(1 起始),并让编辑器获得焦点。
    pub fn goto(&mut self, line: u32, col: u32, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(c) = self.focused_child() {
            c.update(cx, |c, cx| c.goto(line, col, window, cx));
            return;
        }
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
        if let Some(c) = self.focused_child() {
            return c.read(cx).cursor_info(cx);
        }
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
pub(crate) fn load(path: &Path, window: &mut Window, cx: &mut Context<IdeView>) -> Result<IdeDoc, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("无法读取 {}:{e}", path.display()))?;
    let prefs = crate::prefs(cx).editor;
    if meta.len() > u64::from(prefs.max_file_kb) * 1024 {
        return Err(format!("文件较大({} KB),不在编辑器中打开", meta.len() / 1024));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("无法读取 {}:{e}", path.display()))?;
    let text = String::from_utf8(bytes).map_err(|_| "二进制文件,不在编辑器中打开".to_string())?;
    Ok(build_doc(path, text, meta.modified().ok(), window, cx))
}

/// 由文本构建文档(磁盘文件与临时记事本共用)。
///
/// 参数:`path` 标签路径;`text` 初始内容;`mtime` 磁盘修改时间(临时记事本为 `None`)。
/// 返回:文档。
pub(crate) fn build_doc(path: &Path, text: String, mtime: Option<std::time::SystemTime>, window: &mut Window, cx: &mut Context<IdeView>) -> IdeDoc {
    let prefs = crate::prefs(cx).editor;
    let crlf = text.contains("\r\n");
    let theme = cx.theme().highlight_theme.clone();
    let minimap = std::rc::Rc::new(crate::ide_minimap::build_for(&text, language_for(path), prefs.syntax_highlight, &theme));
    let editor = crate::ide_prefs::new_editor(language_for(path), text, true, &prefs, window, cx);
    // 右键菜单由我们自己的主题化弹出菜单(`ide_menu.rs`)接管,关掉编辑器自带的系统菜单
    editor.update(cx, |e, _| e.set_context_menu_enabled(false));
    // 编辑后标记未保存并固定预览标签;光标移动时刷新状态条
    let key = path.to_path_buf();
    let sub = cx.subscribe_in(&editor, window, move |this: &mut IdeView, _, ev: &InputEvent, window, cx| {
        match ev {
            InputEvent::Change => {
                this.tabs.pin(&key);
                if let Some(doc) = this.docs.get_mut(&key) {
                    doc.saved = false;
                    doc.hover = None;
                    doc.pairs.bump();
                }
                this.schedule_minimap(&key, cx);
                this.schedule_blame(&key, cx);
                this.schedule_lsp_sync(&key, cx);
                this.schedule_outline(&key, cx);
                this.schedule_autosave(&key, cx);
                this.sync_sibling(&key, window, cx);
            }
            // 失焦自动保存:编辑器失去焦点(切标签 / 点到别处 / 窗口失焦)时写盘
            InputEvent::Blur if crate::prefs(cx).editor.auto_save == loom_config::AutoSave::Blur && !crate::ide_untitled::is_untitled(&key) => {
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
        // 光标移动 / 滚动:收起悬停卡片
        this.hover_check_moved(&key2, cx);
        this.cursor_msg_check_moved(cx);
        if let Some(doc) = this.docs.get_mut(&key2).filter(|_| crate::prefs(cx).editor.bracket_match) {
            crate::ide_brackets::refresh(&ed, &mut doc.bracket_marks, &mut doc.bracket_pair, cx);
        }
        cx.notify();
    });
    let marks = None;
    IdeDoc { editor, saved: true, crlf, mtime, stale: false, marks, minimap, mm_key: (theme, prefs.syntax_highlight), mm_gen: 0, hunks: Default::default(), pairs: crate::ide_pairs::PairState::new(language_for(path)), bracket_pair: None, bracket_marks: None, blame: Default::default(), blame_gen: 0, blame_fresh: false, lsp: None, lsp_gen: 0, diag: (0, 0), diag_marks: Default::default(), hover: None, outline: Default::default(), outline_gen: 0, _subs: vec![sub, sub2] }
}

/// 编辑器对宿主的通知。
pub enum IdeEvent {
    /// 请在资源管理器(文件树)里定位这个文件
    RevealInExplorer(PathBuf),
    /// 次编辑器组的标签已全部关闭,请主组合并拆分
    GroupEmpty,
}

impl EventEmitter<IdeEvent> for IdeView {}

impl IdeView {
    /// 弹出面包屑下拉:列出 `dir` 的内容,`current` 为当前文件,`pos` 为弹出位置。
    pub(crate) fn open_crumb(&mut self, dir: PathBuf, current: PathBuf, pos: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let picker = cx.new(|cx| crate::crumb_picker::CrumbPicker::new(dir, current, pos, window, cx));
        let sub = cx.subscribe_in(&picker, window, |this: &mut Self, _, ev: &crate::crumb_picker::CrumbEvent, window, cx| {
            this.crumb = None;
            if let crate::crumb_picker::CrumbEvent::Open(path) = ev {
                this.open(path.clone(), false, window, cx);
            }
            this.focus_active(window, cx);
            cx.notify();
        });
        self.crumb = Some((picker, sub));
        cx.notify();
    }
}

impl Render for IdeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 调试用:LOOM_SPLIT=right|down 在首次有打开文件时自动拆分
        if self.split.is_none() && self.parent.is_none() && self.tabs.active().is_some() {
            static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if let Ok(m) = std::env::var("LOOM_SPLIT") {
                if !DONE.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    self.split_active(m == "down", window, cx);
                }
            }
        }
        self.render_with_split(window, cx)
    }
}

impl IdeView {
    /// 渲染本组自己(标签栏 + 编辑区);拆分时由 `render_with_split` 调用。
    pub(crate) fn render_group(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.restore_pending(window, cx);
        self.finish_save_as(window, cx);
        self.sync_prefs(window, cx);
        let t = cx.theme().clone();
        let root = v_flex()
            .key_context("IdeView")
            .id("ide-view")
            .children(self.crumb.as_ref().map(|(p, _)| p.clone()))
            .on_hover({
                let me = cx.entity().downgrade();
                move |on, window, cx| {
                    let id = on.then(|| window.window_handle().window_id());
                    let _ = me.update(cx, |this, _| this.hover_win = id);
                }
            })
            .size_full()
            .bg(t.background)
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| this.close_active(window, cx)))
            .on_action(cx.listener(|this, _: &GoToTypeDefinition, window, cx| this.go_to_type_definition(window, cx)))
            .on_action(cx.listener(|this, _: &GoToImplementation, window, cx| this.go_to_implementation(window, cx)))
            .on_action(cx.listener(|this, _: &FindReferences, window, cx| this.find_references(window, cx)))
            .on_action(cx.listener(|this, _: &RenameSymbol, window, cx| this.begin_rename(false, window, cx)))
            .on_action(cx.listener(|this, _: &ChangeAllOccurrences, window, cx| this.begin_rename(true, window, cx)))
            .on_action(cx.listener(|this, _: &ShowRefactor, window, cx| this.show_code_actions("refactor", "重构", window, cx)))
            .on_action(cx.listener(|this, _: &ShowSourceActions, window, cx| this.show_code_actions("source", "源代码操作", window, cx)))
            .on_action(cx.listener(|this, _: &ToggleBlameAnnotations, _, cx| this.toggle_blame_gutter(cx)))
            .on_action(cx.listener(|this, _: &CompareWithSaved, window, cx| this.compare_with_saved(window, cx)))
            .on_action(cx.listener(|this, _: &CompareWithClipboard, window, cx| this.compare_with_clipboard(window, cx)))
            .on_action(cx.listener(|this, _: &ToggleWordWrap, _, cx| this.toggle_word_wrap(cx)))
            .on_action(cx.listener(|this, _: &FormatDocument, window, cx| this.format_document(window, cx)))
            .on_action(cx.listener(|this, _: &NextTab, window, cx| this.cycle(1, window, cx)))
            .on_action(cx.listener(|this, _: &PrevTab, window, cx| this.cycle(-1, window, cx)));
        let active = self.tabs.active().map(Path::to_path_buf);
        if let Some(a) = &active {
            self.reveal_active_tab(a);
        }
        if let Some(img_path) = active.clone().filter(|p| self.images.contains(p)) {
            return root.child(tab_bar(self.tab_items(&img_path), self.tabs_scroll.clone(), &self.root, self.wrap_tabs, self.is_split(), cx)).child(breadcrumbs(&img_path, &self.root, true, None, None, cx)).child(
                div().flex_1().min_h_0().flex().items_center().justify_center().p_4().child(img(img_path).max_w_full().max_h_full()),
            ).into_any_element();
        }
        if let Some(diff_path) = active.clone().filter(|p| self.diffs.contains_key(p)) {
            let body = self.diff_view(&diff_path, cx);
            return root.child(tab_bar(self.tab_items(&diff_path), self.tabs_scroll.clone(), &self.root, self.wrap_tabs, self.is_split(), cx)).child(div().flex_1().min_h_0().child(body)).into_any_element();
        }
        if let Some(sheet_path) = active.clone().filter(|p| self.sheets.contains_key(p)) {
            let view = self.sheets[&sheet_path].clone();
            return root.child(tab_bar(self.tab_items(&sheet_path), self.tabs_scroll.clone(), &self.root, self.wrap_tabs, self.is_split(), cx)).child(breadcrumbs(&sheet_path, &self.root, true, None, None, cx)).child(div().flex_1().min_h_0().child(view)).into_any_element();
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
        // 括号着色:开关或深浅色变了,给所有文档重装高亮工厂(渲染里不能改编辑器,推迟到本帧之后)
        if crate::ide_rainbow::sync(crate::prefs(cx).editor.bracket_colors, cx.theme().is_dark()) {
            cx.defer_in(window, |this, _, cx| {
                for doc in this.docs.values() {
                    doc.editor.update(cx, |e, cx| crate::ide_rainbow::install(e, cx));
                }
            });
        }
        let overlay = self.overlay_element(cx);
        let doc = &self.docs[&path];
        let editor = doc.editor.clone();
        root.child(tab_bar(self.tab_items(&path), self.tabs_scroll.clone(), &self.root, self.wrap_tabs, self.is_split(), cx))
            .child(breadcrumbs(&path, &self.root, doc.saved, self.note.clone().or_else(|| doc.stale.then(|| "磁盘上的文件已被修改,点右侧刷新按钮载入(会丢弃你的修改)".to_string())), Some(self.blame_gutter), cx))
            .children(md_view.as_ref().map(|v| crate::ide_markdown::mode_bar(v, md_mode, cx)))
            .child({
                let prefs = crate::prefs(cx).editor;
                let gutter = self.blame_gutter.then(|| crate::ide_blame::gutter(doc, &self.blame_ui, cx)).flatten();
                let inline = crate::ide_blame::inline_blame(doc, cx);
                let guide = crate::ide_guides::overlay(doc, cx);
                let pair_guides = crate::prefs(cx).editor.bracket_guides.then(|| crate::ide_pairs::guides(doc));
                let hover = crate::ide_hover::hover_card(doc, &path, cx.weak_entity(), window, cx);
                let cursor_msg = self.cursor_msg.as_ref().and_then(|m| crate::ide_message::message_element(doc, m, cx.weak_entity(), cx));
                let ctx_menu = crate::ide_menu::editor_menu(&editor, cx.weak_entity());
                // ⌘ 单击补强(见 `ide_link`)
                let link_down = {
                    let (p, ed) = (path.clone(), editor.clone());
                    cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                        let sel = ed.read(cx).selected_range();
                        this.link_mouse_down(&p, e, sel);
                    })
                };
                let link_up = {
                    let (p, ed) = (path.clone(), editor.clone());
                    cx.listener(move |this, _: &MouseUpEvent, window, cx| {
                        let sel = ed.read(cx).selected_range();
                        this.link_mouse_up(&p, sel, window, cx);
                    })
                };
                let ui = self.minimap_ui.clone();
                // 编辑区太窄时自动隐藏迷你地图(宽度取自上一帧的测量)
                let wide = ui.row_width.get() == 0. || ui.row_width.get() >= crate::ide_minimap::MIN_EDITOR_W;
                let mm = (prefs.minimap && wide).then(|| crate::ide_minimap::minimap(&editor, doc.minimap.clone(), doc.hunks.clone(), doc.diag_marks.clone(), ui.clone(), crate::ide_minimap::MinimapOpts::from_prefs(&prefs), cx));
                let source = h_flex().relative().flex_1().min_w_0().min_h_0().w_full().h_full()
                    .child(canvas(move |b, _, _| ui.row_width.set(f32::from(b.size.width)), |_, _, _, _| {}).absolute().size_full())
                    .children(gutter)
                    // 迷你地图兼作滚动条:把编辑器多撑出 16px 并裁掉,盖住它自带的纵向滚动条(自动换行时保留,否则折行会被裁)
                    .child({
                        let hide = mm.is_some() && !prefs.soft_wrap;
                        div().flex_1().min_w_0().h_full().relative().overflow_hidden()
                            .on_mouse_down(MouseButton::Left, link_down)
                            .on_mouse_up(MouseButton::Left, link_up)
                            .child(crate::ide_link::modifier_probe())
                            .child(div().absolute().top_0().left_0().bottom_0().right(px(if hide { -16. } else { 0. }))
                                .child(div().size_full().context_menu(ctx_menu).child(Editor::new(&editor).bordered(false).appearance(false).size_full())))
                    })
                    .children(mm)
                    .child(guide)
                    .children(pair_guides)
                    .children(inline)
                    .children(hover)
                    .children(cursor_msg);
                match md_mode {
                    crate::ide_markdown::MdMode::Source => h_flex().flex_1().min_h_0().w_full().child(source).into_any_element(),
                    crate::ide_markdown::MdMode::Preview => h_flex().flex_1().min_h_0().w_full().child(md_view.clone().unwrap()).into_any_element(),
                    crate::ide_markdown::MdMode::Split => h_flex().flex_1().min_h_0().w_full().child(div().flex_1().min_w_0().h_full().child(source)).child(div().w(px(1.)).h_full().bg(t.border)).child(div().flex_1().min_w_0().h_full().child(md_view.clone().unwrap())).into_any_element(),
                }
            })
            .children(overlay)
            .into_any_element()
    }
}
