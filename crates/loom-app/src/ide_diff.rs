//! IDE 里的「差异」标签(对标 VS Code / Cursor 的源代码管理):在 SCM 里单击文件,直接在编辑区
//! 用左右对比(或统一视图,跟随偏好)显示该文件的改动;预览标签、双击固定,与普通文件标签同一套。
//!
//! 标签用伪路径 `diff:<组>:<绝对路径>` 标识(`file_name()` 仍是文件名),差异内容在后台算好后只显示
//! 改动块与前后各几行上下文,其余折叠成「已折叠 N 行未更改」。伪路径不会写进标签记忆。
//!
//! @author ixxxxoooo

use crate::diff::{DiffLine, compute};
use crate::ide::IdeView;
use crate::scm_model::Group;
use crate::ui::{MONO, icon};
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::{git, git_log, git_scm};
use similar::ChangeTag;
use std::path::{Path, PathBuf};

/// 改动块前后保留的上下文行数。
const CONTEXT: usize = 4;

/// 差异标签的来源:哪个仓库的哪个文件、哪一组。
#[derive(Debug, Clone, PartialEq)]
pub struct DiffSource {
    /// 仓库根
    pub top: String,
    /// 仓库内相对路径
    pub rel: String,
    /// 重命名前的路径
    pub orig: Option<String>,
    pub group: Group,
    pub untracked: bool,
    /// 看的是某次提交里的改动(Git 图谱点文件):`(提交哈希, 第一个父提交)`,此时 `group` 无意义
    pub rev: Option<(String, Option<String>)>,
}

impl DiffSource {
    fn abs(&self) -> PathBuf {
        PathBuf::from(&self.top).join(&self.rel)
    }

    /// 标签的伪路径。
    pub fn pseudo_path(&self) -> PathBuf {
        let key = match &self.rev {
            Some((hash, _)) => format!("rev-{hash}"),
            None => group_key(self.group).to_string(),
        };
        PathBuf::from(format!("diff:{key}:{}", self.abs().to_string_lossy()))
    }
}

fn group_key(g: Group) -> &'static str {
    match g {
        Group::Staged => "staged",
        Group::Changes => "changes",
        Group::Conflicts => "conflicts",
    }
}

/// 是不是差异标签的伪路径。
pub fn is_diff(path: &Path) -> bool {
    path.to_str().is_some_and(|s| s.starts_with("diff:"))
}

/// 伪路径对应的真实文件路径;不是伪路径原样返回。
pub fn real_path(path: &Path) -> PathBuf {
    let Some(rest) = path.to_str().and_then(|s| s.strip_prefix("diff:")) else { return path.to_path_buf() };
    PathBuf::from(rest.split_once(':').map_or(rest, |(_, p)| p))
}

/// 标签上显示的名字:`main.rs (更改)`。
pub fn tab_title(path: &Path) -> String {
    let name = real_path(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let tag = match path.to_str().and_then(|s| s.strip_prefix("diff:")).and_then(|s| s.split_once(':')).map(|(g, _)| g) {
        Some("staged") => "暂存".to_string(),
        Some("conflicts") => "冲突".to_string(),
        Some(g) if g.starts_with("rev-") => g["rev-".len()..].chars().take(7).collect(),
        _ => "更改".to_string(),
    };
    format!("{name} ({tag})")
}

/// 一个改动块:`hidden_before` 是它之前被折叠的未改动行数。
#[derive(Debug, Clone, PartialEq)]
pub struct Hunk {
    pub hidden_before: usize,
    pub lines: Vec<DiffLine>,
}

/// 把整份 Diff 切成「改动块 + 上下文」:离改动超过 `ctx` 行的未改动行折叠掉。
/// 返回 (改动块, 末尾被折叠的行数)。没有任何改动时块为空。
pub fn hunks(lines: &[DiffLine], ctx: usize) -> (Vec<Hunk>, usize) {
    let changed: Vec<usize> = lines.iter().enumerate().filter(|(_, l)| l.tag != ChangeTag::Equal).map(|(i, _)| i).collect();
    if changed.is_empty() {
        return (Vec::new(), 0);
    }
    // 每个改动行向两侧扩 ctx 行,合并重叠区间
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for i in changed {
        let (a, b) = (i.saturating_sub(ctx), (i + ctx + 1).min(lines.len()));
        match ranges.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => ranges.push((a, b)),
        }
    }
    let mut out = Vec::new();
    let mut prev_end = 0;
    for (a, b) in &ranges {
        out.push(Hunk { hidden_before: a - prev_end, lines: lines[*a..*b].to_vec() });
        prev_end = *b;
    }
    (out, lines.len() - prev_end)
}

/// 一个差异标签的状态。
pub struct DiffTab {
    pub src: DiffSource,
    /// 后台算好前为 `None`
    pub body: Option<DiffBody>,
}

/// 算好的差异。
pub struct DiffBody {
    pub hunks: Vec<Hunk>,
    pub hidden_tail: usize,
    pub added: usize,
    pub removed: usize,
    /// 文件太大或不是文本时的说明
    pub note: Option<String>,
}

/// 读出旧 / 新两份内容(与 SCM 面板里的行内 Diff 同一套口径):
/// 暂存组 = HEAD → 暂存区;更改组 = 暂存区 → 工作区(未跟踪相对空文件)。
fn load_sides(src: &DiffSource) -> (String, String) {
    if let Some((hash, parent)) = &src.rev {
        let old_path = src.orig.as_deref().unwrap_or(&src.rel);
        let old = parent.as_deref().and_then(|p| git_log::file_at_rev(&src.top, p, old_path)).unwrap_or_default();
        return (old, git_log::file_at_rev(&src.top, hash, &src.rel).unwrap_or_default());
    }
    let abs = src.abs();
    let head_path = src.orig.as_deref().unwrap_or(&src.rel);
    match src.group {
        Group::Staged => (git::file_at_head(&src.top, head_path).unwrap_or_default(), git_scm::file_at_index(&src.top, &src.rel).unwrap_or_default()),
        Group::Changes if src.untracked => (String::new(), std::fs::read_to_string(&abs).unwrap_or_default()),
        Group::Changes => {
            let base = git_scm::file_at_index(&src.top, &src.rel).or_else(|| git::file_at_head(&src.top, head_path)).unwrap_or_default();
            (base, std::fs::read_to_string(&abs).unwrap_or_default())
        }
        Group::Conflicts => (git::file_at_head(&src.top, head_path).unwrap_or_default(), std::fs::read_to_string(&abs).unwrap_or_default()),
    }
}

impl IdeView {
    /// 打开(或刷新)某个文件的差异标签;`preview` 为真时是预览标签(SCM 单击),否则固定。
    pub fn open_diff(&mut self, src: DiffSource, preview: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.note = None;
        let path = src.pseudo_path();
        self.diffs.insert(path.clone(), DiffTab { src, body: None });
        let out = self.tabs.open(&path, preview);
        if let Some(old) = out.replaced {
            self.docs.remove(&old);
            self.images.remove(&old);
            self.drop_sheet(&old);
            self.diffs.remove(&old);
        }
        self.focus_active(window, cx);
        self.load_diff(&path, cx);
        cx.notify();
    }

    /// 丢弃一个差异标签的状态。
    pub(crate) fn drop_diff(&mut self, path: &Path) {
        self.diffs.remove(path);
    }

    /// 后台读取并计算差异,完成后换进标签。
    fn load_diff(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(tab) = self.diffs.get(path) else { return };
        let src = tab.src.clone();
        let max_bytes = u64::from(crate::prefs(cx).scm.diff_max_kb) * 1024;
        let key = path.to_path_buf();
        cx.spawn(async move |this, cx| {
            let body = cx
                .background_spawn(async move {
                    let too_big = || DiffBody { hunks: Vec::new(), hidden_tail: 0, added: 0, removed: 0, note: Some("文件太大,没有显示差异(可在设置 → 源代码管理里调大上限)".into()) };
                    if src.rev.is_none() && std::fs::metadata(src.abs()).is_ok_and(|m| m.len() > max_bytes) {
                        return too_big();
                    }
                    let (old, new) = load_sides(&src);
                    if src.rev.is_some() && (old.len() as u64 > max_bytes || new.len() as u64 > max_bytes) {
                        return too_big();
                    }
                    let (lines, added, removed) = compute(&old, &new);
                    let (hunks, hidden_tail) = hunks(&lines, CONTEXT);
                    let note = hunks.is_empty().then(|| "没有可显示的文本差异(二进制、空文件,或内容相同)".to_string());
                    DiffBody { hunks, hidden_tail, added, removed, note }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(tab) = this.diffs.get_mut(&key) {
                    tab.body = Some(body);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// 差异标签的内容区:顶部信息条(文件、增删数、左右 / 统一切换)+ 可滚动的改动块。
    pub(crate) fn diff_view(&self, path: &Path, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let Some(tab) = self.diffs.get(path) else { return div().into_any_element() };
        let split = crate::prefs(cx).scm.diff_split;
        let lang = crate::file_tree::language_for(&tab.src.abs());
        let group_label = match (&tab.src.rev, tab.src.group) {
            (Some((hash, _)), _) => format!("提交 {}", hash.chars().take(7).collect::<String>()),
            (None, Group::Staged) => "暂存的更改".to_string(),
            (None, Group::Changes) => "更改".to_string(),
            (None, Group::Conflicts) => "合并更改".to_string(),
        };
        let stats = tab.body.as_ref().map(|b| (b.added, b.removed));
        let header = h_flex().h(px(30.)).w_full().flex_none().px_3().gap_2().items_center().border_b_1().border_color(t.border)
            .child(icon(IconName::FileDiff, 13., t.muted_foreground))
            .child(div().min_w_0().truncate().text_size(px(12.)).font_family(MONO).text_color(t.foreground).child(tab.src.rel.clone()))
            .child(div().flex_none().text_size(px(11.)).text_color(t.muted_foreground).child(group_label))
            .when_some(stats, |d, (a, r)| {
                d.child(h_flex().flex_none().gap_1().text_size(px(11.)).font_family(MONO)
                    .child(div().text_color(t.success).child(format!("+{a}")))
                    .child(div().text_color(t.danger).child(format!("−{r}"))))
            })
            .child(div().flex_1())
            .child(crate::diff_split::mode_bar(split, cx).w(px(150.)).border_b_0());
        let body: AnyElement = match &tab.body {
            None => div().p_4().text_size(px(12.)).text_color(t.muted_foreground).child("正在计算差异…").into_any_element(),
            Some(b) if b.note.is_some() => div().p_4().text_size(px(12.)).text_color(t.muted_foreground).child(b.note.clone().unwrap_or_default()).into_any_element(),
            Some(b) => {
                let sep = |n: usize| {
                    div().h(px(22.)).w_full().flex().items_center().justify_center().bg(t.muted.opacity(0.5)).text_size(px(11.)).text_color(t.muted_foreground)
                        .child(format!("⋯ 已折叠 {n} 行未更改"))
                };
                v_flex().w_full()
                    .children(b.hunks.iter().flat_map(|h| {
                        let part = if split { crate::diff_split::render_split(&h.lines, usize::MAX, &lang, cx) } else { crate::diff::render_with(&h.lines, usize::MAX, false, &lang, cx) };
                        [(h.hidden_before > 0).then(|| sep(h.hidden_before).into_any_element()), Some(part.into_any_element())]
                    }).flatten())
                    .when(b.hidden_tail > 0, |d| d.child(sep(b.hidden_tail)))
                    .into_any_element()
            }
        };
        v_flex().size_full().child(header)
            .child(div().id(SharedString::from(format!("diffv-{}", path.to_string_lossy()))).flex_1().min_h_0().overflow_y_scroll().child(body))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(old: &str, new: &str) -> Vec<DiffLine> {
        compute(old, new).0
    }

    #[::core::prelude::v1::test]
    fn 伪路径往返与标题() {
        let src = DiffSource { top: "/r".into(), rel: "src/a.rs".into(), orig: None, group: Group::Staged, untracked: false, rev: None };
        let p = src.pseudo_path();
        assert!(is_diff(&p));
        assert_eq!(real_path(&p), PathBuf::from("/r/src/a.rs"));
        assert_eq!(p.file_name().unwrap(), "a.rs");
        assert_eq!(tab_title(&p), "a.rs (暂存)");
        let c = DiffSource { rev: Some(("abcdef1234567".into(), None)), ..src };
        assert_eq!(tab_title(&c.pseudo_path()), "a.rs (abcdef1)");
        assert_eq!(real_path(&c.pseudo_path()), PathBuf::from("/r/src/a.rs"));
        assert_ne!(c.pseudo_path(), p);
        assert!(!is_diff(Path::new("/r/src/a.rs")));
        assert_eq!(real_path(Path::new("/r/src/a.rs")), PathBuf::from("/r/src/a.rs"));
    }

    #[::core::prelude::v1::test]
    fn 远离改动的行被折叠() {
        let old: String = (1..=30).map(|i| format!("l{i}\n")).collect();
        let new = old.replace("l15\n", "X\n");
        let (hs, tail) = hunks(&lines(&old, &new), 3);
        assert_eq!(hs.len(), 1);
        assert_eq!(hs[0].hidden_before, 11);
        // 3 行上下文 + 删 + 增 + 3 行上下文
        assert_eq!(hs[0].lines.len(), 8);
        assert_eq!(tail, 30 - 18);
    }

    #[::core::prelude::v1::test]
    fn 相邻改动合并_远的分开() {
        let old: String = (1..=40).map(|i| format!("l{i}\n")).collect();
        let new = old.replace("l5\n", "A\n").replace("l8\n", "B\n").replace("l35\n", "C\n");
        let (hs, _) = hunks(&lines(&old, &new), 2);
        assert_eq!(hs.len(), 2);
        assert!(hs[1].hidden_before > 0);
    }

    #[::core::prelude::v1::test]
    fn 没有改动时没有块() {
        let (hs, tail) = hunks(&lines("a\nb\n", "a\nb\n"), 3);
        assert!(hs.is_empty() && tail == 0);
    }
}
