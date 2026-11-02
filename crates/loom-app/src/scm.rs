//! 源代码管理(对标 VS Code 的 Source Control 视图)的状态与操作。
//!
//! 全工作台只有一个 `ScmView` 实体(`Workbench.scm`):Agent 右栏的「Git 更改」与 IDE 活动栏的
//! 「源代码管理」都嵌入它,提交消息、展开状态、视图模式只有一份。界面在 `scm_view.rs`,
//! 纯分组逻辑在 `scm_model.rs`,Git 命令在 `loom-core::git_scm`。所有 git 调用都在后台线程里做。
//!
//! 注意:本实体不回读 `Workbench`(它常在工作台更新期间被调用),需要的信息都由宿主传入。
//!
//! @author ixxxxoooo

use crate::diff::{self, DiffLine};
use crate::scm_model::{self, Group, Groups};
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::input::{InputEvent, TextareaState};
use gpui_kit::*;
use loom_core::git::{self, StatusEntry};
use loom_core::git_log;
use loom_core::git_scm::{self, SyncInfo};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// 视图向宿主发出的事件。
#[derive(Debug, Clone)]
pub enum ScmEvent {
    /// 请在编辑器里打开这个文件(第二项为 true 表示固定为正式标签,否则是预览标签)
    OpenFile(PathBuf, bool),
    /// 仓库状态被改动(暂存 / 提交 / 放弃…),文件树的 Git 标记需要刷新
    Changed,
    /// 请在编辑区打开某文件的差异标签(第二项为 true 表示固定,否则是预览标签)
    OpenDiff(crate::ide_diff::DiffSource, bool),
    /// 点了顶部分支名:请宿主弹出分支选择器(只有宿主能放得下浮层时才会发)
    PickBranch,
}

impl EventEmitter<ScmEvent> for ScmView {}

/// 一个文件的 diff:(行, 新增数, 删除数)。
pub(crate) type FileDiff = (Vec<DiffLine>, usize, usize);

/// 提交后的追加动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum After {
    Nothing,
    Push,
    Sync,
}

pub struct ScmView {
    /// 当前项目根(可能在仓库子目录里)
    pub(crate) root: String,
    /// 当前查看的仓库根目录;不在仓库里为 `None`
    pub(crate) top: Option<String>,
    /// 项目里发现的所有仓库(项目所在仓库 + 子目录里的仓库,对标 VS Code 自动检测),多于一个时顶部出现切换条
    pub(crate) repos: Vec<String>,
    /// 用户选中的仓库(不在 `repos` 里时退回第一个)
    chosen: Option<String>,
    /// 上次扫描仓库的时间:每 30 秒或手动刷新时才重扫,避免每次轮询都遍历目录
    scanned: Option<Instant>,
    pub(crate) entries: Vec<StatusEntry>,
    pub(crate) groups: Groups,
    pub(crate) sync: SyncInfo,
    pub(crate) error: Option<String>,
    /// 至少成功或失败地读取过一次
    pub(crate) loaded: bool,
    loading: bool,
    /// 读取期间又有刷新请求:读完立刻再来一次
    again: bool,
    /// 正在执行的操作(提交中、同步中…),期间按钮置灰
    pub(crate) busy: Option<&'static str>,
    pub(crate) message: Entity<TextareaState>,
    /// 「修改上次提交」
    pub(crate) amend: bool,
    /// 目录树视图(否则平铺列表)
    pub(crate) tree: bool,
    /// 折叠的组与目录(键:`<组>:<目录>` 或 `g:<组>`)
    pub(crate) collapsed: HashSet<String>,
    /// 展开了 diff 的行(行键 → diff;`None` 表示还在算)
    pub(crate) open: HashMap<String, Option<FileDiff>>,
    pub(crate) selected: Option<String>,
    /// 「图形」区(提交历史与泳道图)
    pub(crate) graph: crate::scm_graph::GraphState,
    /// 生成提交说明用的运行环境(真实模式由工作台注入;演示模式为空,用启发式)
    pub(crate) ai: Option<crate::composer_send::RewriteCtx>,
    /// 正在生成提交说明
    pub(crate) generating: bool,
    /// 点文件行:true 在编辑器里打开(IDE,旁边就是编辑器),false 就地展开 diff(Agent 右栏)。由宿主面板在渲染时设置
    pub(crate) open_on_click: bool,
    /// 宿主支持分支选择器时为真:顶部分支名可点(宿主渲染时设置)
    pub(crate) pick_branch: bool,
    /// 上次被绘制的时间:可见时轮询更勤
    last_paint: Instant,
    _sub: Subscription,
}

/// `error` 里表示「不是仓库」的固定文案。
const NOT_REPO: &str = "当前目录不在 Git 仓库里";

impl ScmView {
    /// 创建。`root` 为空时先不读取。
    pub fn new(root: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let message = cx.new(|cx| TextareaState::new(window, cx).auto_grow(1, 8).placeholder("消息(⌘Enter 提交)").submit_on_enter(true));
        // Enter 换行,⌘Enter 提交(与 VS Code 一致)
        let sub = cx.subscribe_in(&message, window, |this: &mut Self, input, ev: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { shift: false, secondary } = ev {
                if *secondary {
                    this.commit(After::Nothing, window, cx);
                } else {
                    input.update(cx, |i, cx| i.insert("\n", window, cx));
                }
            }
        });
        let tree = !std::env::var("LOOM_SCM").is_ok_and(|v| v.contains("list"));
        let mut this = Self {
            root,
            top: None,
            repos: Vec::new(),
            chosen: None,
            scanned: None,
            entries: Vec::new(),
            groups: Groups::default(),
            sync: SyncInfo::default(),
            error: None,
            loaded: false,
            loading: false,
            again: false,
            busy: None,
            message,
            amend: false,
            tree,
            collapsed: HashSet::new(),
            open: HashMap::new(),
            selected: None,
            graph: crate::scm_graph::GraphState::default(),
            ai: None,
            generating: false,
            open_on_click: false,
            pick_branch: false,
            last_paint: Instant::now() - Duration::from_secs(3600),
            _sub: sub,
        };
        this.start_polling(cx);
        if !this.root.is_empty() {
            this.refresh(cx);
        }
        this
    }

    /// 切换项目根;根没变就什么都不做。
    pub fn set_root(&mut self, root: &str, cx: &mut Context<Self>) {
        if self.root == root {
            return;
        }
        self.root = root.to_string();
        self.top = None;
        self.repos.clear();
        self.chosen = None;
        self.scanned = None;
        self.entries.clear();
        self.groups = Groups::default();
        self.sync = SyncInfo::default();
        self.error = None;
        self.loaded = false;
        self.open.clear();
        self.selected = None;
        self.amend = false;
        self.graph.reset();
        self.refresh(cx);
        cx.notify();
    }

    /// 有改动的不同文件数(活动栏角标用)。
    pub fn count(&self) -> usize {
        self.groups.distinct()
    }

    /// 视图被绘制时调用(渲染里),用来判断是否可见。
    pub(crate) fn touch(&mut self) {
        self.last_paint = Instant::now();
    }

    /// 后台轮询:可见时每 3 秒刷新一次,不可见时每 15 秒一次(让角标保持准确)。
    fn start_polling(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let mut tick = 0u32;
            loop {
                cx.background_executor().timer(Duration::from_secs(3)).await;
                tick += 1;
                let alive = this.update(cx, |this, cx| {
                    let visible = this.last_paint.elapsed() < Duration::from_secs(4);
                    if (visible || tick % 5 == 0) && this.busy.is_none() && !this.root.is_empty() {
                        this.refresh(cx);
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// 重新扫描项目里的仓库并刷新(手动刷新、初始化仓库之后)。
    pub fn rescan(&mut self, cx: &mut Context<Self>) {
        self.scanned = None;
        self.refresh(cx);
    }

    /// 切换到另一个仓库。
    pub fn select_repo(&mut self, repo: &str, cx: &mut Context<Self>) {
        if self.top.as_deref() == Some(repo) {
            return;
        }
        self.chosen = Some(repo.to_string());
        self.top = Some(repo.to_string());
        self.entries.clear();
        self.groups = Groups::default();
        self.sync = SyncInfo::default();
        self.open.clear();
        self.selected = None;
        self.amend = false;
        self.graph.reset();
        self.refresh(cx);
        cx.emit(ScmEvent::Changed);
        cx.notify();
    }

    /// 当前目录不是 Git 仓库(而不是读取失败)。
    pub(crate) fn not_repo(&self) -> bool {
        self.error.as_deref() == Some(NOT_REPO)
    }

    /// 在当前目录初始化 Git 仓库(「还不是仓库」空状态里的按钮)。
    pub fn init_repo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy.is_some() || self.root.is_empty() {
            return;
        }
        self.busy = Some("初始化仓库");
        cx.notify();
        let root = self.root.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx.background_spawn(async move { git_scm::init(&root) }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = None;
                match result {
                    Ok(()) => crate::notice::success(window, cx, "已初始化 Git 仓库"),
                    Err(e) => {
                        loom_diag::warn!("源代码管理:初始化仓库失败:{e}");
                        crate::notice::error(window, cx, format!("初始化仓库失败:{e}"));
                    }
                }
                this.rescan(cx);
                cx.emit(ScmEvent::Changed);
                cx.notify();
            });
        })
        .detach();
    }

    /// 后台重新读取状态、分支与上游;已展开的文件会重新计算 diff。
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            self.again = true;
            return;
        }
        self.loading = true;
        let root = self.root.clone();
        let known = self.repos.clone();
        let chosen = self.chosen.clone();
        let rescan = self.scanned.is_none_or(|t| t.elapsed() > Duration::from_secs(30));
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let repos = if rescan { git::find_repos(&root, 2) } else { known };
                    let top = chosen.filter(|c| repos.contains(c)).or_else(|| repos.first().cloned()).ok_or_else(|| NOT_REPO.to_string())?;
                    let entries = git::status(&top)?;
                    let sync = git_scm::sync_info(&top);
                    let sig = git_log::refs_signature(&top);
                    let stashes = git_log::stashes(&top);
                    Ok::<_, String>((top, repos, entries, sync, sig, stashes))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                let first = !this.loaded;
                this.loaded = true;
                let mut changed = first;
                match result {
                    Ok((top, repos, entries, sync, sig, stashes)) => {
                        if rescan {
                            this.scanned = Some(Instant::now());
                        }
                        if this.repos != repos {
                            this.repos = repos;
                            changed = true;
                        }
                        if this.graph.stashes != stashes {
                            this.graph.stashes = stashes;
                            changed = true;
                        }
                        if this.entries != entries || this.sync != sync || this.top.as_deref() != Some(top.as_str()) || this.error.is_some() {
                            changed = true;
                        }
                        this.groups = scm_model::split(&entries);
                        this.entries = entries;
                        this.sync = sync;
                        this.top = Some(top);
                        this.error = None;
                        this.graph_check(sig, cx);
                        // 行消失了(已提交 / 已放弃)就收起它的 diff
                        let keys: HashSet<String> = [Group::Conflicts, Group::Staged, Group::Changes].iter().flat_map(|g| this.groups.of(*g).iter().map(|i| i.key())).collect();
                        this.open.retain(|k, _| keys.contains(k));
                        let reopen: Vec<String> = this.open.keys().cloned().collect();
                        for key in reopen {
                            this.load_diff(&key, false, cx);
                        }
                    }
                    Err(e) => {
                        if rescan {
                            this.scanned = Some(Instant::now());
                        }
                        this.repos.clear();
                        if this.error.as_deref() != Some(e.as_str()) {
                            changed = true;
                        }
                        this.top = None;
                        this.entries.clear();
                        this.groups = Groups::default();
                        this.open.clear();
                        this.error = Some(e);
                    }
                }
                if changed {
                    cx.notify();
                }
                if std::mem::take(&mut this.again) {
                    this.refresh(cx);
                }
            });
        })
        .detach();
    }

    fn item(&self, key: &str) -> Option<scm_model::Item> {
        [Group::Conflicts, Group::Staged, Group::Changes].iter().find_map(|g| self.groups.of(*g).iter().find(|i| i.key() == key).cloned())
    }

    /// 展开 / 收起某一行的 diff(点击文件行)。
    pub(crate) fn toggle_diff(&mut self, key: &str, cx: &mut Context<Self>) {
        self.selected = Some(key.to_string());
        if self.open.remove(key).is_none() {
            self.load_diff(key, true, cx);
        }
        cx.notify();
    }

    /// 后台计算某一行的 diff:暂存组是 HEAD → 暂存区,更改组是暂存区 → 工作区(未跟踪文件相对空文件)。
    ///
    /// 参数:`first` 为真时先放一个「正在计算」占位。
    fn load_diff(&mut self, key: &str, first: bool, cx: &mut Context<Self>) {
        let (Some(top), Some(item)) = (self.top.clone(), self.item(key)) else { return };
        let max_bytes = u64::from(crate::prefs(cx).scm.diff_max_kb) * 1024;
        if first {
            self.open.insert(key.to_string(), None);
        } else if !self.open.contains_key(key) {
            return;
        }
        let key = key.to_string();
        cx.spawn(async move |this, cx| {
            let k = key.clone();
            let diff = cx
                .background_spawn(async move {
                    let abs = PathBuf::from(&top).join(item.path());
                    if std::fs::metadata(&abs).is_ok_and(|m| m.len() > max_bytes) {
                        return (Vec::new(), 0, 0);
                    }
                    let head_path = item.entry.orig.as_deref().unwrap_or(item.path());
                    let (old, new) = match item.group {
                        Group::Staged => (git::file_at_head(&top, head_path).unwrap_or_default(), git_scm::file_at_index(&top, item.path()).unwrap_or_default()),
                        Group::Changes if item.entry.untracked() => (String::new(), std::fs::read_to_string(&abs).unwrap_or_default()),
                        Group::Changes => {
                            let base = git_scm::file_at_index(&top, item.path()).or_else(|| git::file_at_head(&top, head_path)).unwrap_or_default();
                            (base, std::fs::read_to_string(&abs).unwrap_or_default())
                        }
                        Group::Conflicts => (git::file_at_head(&top, head_path).unwrap_or_default(), std::fs::read_to_string(&abs).unwrap_or_default()),
                    };
                    diff::compute(&old, &new)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                // 期间可能已被收起,或结果没变
                if let Some(slot) = this.open.get_mut(&k) {
                    if slot.as_ref() != Some(&diff) {
                        *slot = Some(diff);
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    /// 调试用(`LOOM_DIFF`):打开第 `n` 个文件(暂存组在前)的差异标签。
    pub(crate) fn open_nth_diff(&self, n: usize, cx: &mut Context<Self>) {
        let keys: Vec<String> = self.groups.staged.iter().chain(self.groups.changes.iter()).map(|i| i.key()).collect();
        if let Some(k) = keys.get(n) {
            self.open_diff_tab(k, false, cx);
        }
    }

    /// 在编辑区打开某一行的差异标签(IDE 里单击 = 预览标签,双击 = 固定)。
    pub(crate) fn open_diff_tab(&self, key: &str, pinned: bool, cx: &mut Context<Self>) {
        let (Some(top), Some(item)) = (self.top.clone(), self.item(key)) else { return };
        let src = crate::ide_diff::DiffSource { top, rel: item.path().to_string(), orig: item.entry.orig.clone(), group: item.group, untracked: item.entry.untracked(), rev: None };
        cx.emit(ScmEvent::OpenDiff(src, pinned));
    }

    /// 在编辑区打开某次提交里某文件的差异标签(Git 图谱点文件:该文件在提交前 → 提交后)。
    pub(crate) fn open_commit_diff(&self, hash: &str, file: &loom_core::git_log::CommitFile, pinned: bool, cx: &mut Context<Self>) {
        let Some(top) = self.top.clone() else { return };
        let parent = self.graph.rows.iter().find(|r| r.commit.hash == hash).and_then(|r| r.commit.parents.first().cloned());
        let src = crate::ide_diff::DiffSource { top, rel: file.path.clone(), orig: file.orig.clone(), group: crate::scm_model::Group::Changes, untracked: false, rev: Some((hash.to_string(), parent)) };
        cx.emit(ScmEvent::OpenDiff(src, pinned));
    }

    /// 在编辑区打开某文件在某次提交里的左右对比(时间线点提交)。`parent` 为该提交的第一个父提交。
    pub(crate) fn open_rev_diff(&self, top: String, rel: String, hash: &str, parent: Option<String>, pinned: bool, cx: &mut Context<Self>) {
        let src = crate::ide_diff::DiffSource { top, rel, orig: None, group: crate::scm_model::Group::Changes, untracked: false, rev: Some((hash.to_string(), parent)) };
        cx.emit(ScmEvent::OpenDiff(src, pinned));
    }

    /// 在编辑器里打开某个文件(仓库内相对路径)。
    pub(crate) fn open_file(&self, rel: &str, pinned: bool, cx: &mut Context<Self>) {
        if let Some(top) = &self.top {
            cx.emit(ScmEvent::OpenFile(PathBuf::from(top).join(rel), pinned));
        }
    }

    /// 在后台执行一个会改动仓库的操作,完成后提示、刷新并通知宿主。
    ///
    /// 参数:`busy` 进行中的文案;`ok` 成功提示(`None` 不提示);`op` 在后台线程里跑,参数是仓库根与当前同步信息。
    fn run(&mut self, busy: &'static str, ok: Option<String>, window: &mut Window, cx: &mut Context<Self>, op: impl FnOnce(&str, &SyncInfo) -> Result<(), String> + Send + 'static) {
        self.run_then(busy, ok, window, cx, op, |_, _, _| {});
    }

    /// 同 [`Self::run`],结束后(无论成败)再在界面线程执行 `done`(提交后据此清空输入框)。
    fn run_then(&mut self, busy: &'static str, ok: Option<String>, window: &mut Window, cx: &mut Context<Self>, op: impl FnOnce(&str, &SyncInfo) -> Result<(), String> + Send + 'static, done: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static) {
        let Some(top) = self.top.clone() else { return };
        if self.busy.is_some() {
            return;
        }
        self.busy = Some(busy);
        let sync = self.sync.clone();
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx.background_spawn(async move { op(&top, &sync) }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = None;
                match result {
                    Ok(()) => {
                        if let Some(m) = ok {
                            crate::notice::success(window, cx, m);
                        }
                    }
                    Err(e) => {
                        loom_diag::warn!("源代码管理:{busy}失败:{e}");
                        crate::notice::error(window, cx, format!("{busy}失败:{e}"));
                    }
                }
                done(this, window, cx);
                this.refresh(cx);
                cx.emit(ScmEvent::Changed);
                cx.notify();
            });
        })
        .detach();
    }

    // ---------- 暂存 / 取消暂存 / 放弃 ----------

    /// 暂存若干文件。
    pub(crate) fn stage(&mut self, entries: Vec<StatusEntry>, window: &mut Window, cx: &mut Context<Self>) {
        let paths: Vec<String> = entries.into_iter().map(|e| e.path).collect();
        self.run("暂存", None, window, cx, move |top, _| git_scm::stage(top, &paths));
    }

    /// 取消暂存若干文件。
    pub(crate) fn unstage(&mut self, entries: Vec<StatusEntry>, window: &mut Window, cx: &mut Context<Self>) {
        let paths: Vec<String> = entries.into_iter().map(|e| e.path).collect();
        self.run("取消暂存", None, window, cx, move |top, sync| git_scm::unstage(top, &paths, sync.has_commits));
    }

    /// 暂存某组的全部文件(「更改」组用 `git add -A` 一把梭,冲突组只暂存冲突文件)。
    pub(crate) fn stage_group(&mut self, group: Group, window: &mut Window, cx: &mut Context<Self>) {
        match group {
            Group::Changes => self.run("暂存", None, window, cx, |top, _| git_scm::stage_all(top)),
            _ => {
                let entries = self.groups.of(group).iter().map(|i| i.entry.clone()).collect();
                self.stage(entries, window, cx)
            }
        }
    }

    /// 取消暂存全部。
    pub(crate) fn unstage_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run("取消暂存", None, window, cx, |top, sync| git_scm::unstage_all(top, sync.has_commits));
    }

    /// 放弃若干文件的工作区改动(先弹确认,不可恢复)。
    pub(crate) fn confirm_discard(&mut self, entries: Vec<StatusEntry>, window: &mut Window, cx: &mut Context<Self>) {
        if entries.is_empty() {
            return;
        }
        let untracked = entries.iter().filter(|e| e.untracked()).count();
        let what = match entries.as_slice() {
            [one] => format!("「{}」", one.path.rsplit('/').next().unwrap_or(&one.path)),
            many => format!("{} 个文件", many.len()),
        };
        let extra = if untracked > 0 { format!("其中 {untracked} 个是未跟踪的新文件,会被直接删除。") } else { String::new() };
        let weak = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (weak, entries) = (weak.clone(), entries.clone());
            alert
                .confirm()
                .title("放弃更改?")
                .description(format!("{what}的未暂存改动将被丢弃,无法恢复。{extra}"))
                .ok_text("放弃更改")
                .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    let entries = entries.clone();
                    let _ = weak.update(cx, |this, cx| this.run("放弃更改", None, window, cx, move |top, _| git_scm::discard(top, &entries)));
                    true
                })
        });
    }

    // ---------- 提交 / 同步 ----------

    /// 提交(⌘Enter 或主按钮)。没有暂存的更改但有未暂存的,先确认「暂存全部并提交」。
    pub fn commit(&mut self, after: After, window: &mut Window, cx: &mut Context<Self>) {
        if self.top.is_none() || self.busy.is_some() {
            return;
        }
        let message = self.message.read(cx).value().trim().to_string();
        if message.is_empty() && !self.amend {
            crate::notice::warning(window, cx, "请先输入提交消息");
            self.message.update(cx, |i, cx| i.focus(window, cx));
            return;
        }
        let (staged, changes) = (self.groups.staged.len(), self.groups.changes.len() + self.groups.conflicts.len());
        if staged == 0 && changes == 0 && !self.amend {
            crate::notice::warning(window, cx, "没有可提交的更改");
            return;
        }
        if staged == 0 && !self.amend {
            // 与 VS Code 一致:没有暂存任何文件时,询问是否暂存全部并直接提交
            let weak = cx.entity().downgrade();
            window.open_alert_dialog(cx, move |alert, _, _| {
                let weak = weak.clone();
                alert
                    .confirm()
                    .title("没有暂存的更改")
                    .description("是否暂存所有更改并直接提交?")
                    .ok_text("全部暂存并提交")
                    .on_ok(move |_, window, cx| {
                        let _ = weak.update(cx, |this, cx| this.do_commit(true, after, window, cx));
                        true
                    })
            });
            return;
        }
        self.do_commit(false, after, window, cx);
    }

    fn do_commit(&mut self, stage_all: bool, after: After, window: &mut Window, cx: &mut Context<Self>) {
        let message = self.message.read(cx).value().trim().to_string();
        let amend = self.amend;
        let busy = match after {
            After::Nothing => "提交",
            After::Push => "提交并推送",
            After::Sync => "提交并同步",
        };
        let ok = if amend { "已修改上次提交" } else { "提交成功" }.to_string();
        // 成功后清空输入框、退出「修改上次提交」;失败时保留消息,方便重试
        let input = self.message.clone();
        // 提交本身成功了就清空输入框(即便随后的推送失败,提交也已经生效)
        let committed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = committed.clone();
        self.run_then(
            busy,
            Some(ok),
            window,
            cx,
            move |top, _| {
                if stage_all {
                    git_scm::stage_all(top)?;
                }
                git_scm::commit(top, &message, amend)?;
                flag.store(true, std::sync::atomic::Ordering::Relaxed);
                let info = git_scm::sync_info(top);
                match after {
                    After::Nothing => Ok(()),
                    After::Push => git_scm::push(top, &info).map_err(|e| format!("提交已完成,但推送失败:{e}")),
                    After::Sync => git_scm::sync(top, &info).map_err(|e| format!("提交已完成,但同步失败:{e}")),
                }
            },
            move |this, window, cx| {
                if committed.load(std::sync::atomic::Ordering::Relaxed) {
                    this.amend = false;
                    input.update(cx, |i, cx| i.set_value("", window, cx));
                }
            },
        );
    }

    /// 勾选 / 取消「修改上次提交」:勾选时若输入框为空,回填上次的提交消息。
    pub(crate) fn toggle_amend(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.amend = !self.amend;
        cx.notify();
        if !self.amend || !self.message.read(cx).value().trim().is_empty() {
            return;
        }
        let Some(top) = self.top.clone() else { return };
        cx.spawn_in(window, async move |this, cx| {
            let last = cx.background_spawn(async move { git_scm::last_commit_message(&top) }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                if let Some(m) = last {
                    if this.amend && this.message.read(cx).value().trim().is_empty() {
                        this.message.update(cx, |i, cx| i.set_value(m, window, cx));
                    }
                }
            });
        })
        .detach();
    }

    /// 同步更改(先拉取再推送)。
    pub(crate) fn sync_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run("同步", Some("已同步".into()), window, cx, |top, sync| git_scm::sync(top, sync));
    }

    /// 推送(没有上游时发布分支)。
    pub(crate) fn push(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let publish = self.sync.upstream.is_none();
        self.run(if publish { "发布分支" } else { "推送" }, Some(if publish { "已发布分支" } else { "已推送" }.into()), window, cx, |top, sync| git_scm::push(top, sync));
    }

    /// 拉取并合并。
    pub(crate) fn pull(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run("拉取", Some("已拉取".into()), window, cx, |top, _| git_scm::pull(top));
    }

    /// 取回远程更新(不合并)。
    pub(crate) fn fetch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run("提取", Some("已提取".into()), window, cx, |top, _| git_scm::fetch(top));
    }

    /// 折叠 / 展开某个组或目录。
    pub(crate) fn toggle_collapsed(&mut self, key: String, cx: &mut Context<Self>) {
        if !self.collapsed.remove(&key) {
            self.collapsed.insert(key);
        }
        cx.notify();
    }

    /// 切换列表 / 目录树。
    pub(crate) fn toggle_tree(&mut self, cx: &mut Context<Self>) {
        self.tree = !self.tree;
        cx.notify();
    }

    // ---------- AI 生成提交说明 ----------

    /// 注入生成提交说明所需的运行环境(真实模式)。
    pub fn set_ai_ctx(&mut self, ctx: Option<crate::composer_send::RewriteCtx>) {
        self.ai = ctx;
    }

    /// 生成提交说明(对标 Cursor / VS Code Copilot 的 ✨ 按钮):
    /// 有暂存内容时只看暂存区,否则看全部改动;优先让小模型生成「标题 + 要点」,没有可用模型或失败时退回启发式。
    /// 生成期间用户改过输入框则丢弃结果,不覆盖用户的输入。
    pub(crate) fn generate_message(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(top) = self.top.clone() else { return };
        if self.generating || self.groups.distinct() == 0 {
            if self.groups.distinct() == 0 {
                crate::notice::warning(window, cx, "没有可生成说明的更改");
            }
            return;
        }
        self.generating = true;
        let before = self.message.read(cx).value().to_string();
        let untracked: Vec<String> = self.entries.iter().filter(|e| e.untracked()).map(|e| e.path.clone()).collect();
        let ai = self.ai.clone();
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let ctx = loom_core::commit_msg::collect(&top, &untracked);
                    if ctx.files.is_empty() {
                        return Err("没有读取到改动内容".to_string());
                    }
                    // 先试小模型,失败再启发式(把失败原因带回去提示一下)
                    let ai_result = match ai {
                        Some(a) => match loom_claude::find_claude(a.claude_path.as_deref()) {
                            Some(claude) => loom_claude::rewrite::generate_commit_message(&claude, &a.cwd, &a.env, &loom_core::commit_msg::prompt(&ctx)),
                            None => Err("找不到 claude 可执行文件".to_string()),
                        },
                        None => Err(String::new()),
                    };
                    match ai_result {
                        Ok(m) => Ok((m, None)),
                        Err(why) => Ok((loom_core::commit_msg::heuristic(&ctx), Some(why))),
                    }
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.generating = false;
                match result {
                    Ok((message, why)) => {
                        if this.message.read(cx).value() == before.as_str() && !message.is_empty() {
                            this.message.update(cx, |i, cx| i.set_value(message, window, cx));
                        }
                        if let Some(why) = why.filter(|w| !w.is_empty()) {
                            loom_diag::warn!("生成提交说明:模型不可用,已用启发式:{why}");
                            crate::notice::warning(window, cx, format!("模型生成失败,已按改动内容简单生成:{why}"));
                        }
                    }
                    Err(e) => crate::notice::error(window, cx, format!("生成提交说明失败:{e}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    // ---------- 储藏 ----------

    /// 储藏当前所有改动(含未跟踪文件)。
    pub(crate) fn stash_push(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.groups.distinct() == 0 {
            crate::notice::warning(window, cx, "没有可储藏的更改");
            return;
        }
        self.run("储藏", Some("已储藏更改".into()), window, cx, |top, _| git_log::stash_push(top, ""));
    }

    /// 弹出(应用并删除)某条储藏。
    pub(crate) fn stash_pop(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.run("弹出储藏", Some("已应用储藏".into()), window, cx, move |top, _| git_log::stash_pop(top, index));
    }

    /// 应用某条储藏(保留它)。
    pub(crate) fn stash_apply(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.run("应用储藏", Some("已应用储藏".into()), window, cx, move |top, _| git_log::stash_apply(top, index));
    }

    /// 删除某条储藏。
    pub(crate) fn stash_drop(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.run("删除储藏", Some("已删除储藏".into()), window, cx, move |top, _| git_log::stash_drop(top, index));
    }
}
