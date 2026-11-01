//! 源代码管理视图里的「图形」区(对标 VS Code 的 Source Control Graph / Git Graph 扩展):
//! 提交历史 + 分支泳道图,点提交展开详情(完整消息、作者、改动的文件),点文件看该次提交里的 diff。
//!
//! 状态挂在 `ScmView.graph` 上(全工作台唯一),渲染在 `scm_graph_view.rs`。读取日志与计算 diff 都在后台线程;
//! 只在区块展开时才读取,历史没变(引用签名相同)时不重读。
//!
//! @author ixxxxoooo

use crate::diff;
use crate::scm::{FileDiff, ScmView};
use gpui_kit::*;
use loom_core::git_log::{self, CommitFile, GraphRow, Stash};
use std::collections::HashMap;

/// 每页读取的提交数。
pub const PAGE: usize = 150;
/// 最多读取多少提交(再多的历史没有浏览意义,且泳道布局会变慢)。
pub const MAX_COMMITS: usize = 3000;

/// 图形区状态。
pub struct GraphState {
    /// 区块是否展开
    pub open: bool,
    /// 显示所有分支(含远程与 tag);否则只看当前分支
    pub all: bool,
    pub rows: Vec<GraphRow>,
    /// 当前请求的条数
    pub limit: usize,
    /// 可能还有更早的提交
    pub has_more: bool,
    pub loading: bool,
    pub loaded: bool,
    pub error: Option<String>,
    /// 上次读取时的引用签名
    pub sig: Option<u64>,
    /// 展开了详情的提交
    pub expanded: Option<String>,
    /// 提交 → 改动的文件(`None` 表示还在读)
    pub files: HashMap<String, Option<Vec<CommitFile>>>,
    /// `<哈希>:<路径>` → 该文件在这次提交里的 diff(`None` 表示还在算)
    pub diffs: HashMap<String, Option<FileDiff>>,
    /// 储藏列表(「更多」菜单用)
    pub stashes: Vec<Stash>,
    /// 只看这个文件的历史(仓库内相对路径);`None` 看整个仓库
    pub file: Option<String>,
}

impl Default for GraphState {
    fn default() -> Self {
        Self { open: true, all: true, rows: Vec::new(), limit: PAGE, has_more: false, loading: false, loaded: false, error: None, sig: None, expanded: None, files: HashMap::new(), diffs: HashMap::new(), stashes: Vec::new(), file: None }
    }
}

impl GraphState {
    /// 清空数据(切换仓库时)。
    pub fn reset(&mut self) {
        *self = Self { open: self.open, all: self.all, file: self.file.clone(), ..Self::default() };
    }
}

impl ScmView {
    /// 显示某个文件的提交历史(右键「显示历史」):图形区只列改过它的提交,并展开图形区。
    ///
    /// 参数:`path` 文件绝对路径。文件不在当前仓库里时什么也不做。
    pub fn show_file_history(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        let Some(top) = self.top.clone() else { return };
        let Ok(rel) = path.strip_prefix(&top) else { return };
        self.graph.file = Some(rel.to_string_lossy().to_string());
        self.graph.open = true;
        self.graph.limit = PAGE;
        self.graph.sig = None;
        self.graph.loaded = false;
        self.graph.expanded = None;
        self.reload_graph(cx);
        cx.notify();
    }

    /// 展开 / 收起图形区;首次展开时读取。
    pub(crate) fn toggle_graph(&mut self, cx: &mut Context<Self>) {
        self.graph.open = !self.graph.open;
        if self.graph.open && !self.graph.loaded {
            self.reload_graph(cx);
        }
        cx.notify();
    }

    /// 切换「所有分支 / 仅当前分支」。
    pub(crate) fn toggle_graph_all(&mut self, cx: &mut Context<Self>) {
        self.graph.all = !self.graph.all;
        self.graph.limit = PAGE;
        self.graph.sig = None;
        self.reload_graph(cx);
    }

    /// 再多读一页。
    pub(crate) fn graph_more(&mut self, cx: &mut Context<Self>) {
        if !self.graph.has_more || self.graph.loading {
            return;
        }
        self.graph.limit = (self.graph.limit + PAGE).min(MAX_COMMITS);
        self.reload_graph(cx);
    }

    /// 状态刷新后调用:区块展开且引用签名变了才重读。`sig` 来自后台线程。
    pub(crate) fn graph_check(&mut self, sig: u64, cx: &mut Context<Self>) {
        if self.graph.open && (!self.graph.loaded || self.graph.sig != Some(sig)) {
            self.reload_graph(cx);
        }
    }

    /// 后台读取日志并计算泳道布局。
    pub(crate) fn reload_graph(&mut self, cx: &mut Context<Self>) {
        let Some(top) = self.top.clone() else { return };
        if self.graph.loading {
            return;
        }
        self.graph.loading = true;
        let (limit, all, file) = (self.graph.limit, self.graph.all, self.graph.file.clone());
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let sig = git_log::refs_signature(&top);
                    let commits = git_log::log(&top, limit, file.as_deref(), all)?;
                    let more = commits.len() >= limit && limit < MAX_COMMITS;
                    Ok::<_, String>((sig, git_log::layout(&commits), more))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.graph.loading = false;
                this.graph.loaded = true;
                match result {
                    Ok((sig, rows, more)) => {
                        this.graph.sig = Some(sig);
                        this.graph.rows = rows;
                        this.graph.has_more = more;
                        this.graph.error = None;
                        // 展开的提交已不在历史里(如被 reset 掉)就收起
                        if this.graph.expanded.as_ref().is_some_and(|h| !this.graph.rows.iter().any(|r| &r.commit.hash == h)) {
                            this.graph.expanded = None;
                        }
                    }
                    Err(e) => this.graph.error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 展开 / 收起某次提交的详情;展开时读取它改动的文件。
    pub(crate) fn toggle_commit(&mut self, hash: &str, cx: &mut Context<Self>) {
        if self.graph.expanded.as_deref() == Some(hash) {
            self.graph.expanded = None;
            cx.notify();
            return;
        }
        self.graph.expanded = Some(hash.to_string());
        if !self.graph.files.contains_key(hash) {
            if let Some(top) = self.top.clone() {
                self.graph.files.insert(hash.to_string(), None);
                let h = hash.to_string();
                cx.spawn(async move |this, cx| {
                    let hh = h.clone();
                    let files = cx.background_spawn(async move { git_log::commit_files(&top, &hh) }).await;
                    let _ = this.update(cx, |this, cx| {
                        this.graph.files.insert(h, Some(files.unwrap_or_default()));
                        cx.notify();
                    });
                })
                .detach();
            }
        }
        cx.notify();
    }

    /// 展开 / 收起某次提交里某个文件的 diff(该文件在提交前 → 提交后的内容)。
    pub(crate) fn toggle_commit_diff(&mut self, hash: &str, file: &CommitFile, cx: &mut Context<Self>) {
        let key = format!("{hash}:{}", file.path);
        if self.graph.diffs.remove(&key).is_some() {
            cx.notify();
            return;
        }
        let Some(top) = self.top.clone() else { return };
        let parent = self.graph.rows.iter().find(|r| r.commit.hash == hash).and_then(|r| r.commit.parents.first().cloned());
        let max_bytes = u64::from(crate::prefs(cx).scm.diff_max_kb) * 1024;
        self.graph.diffs.insert(key.clone(), None);
        let (hash, file) = (hash.to_string(), file.clone());
        cx.spawn(async move |this, cx| {
            let k = key.clone();
            let d = cx
                .background_spawn(async move {
                    let old_path = file.orig.as_deref().unwrap_or(&file.path);
                    let old = parent.as_deref().and_then(|p| git_log::file_at_rev(&top, p, old_path)).unwrap_or_default();
                    let new = git_log::file_at_rev(&top, &hash, &file.path).unwrap_or_default();
                    if (old.len() as u64) > max_bytes || (new.len() as u64) > max_bytes {
                        return (Vec::new(), 0, 0);
                    }
                    diff::compute(&old, &new)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(slot) = this.graph.diffs.get_mut(&k) {
                    *slot = Some(d);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// 复制提交哈希到剪贴板。
    pub(crate) fn copy_hash(&self, hash: &str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(hash.to_string()));
    }
}
