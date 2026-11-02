//! 资源管理器的分区(对标 VS Code 的「打开的编辑器 / 文件夹 / 大纲 / 时间线」):
//! 每个区块可折叠、可在 `···` 菜单里显示 / 隐藏,状态记进 `state.json`。
//! 区块内容:「打开的编辑器」与「时间线」在这里,「大纲」在 `outline.rs`,「文件夹」是文件树本体(`file_tree_view.rs`)。
//!
//! @author ixxxxoooo

use crate::ide::IdeView;
use crate::scm::ScmView;
use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::git_log::{self, Commit};
use std::path::{Path, PathBuf};

/// 资源管理器里的区块。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    OpenEditors,
    Folder,
    Outline,
    Timeline,
}

impl Section {
    /// 从上到下的显示顺序。
    pub const ALL: [Section; 4] = [Section::OpenEditors, Section::Folder, Section::Outline, Section::Timeline];

    pub fn key(self) -> &'static str {
        match self {
            Section::OpenEditors => "open-editors",
            Section::Folder => "folder",
            Section::Outline => "outline",
            Section::Timeline => "timeline",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Section::OpenEditors => "打开的编辑器",
            Section::Folder => "文件夹",
            Section::Outline => "大纲",
            Section::Timeline => "时间线",
        }
    }

    fn ix(self) -> usize {
        self as usize
    }
}

/// 各区块的显示 / 折叠状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionState {
    hidden: [bool; 4],
    collapsed: [bool; 4],
}

impl Default for SectionState {
    /// 默认:四个区块都显示;「时间线」默认折叠(和 VS Code 一样)。
    fn default() -> Self {
        Self { hidden: [false; 4], collapsed: [false, false, false, true] }
    }
}

impl SectionState {
    pub fn visible(&self, s: Section) -> bool {
        !self.hidden[s.ix()]
    }

    /// 显示且展开。
    pub fn open(&self, s: Section) -> bool {
        self.visible(s) && !self.collapsed[s.ix()]
    }

    pub fn toggle_collapsed(&mut self, s: Section) {
        self.collapsed[s.ix()] = !self.collapsed[s.ix()];
    }

    /// 显示 / 隐藏;重新显示时一并展开(隐藏前是折叠的也一样,免得「显示了却看不到内容」)。
    pub fn toggle_hidden(&mut self, s: Section) {
        self.hidden[s.ix()] = !self.hidden[s.ix()];
        if !self.hidden[s.ix()] {
            self.collapsed[s.ix()] = false;
        }
    }

    /// 排在最前面的可见区块(它的标题行放 `···` 菜单)。
    pub fn first_visible(&self) -> Option<Section> {
        Section::ALL.into_iter().find(|s| self.visible(*s))
    }

    /// 写进记忆:(隐藏的区块键, 折叠的区块键)。
    pub fn to_keys(&self) -> (Vec<String>, Vec<String>) {
        let pick = |a: &[bool; 4]| Section::ALL.into_iter().filter(|s| a[s.ix()]).map(|s| s.key().to_string()).collect();
        (pick(&self.hidden), pick(&self.collapsed))
    }

    /// 从记忆还原;`collapsed` 为 `None`(从没记录过)用默认折叠。
    pub fn from_keys(hidden: &[String], collapsed: Option<&[String]>) -> Self {
        let mut s = Self::default();
        let has = |keys: &[String], sec: Section| keys.iter().any(|k| k == sec.key());
        for sec in Section::ALL {
            s.hidden[sec.ix()] = has(hidden, sec);
            if let Some(c) = collapsed {
                s.collapsed[sec.ix()] = has(c, sec);
            }
        }
        s
    }
}

// ───────────────────────── 打开的编辑器 ─────────────────────────

/// 「打开的编辑器」:编辑器里所有标签的列表,点击切换,悬停显示关闭按钮,未保存的有圆点。
pub struct OpenEditorsView {
    editor: Entity<IdeView>,
    _sub: Subscription,
}

/// 列表最多显示的行数(超过后内部滚动)。
const OPEN_ROWS_MAX: f32 = 8.;
const ROW_H: f32 = 22.;

impl OpenEditorsView {
    pub fn new(editor: Entity<IdeView>, cx: &mut Context<Self>) -> Self {
        let sub = cx.observe(&editor, |_, _, cx| cx.notify());
        Self { editor, _sub: sub }
    }
}

impl Render for OpenEditorsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let ed = self.editor.read(cx);
        let root = ed.root.clone();
        let active = ed.tabs.active().map(Path::to_path_buf);
        let rows: Vec<(PathBuf, bool, bool)> = ed.tabs.tabs().iter().map(|p| (p.clone(), ed.docs.get(p).is_some_and(|d| !d.saved), ed.tabs.is_preview(p))).collect();
        if rows.is_empty() {
            return div().px_3().py_2().text_size(px(12.)).text_color(t.muted_foreground).child("没有打开的文件").into_any_element();
        }
        let editor = self.editor.clone();
        v_flex().id("open-editors-scroll").max_h(px(ROW_H * OPEN_ROWS_MAX + 4.)).overflow_y_scroll().py_0p5()
            .children(rows.into_iter().enumerate().map(|(i, (path, dirty, preview))| {
                let is_diff = crate::ide_diff::is_diff(&path);
                let name = if is_diff { crate::ide_diff::tab_title(&path) } else { path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default() };
                // 文件所在目录(相对项目根),帮助区分同名文件
                let dir = if is_diff { String::new() } else { path.parent().and_then(|d| d.strip_prefix(&root).ok()).map(|d| d.display().to_string()).unwrap_or_default() };
                let on = active.as_deref() == Some(path.as_path());
                let group = SharedString::from(format!("oe-g-{i}"));
                let (p_open, p_close, ed_open, ed_close) = (path.clone(), path.clone(), editor.clone(), editor.clone());
                h_flex().id(SharedString::from(format!("oe-{i}"))).group(group.clone()).h(px(ROW_H)).flex_none().mx_1().pl_1().pr_1().gap_1p5().items_center().rounded(px(5.)).cursor_pointer()
                    .when(on, |d| d.bg(t.list_active))
                    .hover(|d| d.bg(t.list_hover))
                    .on_click(move |_, window, cx| {
                        let p = p_open.clone();
                        ed_open.update(cx, |v, cx| { v.activate(&p, window, cx); });
                    })
                    .child(
                        div().id(SharedString::from(format!("oe-x-{i}"))).size(px(16.)).flex_none().flex().items_center().justify_center().rounded(px(4.)).hover(|d| d.bg(t.border))
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                let p = p_close.clone();
                                ed_close.update(cx, |v, cx| v.close_tab(&p, window, cx));
                            })
                            .child(if dirty {
                                div().size(px(8.)).rounded_full().bg(t.foreground).into_any_element()
                            } else {
                                div().when(!on, |d| d.invisible().group_hover(group.clone(), |s| s.visible())).child(icon(IconName::X, 11., t.muted_foreground)).into_any_element()
                            }),
                    )
                    .child(if is_diff { icon(IconName::FileDiff, 14., t.muted_foreground).into_any_element() } else { crate::file_icons::file_icon_img(&name, false, false, 14.).into_any_element() })
                    .child(div().min_w_0().truncate().text_size(px(12.5)).when(preview, |d| d.italic()).child(name))
                    .when(!dir.is_empty(), |d| d.child(div().flex_1().min_w_0().truncate().text_size(px(11.)).text_color(t.muted_foreground).child(dir)))
            }))
            .into_any_element()
    }
}

// ───────────────────────── 时间线 ─────────────────────────

/// 时间线的数据状态。
enum TlState {
    /// 没有打开的文件 / 打开的是差异标签
    Empty,
    Loading,
    /// 文件不在 Git 仓库里
    NotGit,
    Ready { top: String, rel: String, commits: Vec<Commit> },
}

/// 「时间线」:当前文件的 Git 提交历史(`git log --follow`),点击在编辑区打开该次提交里这个文件的左右对比。
pub struct TimelineView {
    editor: Entity<IdeView>,
    scm: Entity<ScmView>,
    /// 当前列出的文件
    path: Option<PathBuf>,
    state: TlState,
    seq: u64,
    /// 区块展开着才读 Git(折叠 / 隐藏时切标签不要白起 git 进程)
    enabled: bool,
    _subs: Vec<Subscription>,
}

/// 最多读取的提交数。
const TIMELINE_LIMIT: usize = 100;

impl TimelineView {
    pub fn new(editor: Entity<IdeView>, scm: Entity<ScmView>, cx: &mut Context<Self>) -> Self {
        // 编辑器 notify(切标签 / 光标移动 / 保存)时看当前文件有没有变;没变什么都不做
        let sub = cx.observe(&editor, |this, _, cx| this.sync_path(cx));
        // 提交 / 暂存 / 切分支后(SCM 刷新)重新读取
        let scm_sub = cx.subscribe(&scm, |this, _, ev: &crate::scm::ScmEvent, cx| {
            if matches!(ev, crate::scm::ScmEvent::Changed) {
                this.reload(cx);
            }
        });
        let mut me = Self { editor, scm, path: None, state: TlState::Empty, seq: 0, enabled: false, _subs: vec![sub, scm_sub] };
        me.sync_path(cx);
        me
    }

    fn current_path(&self, cx: &App) -> Option<PathBuf> {
        self.editor.read(cx).tabs.active().filter(|p| !crate::ide_diff::is_diff(p)).map(Path::to_path_buf)
    }

    fn sync_path(&mut self, cx: &mut Context<Self>) {
        let now = self.current_path(cx);
        if now != self.path {
            self.path = now;
            self.reload(cx);
        }
    }

    /// 区块展开 / 收起:展开时(重新)读取当前文件的历史。
    pub fn set_enabled(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.enabled != on {
            self.enabled = on;
            if on {
                self.reload(cx);
            }
        }
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        if !self.enabled {
            return;
        }
        self.seq += 1;
        let g = self.seq;
        let Some(path) = self.path.clone() else {
            self.state = TlState::Empty;
            cx.notify();
            return;
        };
        self.state = TlState::Loading;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let res = cx
                .background_spawn(async move {
                    let dir = path.parent()?.to_string_lossy().to_string();
                    let name = path.file_name()?.to_string_lossy().to_string();
                    // 一次问出仓库根与文件所在目录相对仓库根的前缀
                    let out = loom_core::git::git(&dir, &["rev-parse", "--show-toplevel", "--show-prefix"]).ok()?;
                    let mut it = out.lines();
                    let top = it.next()?.trim().to_string();
                    let prefix = it.next().unwrap_or("").trim().to_string();
                    let rel = format!("{prefix}{name}");
                    let commits = git_log::log(&top, TIMELINE_LIMIT, Some(&rel), false).unwrap_or_default();
                    Some((top, rel, commits))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.seq != g {
                    return;
                }
                this.state = match res {
                    Some((top, rel, commits)) => TlState::Ready { top, rel, commits },
                    None => TlState::NotGit,
                };
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for TimelineView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let hint = |text: &str| div().p_3().text_size(px(12.)).text_color(t.muted_foreground).child(text.to_string()).into_any_element();
        match &self.state {
            TlState::Empty => hint("没有打开的文件"),
            TlState::Loading => hint("正在读取…"),
            TlState::NotGit => hint("文件不在 Git 仓库里"),
            TlState::Ready { commits, .. } if commits.is_empty() => hint("这个文件还没有提交记录"),
            TlState::Ready { top, rel, commits } => {
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
                let (top, rel, scm) = (top.clone(), rel.clone(), self.scm.clone());
                v_flex().id("timeline-scroll").size_full().overflow_y_scroll().py_1()
                    .children(commits.iter().enumerate().map(|(i, c)| {
                        let (top, rel, scm) = (top.clone(), rel.clone(), scm.clone());
                        let (hash, parent) = (c.hash.clone(), c.parents.first().cloned());
                        v_flex().id(SharedString::from(format!("tl-{i}"))).flex_none().mx_1().px_2().py_1().gap_0p5().rounded(px(5.)).cursor_pointer()
                            .hover(|d| d.bg(t.list_hover))
                            .on_click(move |ev: &ClickEvent, _, cx| {
                                let (top, rel, hash, parent) = (top.clone(), rel.clone(), hash.clone(), parent.clone());
                                let pinned = ev.click_count() >= 2;
                                scm.update(cx, |s, cx| s.open_rev_diff(top, rel, &hash, parent, pinned, cx));
                            })
                            .child(div().min_w_0().truncate().text_size(px(12.5)).child(c.subject.clone()))
                            .child(
                                h_flex().gap_1p5().items_center().text_size(px(11.)).text_color(t.muted_foreground)
                                    .child(icon(IconName::GitCommitHorizontal, 11., t.muted_foreground))
                                    .child(div().min_w_0().truncate().child(format!("{} · {}", c.author, git_log::relative_time(c.time, now))))
                                    .child(div().flex_none().font_family(crate::ui::MONO).child(c.short().to_string())),
                            )
                    }))
                    .into_any_element()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 默认时间线折叠_其余展开() {
        let s = SectionState::default();
        assert!(s.open(Section::Folder) && s.open(Section::Outline) && s.open(Section::OpenEditors));
        assert!(s.visible(Section::Timeline) && !s.open(Section::Timeline));
    }

    #[::core::prelude::v1::test]
    fn 隐藏再显示会展开() {
        let mut s = SectionState::default();
        s.toggle_hidden(Section::Timeline);
        assert!(!s.visible(Section::Timeline));
        s.toggle_hidden(Section::Timeline);
        assert!(s.open(Section::Timeline));
        s.toggle_hidden(Section::OpenEditors);
        assert_eq!(s.first_visible(), Some(Section::Folder));
    }

    #[::core::prelude::v1::test]
    fn 记忆往返_没有记录用默认() {
        let mut s = SectionState::default();
        s.toggle_hidden(Section::Outline);
        s.toggle_collapsed(Section::Folder);
        let (h, c) = s.to_keys();
        assert_eq!(SectionState::from_keys(&h, Some(&c)), s);
        let fresh = SectionState::from_keys(&[], None);
        assert_eq!(fresh, SectionState::default());
    }
}
