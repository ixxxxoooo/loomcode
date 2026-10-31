//! 编辑器里的 Git 行标记:对比 HEAD,给新增 / 修改 / 删除处的行铺淡色底。
//! 编辑器没有独立的行号槽装饰接口,所以用「整行填充」的范围装饰代替(与 VSCode 的行内变更着色类似)。
//!
//! @author ixxxxoooo

use crate::git_status::GitMark;
use gpui_kit::component::input::{EditorState, RangeDecoration, RangeDecorationCollection, RangeDecorationStyle};
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;
use std::ops::Range;
use std::path::Path;
use std::process::Command;

/// 解析 `git diff -U0` 的输出,得到 (0 起始的行区间, 类型)。
/// 纯删除没有对应的新行,标在删除位置的下一行(越界时由调用方夹到末行)。
///
/// 参数:`diff` 为 `-U0` 的 unified diff 文本。
pub fn parse_hunks(diff: &str) -> Vec<(Range<usize>, GitMark)> {
    let mut out = Vec::new();
    for line in diff.lines().filter(|l| l.starts_with("@@")) {
        // @@ -a[,b] +c[,d] @@
        let Some(plus) = line.split_whitespace().nth(2).and_then(|s| s.strip_prefix('+')) else { continue };
        let minus = line.split_whitespace().nth(1).and_then(|s| s.strip_prefix('-')).unwrap_or("0");
        let count = |s: &str| s.split(',').nth(1).and_then(|n| n.parse::<usize>().ok()).unwrap_or(1);
        let start: usize = plus.split(',').next().and_then(|n| n.parse().ok()).unwrap_or(1);
        let (old_n, new_n) = (count(minus), count(plus));
        if new_n == 0 {
            out.push((start..start + 1, GitMark::Deleted));
        } else if old_n == 0 {
            out.push((start - 1..start - 1 + new_n, GitMark::Added));
        } else {
            out.push((start - 1..start - 1 + new_n, GitMark::Modified));
        }
    }
    out
}

/// 读取文件相对 HEAD 的改动;未跟踪文件和非仓库文件返回空。
pub fn read_hunks(path: &Path) -> Vec<(Range<usize>, GitMark)> {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else { return Vec::new() };
    let git = |args: &[&str]| Command::new("git").arg("-C").arg(dir).args(args).arg(name).output().ok().filter(|o| o.status.success());
    let tracked = git(&["ls-files", "--error-unmatch", "--"]).is_some();
    if !tracked {
        // 未跟踪的新文件不逐行标记(VSCode 同样只在资源管理器里标 U),整页着色只会淹没真正的改动
        return Vec::new();
    }
    git(&["diff", "-U0", "HEAD", "--"]).map(|o| parse_hunks(&String::from_utf8_lossy(&o.stdout))).unwrap_or_default()
}

/// 把 `hunks` 画到文档上:每个命中行铺一层淡色底。
///
/// 参数:`editor` 文档编辑器;`slot` 该文档持有的装饰集合(首次为 `None`);`hunks` 来自 [`read_hunks`]。
pub fn apply(editor: &Entity<EditorState>, slot: &mut Option<RangeDecorationCollection>, hunks: Vec<(Range<usize>, GitMark)>, cx: &mut App) {
    let text = editor.read(cx).value().to_string();
    let mut starts = vec![0usize];
    starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
    let lines = starts.len();
    let end_of = |ix: usize| starts.get(ix + 1).map_or(text.len(), |s| s - 1).max(starts[ix]);
    let t = cx.theme().clone();
    let decos: Vec<RangeDecoration> = hunks
        .into_iter()
        .flat_map(|(range, mark)| {
            let color = match mark {
                GitMark::Added => t.success,
                GitMark::Modified => t.warning,
                GitMark::Deleted => t.danger,
            };
            let is_deleted = mark == GitMark::Deleted;
            let last = lines - 1;
            let (from, to) = (range.start.min(last), range.end.min(lines));
            (from..to.max(from + 1)).map(move |ix| (ix, color, is_deleted)).collect::<Vec<_>>()
        })
        .map(|(ix, color, is_deleted)| RangeDecoration::new(starts[ix]..end_of(ix)).with_style(RangeDecorationStyle::Fill).with_color(color.opacity(if is_deleted { 0.10 } else { 0.13 })))
        .collect();
    match slot {
        Some(c) => c.set(decos, cx),
        None => *slot = Some(editor.update(cx, |e, cx| e.create_range_decorations_collection(decos, cx))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 解析新增修改删除() {
        let d = "@@ -3,0 +4,2 @@\n+a\n+b\n@@ -10 +12 @@\n-x\n+y\n@@ -20,2 +21,0 @@\n-p\n-q\n";
        let h = parse_hunks(d);
        assert_eq!(h[0], (3..5, GitMark::Added));
        assert_eq!(h[1], (11..12, GitMark::Modified));
        assert_eq!(h[2], (21..22, GitMark::Deleted));
    }

    #[::core::prelude::v1::test]
    fn 真实仓库里的改动与未跟踪文件() {
        let dir = std::env::temp_dir().join(format!("ucode-gutter-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sh = |args: &[&str]| {
            let ok = Command::new("git").arg("-C").arg(&dir).args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).output().unwrap().status.success();
            assert!(ok, "git {args:?}");
        };
        sh(&["init", "-q"]);
        std::fs::write(dir.join("a.txt"), "1\n2\n3\n").unwrap();
        sh(&["add", "."]);
        sh(&["commit", "-q", "-m", "x"]);
        std::fs::write(dir.join("a.txt"), "1\nB\n3\n4\n").unwrap();
        std::fs::write(dir.join("new.txt"), "x\ny\n").unwrap();
        let h = read_hunks(&dir.join("a.txt"));
        assert_eq!(h, vec![(1..2, GitMark::Modified), (3..4, GitMark::Added)]);
        assert!(read_hunks(&dir.join("new.txt")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
