//! 编辑器的 VS Code 式行操作:切换行注释(⌘/)、上下移动行(⌥↑↓)、向上 / 向下复制行(⇧⌥↑↓)、删除行(⇧⌘K)、
//! 在下方 / 上方插入行(⌘↩ / ⇧⌘↩)、添加下一个匹配(⌘D)、选中所有匹配(⇧⌘L)。
//!
//! 文本变换都是纯函数(偏移按字节、行以 `\n` 分隔),界面部分只负责取选区、套用结果、还原选区。
//!
//! @author ixxxxoooo

use crate::ide::IdeView;
use gpui_kit::component::input::EditorState;
use gpui_kit::*;
use std::ops::Range;
use std::path::Path;

actions!(
    ide_edit,
    [ToggleLineComment, MoveLinesUp, MoveLinesDown, CopyLinesUp, CopyLinesDown, DeleteLines, InsertLineBelow, InsertLineAbove, AddNextOccurrence, SelectAllOccurrences]
);

/// 一次文本修改:把 `range` 换成 `text`,然后选中 `select`(都是修改后文本里的字节偏移)。
#[derive(Debug, PartialEq, Eq)]
pub struct Edit {
    pub range: Range<usize>,
    pub text: String,
    pub select: Range<usize>,
}

/// 按扩展名 / 文件名给出行注释符;没有行注释的语言(HTML、CSS、Markdown…)返回 `None`。
pub fn comment_prefix(path: &Path) -> Option<&'static str> {
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    match (name.as_str(), ext.as_str()) {
        ("dockerfile" | "makefile" | ".gitignore" | ".env", _) => Some("#"),
        (_, "rs" | "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" | "go" | "java" | "c" | "h" | "cc" | "cpp" | "hpp" | "cs" | "swift" | "kt" | "kts" | "scala" | "dart" | "php" | "zig" | "proto" | "sol" | "groovy" | "gradle" | "jsonc" | "json5") => Some("//"),
        (_, "py" | "sh" | "bash" | "zsh" | "rb" | "yaml" | "yml" | "toml" | "r" | "pl" | "ex" | "exs" | "mk" | "cmake" | "tf" | "nim" | "jl" | "conf") => Some("#"),
        (_, "sql" | "lua" | "hs" | "elm") => Some("--"),
        (_, "vim") => Some("\""),
        (_, "ini" | "cfg") => Some(";"),
        _ => None,
    }
}

/// 选区覆盖的整行范围(不含最后一行的换行)。选区非空且结束在某行行首时,那一行不算在内。
pub fn line_span(text: &str, sel: &Range<usize>) -> Range<usize> {
    let start = text[..sel.start].rfind('\n').map_or(0, |i| i + 1);
    let mut end_at = sel.end;
    if sel.end > sel.start && text[..sel.end].ends_with('\n') {
        end_at -= 1;
    }
    let end = text[end_at..].find('\n').map_or(text.len(), |i| end_at + i);
    start..end.max(start)
}

/// 切换行注释:所选行里的非空行都已注释 → 取消注释;否则在最小缩进处给非空行加注释。
pub fn toggle_comment(text: &str, sel: &Range<usize>, prefix: &str) -> Edit {
    let span = line_span(text, sel);
    let block = &text[span.clone()];
    let indent_of = |l: &str| l.len() - l.trim_start().len();
    let non_blank: Vec<&str> = block.split('\n').filter(|l| !l.trim().is_empty()).collect();
    let all_commented = !non_blank.is_empty() && non_blank.iter().all(|l| l.trim_start().starts_with(prefix));
    let min_indent = non_blank.iter().map(|l| indent_of(l)).min().unwrap_or(0);
    let new_lines: Vec<String> = block
        .split('\n')
        .map(|l| {
            if l.trim().is_empty() {
                return l.to_string();
            }
            if all_commented {
                let ind = indent_of(l);
                let rest = &l[ind..][prefix.len()..];
                format!("{}{}", &l[..ind], rest.strip_prefix(' ').unwrap_or(rest))
            } else {
                format!("{}{} {}", &l[..min_indent], prefix, &l[min_indent..])
            }
        })
        .collect();
    let new = new_lines.join("\n");
    let select = if sel.is_empty() {
        // 光标跟着所在行的长度变化移动(行内位置不变)
        let delta = new.len() as isize - block.len() as isize;
        let c = (sel.start as isize + delta).clamp(span.start as isize, (span.start + new.len()) as isize) as usize;
        c..c
    } else {
        span.start..span.start + new.len()
    };
    Edit { range: span, text: new, select }
}

/// 把所选行整体上移 / 下移一行;已在文件边界时返回 `None`。选区随之移动。
pub fn move_lines(text: &str, sel: &Range<usize>, up: bool) -> Option<Edit> {
    let span = line_span(text, sel);
    let block = &text[span.clone()];
    if up {
        if span.start == 0 {
            return None;
        }
        let prev_start = text[..span.start - 1].rfind('\n').map_or(0, |i| i + 1);
        let prev = &text[prev_start..span.start - 1];
        let shift = prev.len() + 1;
        Some(Edit { range: prev_start..span.end, text: format!("{block}\n{prev}"), select: sel.start - shift..sel.end - shift })
    } else {
        if span.end >= text.len() {
            return None;
        }
        let next_end = text[span.end + 1..].find('\n').map_or(text.len(), |i| span.end + 1 + i);
        let next = &text[span.end + 1..next_end];
        let shift = next.len() + 1;
        Some(Edit { range: span.start..next_end, text: format!("{next}\n{block}"), select: sel.start + shift..sel.end + shift })
    }
}

/// 复制所选行:`up` 为真时光标留在上面那份(原位置),否则落到下面那份。
pub fn copy_lines(text: &str, sel: &Range<usize>, up: bool) -> Edit {
    let span = line_span(text, sel);
    let block = &text[span.clone()];
    let shift = if up { 0 } else { block.len() + 1 };
    Edit { range: span, text: format!("{block}\n{block}"), select: sel.start + shift..sel.end + shift }
}

/// 删除所选行(连同一个换行符);删最后一行时带走前面的换行。
pub fn delete_lines(text: &str, sel: &Range<usize>) -> Edit {
    let span = line_span(text, sel);
    let range = if span.end < text.len() {
        span.start..span.end + 1
    } else if span.start > 0 {
        span.start - 1..span.end
    } else {
        span.clone()
    };
    let at = range.start;
    Edit { range, text: String::new(), select: at..at }
}

/// 在光标所在行下方(`below`)或上方插入一个空行,沿用该行的缩进,光标落在新行缩进之后。
pub fn insert_line(text: &str, cursor: usize, below: bool) -> Edit {
    let span = line_span(text, &(cursor..cursor));
    let line = &text[span.clone()];
    let indent = &line[..line.len() - line.trim_start().len()];
    if below {
        let at = span.end;
        Edit { range: at..at, text: format!("\n{indent}"), select: at + 1 + indent.len()..at + 1 + indent.len() }
    } else {
        let at = span.start;
        Edit { range: at..at, text: format!("{indent}\n"), select: at + indent.len()..at + indent.len() }
    }
}

/// 光标所在的单词范围(字母 / 数字 / 下划线);不在单词上返回 `None`。
pub fn word_at(text: &str, at: usize) -> Option<Range<usize>> {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let start = text[..at].char_indices().rev().take_while(|(_, c)| is_word(*c)).last().map_or(at, |(i, _)| i);
    let end = at + text[at..].char_indices().take_while(|(_, c)| is_word(*c)).last().map_or(0, |(i, c)| i + c.len_utf8());
    (start < end).then_some(start..end)
}

/// 在 `from` 之后(到结尾再回到开头)找下一个 `needle`;只有选中的这一处时返回 `None`。
pub fn next_occurrence(text: &str, needle: &str, from: usize, current: &Range<usize>) -> Option<Range<usize>> {
    if needle.is_empty() {
        return None;
    }
    let found = text[from..].find(needle).map(|i| from + i).or_else(|| text.find(needle));
    found.map(|s| s..s + needle.len()).filter(|r| r != current)
}

/// 文本里所有 `needle` 的位置(不重叠)。
pub fn all_occurrences(text: &str, needle: &str) -> Vec<Range<usize>> {
    if needle.is_empty() {
        return Vec::new();
    }
    text.match_indices(needle).map(|(i, m)| i..i + m.len()).collect()
}

/// 套用一次修改到编辑器:整块替换(一步撤销),再还原选区。
fn apply(ed: &mut EditorState, e: Edit, window: &mut Window, cx: &mut Context<EditorState>) {
    ed.set_selected_range(e.range, cx);
    ed.replace(e.text, window, cx);
    ed.set_selected_range(e.select, cx);
}

impl IdeView {
    /// 当前聚焦的文档编辑器;焦点在别处(查找框、重命名框)时返回 `None`,让按键交还给那个输入框。
    fn focused_doc_editor(&self, window: &Window, cx: &App) -> Option<(Entity<EditorState>, std::path::PathBuf)> {
        let path = self.tabs.active()?.to_path_buf();
        let doc = self.docs.get(&path)?;
        doc.editor.focus_handle(cx).contains_focused(window, cx).then(|| (doc.editor.clone(), path))
    }

    /// 在聚焦的文档编辑器上执行一次行操作:`f` 拿到全文与选区,返回要套用的修改。
    fn edit_lines(&mut self, window: &mut Window, cx: &mut Context<Self>, f: impl FnOnce(&str, &Range<usize>, &Path) -> Option<Edit>) {
        if let Some(c) = self.focused_child() {
            c.update(cx, |c, cx| c.edit_lines(window, cx, f));
            return;
        }
        let Some((ed, path)) = self.focused_doc_editor(window, cx) else {
            cx.propagate();
            return;
        };
        let (text, sel) = {
            let e = ed.read(cx);
            (e.value().to_string(), e.selected_range())
        };
        if let Some(edit) = f(&text, &sel, &path) {
            ed.update(cx, |e, cx| apply(e, edit, window, cx));
        }
    }

    pub(crate) fn toggle_line_comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut unsupported = false;
        self.edit_lines(window, cx, |text, sel, path| match comment_prefix(path) {
            Some(p) => Some(toggle_comment(text, sel, p)),
            None => {
                unsupported = true;
                None
            }
        });
        if unsupported {
            crate::notice::info(window, cx, "这种文件没有行注释语法");
        }
    }

    pub(crate) fn move_lines_action(&mut self, up: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.edit_lines(window, cx, |t, s, _| move_lines(t, s, up));
    }

    pub(crate) fn copy_lines_action(&mut self, up: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.edit_lines(window, cx, |t, s, _| Some(copy_lines(t, s, up)));
    }

    pub(crate) fn delete_lines_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.edit_lines(window, cx, |t, s, _| Some(delete_lines(t, s)));
    }

    pub(crate) fn insert_line_action(&mut self, below: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.edit_lines(window, cx, |t, s, _| Some(insert_line(t, s.end, below)));
    }

    /// ⌘D:没有选区先选中光标处的单词;已有选区则把下一处相同文字加为新的选区(到结尾后回到开头)。
    pub(crate) fn add_next_occurrence(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(c) = self.focused_child() {
            c.update(cx, |c, cx| c.add_next_occurrence(window, cx));
            return;
        }
        let Some((ed, _)) = self.focused_doc_editor(window, cx) else {
            cx.propagate();
            return;
        };
        ed.update(cx, |e, cx| {
            let (text, sel) = (e.value().to_string(), e.selected_range());
            if sel.is_empty() {
                if let Some(w) = word_at(&text, sel.start) {
                    e.set_selected_range(w, cx);
                }
            } else if let Some(r) = next_occurrence(&text, &text[sel.clone()], sel.end, &sel) {
                e.add_selection(r, cx);
            }
        });
    }

    /// ⇧⌘L:选中文件里所有与当前选区(或光标处单词)相同的文字。
    pub(crate) fn select_all_occurrences(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(c) = self.focused_child() {
            c.update(cx, |c, cx| c.select_all_occurrences(window, cx));
            return;
        }
        let Some((ed, _)) = self.focused_doc_editor(window, cx) else {
            cx.propagate();
            return;
        };
        ed.update(cx, |e, cx| {
            let (text, sel) = (e.value().to_string(), e.selected_range());
            let target = if sel.is_empty() { word_at(&text, sel.start) } else { Some(sel) };
            let Some(target) = target else { return };
            let all = all_occurrences(&text, &text[target]);
            let mut it = all.into_iter();
            if let Some(first) = it.next() {
                e.set_selected_range(first, cx);
                for r in it {
                    e.add_selection(r, cx);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, e: &Edit) -> String {
        let mut s = text.to_string();
        s.replace_range(e.range.clone(), &e.text);
        s
    }

    #[::core::prelude::v1::test]
    fn 行注释前缀() {
        assert_eq!(comment_prefix(Path::new("a/b.rs")), Some("//"));
        assert_eq!(comment_prefix(Path::new("x.py")), Some("#"));
        assert_eq!(comment_prefix(Path::new("Dockerfile")), Some("#"));
        assert_eq!(comment_prefix(Path::new("a.html")), None);
    }

    #[::core::prelude::v1::test]
    fn 选区覆盖的行_结束在行首不含下一行() {
        let t = "aa\nbb\ncc";
        assert_eq!(line_span(t, &(1..1)), 0..2);
        assert_eq!(line_span(t, &(1..4)), 0..5);
        assert_eq!(line_span(t, &(0..3)), 0..2);
        assert_eq!(line_span(t, &(8..8)), 6..8);
    }

    #[::core::prelude::v1::test]
    fn 注释与取消注释互逆_空行不动() {
        let t = "fn a() {\n    x;\n\n    y;\n}";
        let sel = 9..t.len() - 2;
        let c = toggle_comment(t, &sel, "//");
        let commented = run(t, &c);
        assert_eq!(commented, "fn a() {\n    // x;\n\n    // y;\n}");
        let u = toggle_comment(&commented, &(c.select.clone()), "//");
        assert_eq!(run(&commented, &u), t);
    }

    #[::core::prelude::v1::test]
    fn 混合状态先统一加注释() {
        let t = "// a\nb";
        let e = toggle_comment(t, &(0..t.len()), "//");
        assert_eq!(run(t, &e), "// // a\n// b");
    }

    #[::core::prelude::v1::test]
    fn 光标行注释后光标随之右移() {
        let t = "abc";
        let e = toggle_comment(t, &(1..1), "#");
        assert_eq!(run(t, &e), "# abc");
        assert_eq!(e.select, 3..3);
    }

    #[::core::prelude::v1::test]
    fn 上下移动行() {
        let t = "a\nb\nc";
        let e = move_lines(t, &(2..2), true).unwrap();
        assert_eq!((run(t, &e).as_str(), e.select), ("b\na\nc", 0..0));
        let e = move_lines(t, &(2..2), false).unwrap();
        assert_eq!((run(t, &e).as_str(), e.select), ("a\nc\nb", 4..4));
        assert!(move_lines(t, &(0..0), true).is_none());
        assert!(move_lines(t, &(4..4), false).is_none());
    }

    #[::core::prelude::v1::test]
    fn 复制行() {
        let t = "a\nb";
        let up = copy_lines(t, &(2..2), true);
        assert_eq!((run(t, &up).as_str(), up.select), ("a\nb\nb", 2..2));
        let down = copy_lines(t, &(2..2), false);
        assert_eq!((run(t, &down).as_str(), down.select), ("a\nb\nb", 4..4));
    }

    #[::core::prelude::v1::test]
    fn 删除行_含最后一行() {
        let t = "a\nb\nc";
        let e = delete_lines(t, &(2..2));
        assert_eq!(run(t, &e), "a\nc");
        let e = delete_lines(t, &(4..4));
        assert_eq!((run(t, &e).as_str(), e.select), ("a\nb", 3..3));
        let only = delete_lines("x", &(0..0));
        assert_eq!(run("x", &only), "");
    }

    #[::core::prelude::v1::test]
    fn 插入行沿用缩进() {
        let t = "fn f() {\n    x\n}";
        let e = insert_line(t, 12, true);
        assert_eq!(run(t, &e), "fn f() {\n    x\n    \n}");
        assert_eq!(e.select, 19..19);
        let e = insert_line(t, 12, false);
        assert_eq!(run(t, &e), "fn f() {\n    \n    x\n}");
        assert_eq!(e.select, 13..13);
    }

    #[::core::prelude::v1::test]
    fn 单词与匹配() {
        let t = "foo bar_1 foo";
        assert_eq!(word_at(t, 5), Some(4..9));
        assert_eq!(word_at(t, 3), Some(0..3));
        assert_eq!(word_at("a  b", 2), None);
        assert_eq!(next_occurrence(t, "foo", 3, &(0..3)), Some(10..13));
        assert_eq!(next_occurrence(t, "foo", 13, &(10..13)), Some(0..3));
        assert_eq!(next_occurrence("foo", "foo", 3, &(0..3)), None);
        assert_eq!(all_occurrences(t, "foo"), vec![0..3, 10..13]);
    }
}
