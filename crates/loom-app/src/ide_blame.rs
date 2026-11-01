//! 编辑器里的 Git 追溯(blame):行内追溯(光标行末尾淡淡显示「作者, 时间 • 提交说明」,对标 Zed 的 inline blame
//! 与 GitLens 的当前行追溯)和追溯栏(行号左侧逐行显示作者与时间,同一提交的连续行只在首行显示)。
//!
//! 数据来自 `loom-core::git_blame`(`git blame --porcelain`),编辑中的未保存内容通过 `--contents -` 传入,
//! 所以改动中的行显示为「未提交」而不会错位。打开、保存时立即刷新,编辑后停顿 0.7 秒刷新;
//! 刷新期间(内容刚变、追溯还没算出来)行内追溯先隐藏,追溯栏变淡,避免显示错位的旧信息。
//!
//! @author ixxxxoooo

use crate::ide::{IdeDoc, IdeView};
use gpui_kit::component::{ActiveTheme, RopeExt as _, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::git_blame::{self, BlameCommit};
use loom_core::git_log::relative_time;
use std::cell::Cell;
use std::path::Path;
use std::sync::Arc;

/// 追溯栏宽度。
pub const GUTTER_W: f32 = 196.;

/// 追溯栏的界面状态:上次绘制的区域(把窗口坐标换算成栏内坐标)。
#[derive(Default)]
pub struct BlameUi {
    pub bounds: Cell<Bounds<Pixels>>,
}

/// 当前 Unix 秒。
fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// 行内追溯的文字:`作者, 3 天前 • 提交说明`;未提交的行显示「未提交的更改」。
pub fn inline_label(c: &BlameCommit, now: i64) -> String {
    if c.uncommitted() {
        return "未提交的更改".into();
    }
    format!("{}, {} • {}", c.author, relative_time(c.time, now), c.summary)
}

/// 悬停卡片的文字:说明、作者与邮箱、完整时间、短哈希。
pub fn tooltip_text(c: &BlameCommit, now: i64) -> String {
    if c.uncommitted() {
        return "未提交的更改\n这一行还没有被提交(或在编辑器里改过尚未保存)".into();
    }
    let when = loom_core::git_log::format_datetime(c.time, loom_diag::local_tz_offset());
    format!("{}\n{} <{}>\n{} ({})\n{}", c.summary, c.author, c.email, when, relative_time(c.time, now), c.short())
}

/// 追溯栏单行的文字:`作者  3 天前`。
pub fn gutter_label(c: &BlameCommit, now: i64) -> String {
    if c.uncommitted() { "未提交".into() } else { format!("{}  {}", c.author, relative_time(c.time, now)) }
}

impl IdeView {
    /// 重新计算某文档的追溯(后台线程)。文档有未保存修改时用编辑器内容代替磁盘文件。
    pub(crate) fn refresh_blame(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(doc) = self.docs.get_mut(path) else { return };
        doc.blame_gen += 1;
        let g = doc.blame_gen;
        let contents = (!doc.saved).then(|| doc.editor.read(cx).value().to_string());
        let path = path.to_path_buf();
        cx.spawn(async move |this, cx| {
            let p = path.clone();
            let lines = cx
                .background_spawn(async move {
                    // 在文件所在目录里按文件名追溯:不必换算仓库根(沙盒 / 符号链接下根路径的写法可能不同)
                    let dir = p.parent()?.to_string_lossy().to_string();
                    let name = p.file_name()?.to_string_lossy().to_string();
                    git_blame::blame(&dir, &name, contents.as_deref()).ok()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                // 期间又有编辑 / 又触发了刷新:丢弃这次结果
                if let Some(doc) = this.docs.get_mut(&path).filter(|d| d.blame_gen == g) {
                    doc.blame = Arc::new(lines.unwrap_or_default());
                    doc.blame_fresh = true;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// 编辑后 0.7 秒再刷新追溯(防抖);这期间把追溯标成「过期」。
    pub(crate) fn schedule_blame(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(doc) = self.docs.get_mut(path) else { return };
        doc.blame_fresh = false;
        doc.blame_gen += 1;
        let (path, g) = (path.to_path_buf(), doc.blame_gen);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(700)).await;
            let _ = this.update(cx, |this, cx| {
                if this.docs.get(&path).is_some_and(|d| d.blame_gen == g) {
                    this.refresh_blame(&path, cx);
                }
            });
        })
        .detach();
    }

    /// 切换追溯栏(面包屑栏按钮、命令面板)。
    pub fn toggle_blame_gutter(&mut self, cx: &mut Context<Self>) {
        self.blame_gutter = !self.blame_gutter;
        cx.notify();
    }
}

/// 光标所在行的行内追溯元素。条件不满足(没有追溯、正在选择、行不在视口里…)返回 `None`。
pub fn inline_blame(doc: &IdeDoc, cx: &App) -> Option<AnyElement> {
    if !crate::prefs(cx).editor.blame_inline || !doc.blame_fresh {
        return None;
    }
    let ed = doc.editor.read(cx);
    let sel = ed.selected_range();
    if sel.start != sel.end {
        return None;
    }
    let text = ed.text();
    let line = text.offset_to_position(sel.start).line as usize;
    let commit = doc.blame.get(line)?.commit.clone();
    let end = text.line_end_offset(line);
    // 行尾那个位置的窗口坐标(行不在视口里时为 None);自动换行时取最后一段的末尾
    let at = ed.range_to_bounds(&(end..end))?;
    let t = cx.theme();
    let label = inline_label(&commit, now());
    let color = if commit.uncommitted() { t.warning } else { t.muted_foreground };
    let lh = at.size.height;
    // 不越过编辑区右缘(否则会盖到迷你地图上);剩余空间太窄就不显示
    let left = at.origin.x + px(28.);
    let room = ed.text_bounds().map_or(px(560.), |b| b.right() - left - px(8.));
    if room < px(80.) {
        return None;
    }
    Some(
        deferred(
            anchored().position(point(left, at.origin.y)).anchor(Anchor::TopLeft).child(
                // 纯展示:浅灰文字,不响应悬停与点击(不挡住下面的编辑器操作)
                div().h(lh).max_w(room.min(px(560.))).flex().items_center().px_1().text_size(px(12.)).text_color(color.opacity(0.55))
                    .whitespace_nowrap().overflow_hidden()
                    .child(label),
            ),
        )
        .into_any_element(),
    )
}

/// 追溯栏元素:每个可见行一个绝对定位的标签,同一提交的连续行只在首行显示。
/// 自动换行时行与显示行不是一一对应,不显示追溯栏;没有追溯数据时返回 `None`。
pub fn gutter(doc: &IdeDoc, ui: &std::rc::Rc<BlameUi>, cx: &App) -> Option<AnyElement> {
    let p = crate::prefs(cx).editor;
    if p.soft_wrap || doc.blame.is_empty() {
        return None;
    }
    let t = cx.theme();
    let ed = doc.editor.read(cx);
    let visible = ed.visible_row_range()?;
    let text = ed.text();
    let origin = ui.bounds.get().origin;
    let stale = !doc.blame_fresh;
    let n = now();
    let mut labels: Vec<AnyElement> = Vec::new();
    for line in visible.start..visible.end.min(doc.blame.len()) {
        let Some(b) = ed.range_to_bounds(&{
            let s = text.line_start_offset(line);
            s..s
        }) else {
            continue;
        };
        let c = &doc.blame[line].commit;
        // 与上一行同一提交:只画色条,不重复文字(视口首行总是画出文字)
        let first = line == visible.start || line == 0 || !Arc::ptr_eq(&doc.blame[line - 1].commit, c);
        let color = if c.uncommitted() { t.warning } else { t.muted_foreground };
        let tip = tooltip_text(c, n);
        let hue = commit_hue(&c.hash);
        labels.push(
            h_flex().id(("ide-blame-line", line)).absolute().left_0().top(b.origin.y - origin.y).h(b.size.height).w_full().items_center().gap_1().pr_2()
                .child(div().w(px(2.)).h_full().flex_none().bg(if c.uncommitted() { t.warning } else { hsla(hue, 0.45, 0.55, 0.8) }))
                .when(first, |d| d.child(div().flex_1().min_w_0().truncate().text_size(px(11.)).text_color(color).child(gutter_label(c, n))))
                .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx))
                .into_any_element(),
        );
    }
    let ui_paint = ui.clone();
    Some(
        div().relative().w(px(GUTTER_W)).h_full().flex_none().overflow_hidden().border_r_1().border_color(t.border.opacity(0.6)).bg(t.sidebar.opacity(0.5))
            .when(stale, |d| d.opacity(0.45))
            .child(canvas(move |bounds, _, _| ui_paint.bounds.set(bounds), |_, _, _, _| {}).absolute().size_full())
            .children(labels)
            .into_any_element(),
    )
}

/// 由提交哈希得到稳定的色相(0..1),追溯栏左侧色条用,让同一提交的行一眼看出是一组。
fn commit_hue(hash: &str) -> f32 {
    let h = hash.bytes().take(8).fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(u32::from(b)));
    (h % 360) as f32 / 360.
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(summary: &str, hash: &str) -> BlameCommit {
        BlameCommit { hash: hash.into(), author: "张三".into(), email: "z@x.com".into(), time: 1_000_000, summary: summary.into() }
    }

    #[::core::prelude::v1::test]
    fn 行内文字含作者时间与说明() {
        let commit = c("feat: 初始", &"a".repeat(40));
        assert_eq!(inline_label(&commit, 1_000_000 + 3 * 86400), "张三, 3 天前 • feat: 初始");
        assert_eq!(gutter_label(&commit, 1_000_000 + 7200), "张三  2 小时前");
    }

    #[::core::prelude::v1::test]
    fn 未提交的行单独标注() {
        let commit = c("x", git_blame::ZERO_HASH);
        assert_eq!(inline_label(&commit, 0), "未提交的更改");
        assert_eq!(gutter_label(&commit, 0), "未提交");
        assert!(tooltip_text(&commit, 0).starts_with("未提交的更改"));
    }

    #[::core::prelude::v1::test]
    fn 悬停卡片含邮箱与短哈希() {
        let commit = c("fix: 修复", &"b".repeat(40));
        let tip = tooltip_text(&commit, 1_000_000 + 60);
        assert!(tip.contains("张三 <z@x.com>") && tip.contains("bbbbbbb") && tip.starts_with("fix: 修复"));
    }

    #[::core::prelude::v1::test]
    fn 色相稳定且在范围内() {
        let h = commit_hue(&"c".repeat(40));
        assert_eq!(h, commit_hue(&"c".repeat(40)));
        assert!((0. ..1.).contains(&h));
    }
}
