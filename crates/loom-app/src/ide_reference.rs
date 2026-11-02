//! ⌘L:把编辑器里选中的代码作为 `@文件#起-止` 引用附加到当前会话的输入框。
//!
//! 引用格式与 Claude Code 的 VS Code 扩展一致(`@app.ts#5-10`),由 claude 自己解析。
//!
//! @author ixxxxoooo

use crate::ide::IdeView;
use crate::workbench::Workbench;
use gpui_kit::component::RopeExt as _;
use gpui_kit::*;
use std::path::Path;

actions!(loomcode, [QuoteSelection]);

/// 选区换算成 1 起始的行范围 `(首行, 末行)`。
///
/// 参数:`start` `end` 是选区起止点的 (行, 列),均 0 起始,`start <= end`。
/// 整行选中时终点会落在下一行的第 0 列,这一行并没有被选中,所以不算进去。
pub fn selection_lines(start: (u32, u32), end: (u32, u32)) -> (u32, u32) {
    let first = start.0 + 1;
    let last = if end.1 == 0 && end.0 > start.0 { end.0 } else { end.0 + 1 };
    (first, last.max(first))
}

/// 拼出引用文本(不含开头的 `@`):`相对路径#首行-末行`;单行是 `相对路径#行`;没有行信息就只有路径。
///
/// 参数:`root` 项目根目录(用来算相对路径,不在根目录下就用绝对路径);`lines` 行范围。
pub fn format_reference(root: &str, path: &Path, lines: Option<(u32, u32)>) -> String {
    let shown = if root.is_empty() { path } else { path.strip_prefix(root).unwrap_or(path) };
    let base = shown.to_string_lossy();
    match lines {
        Some((a, b)) if a == b => format!("{base}#{a}"),
        Some((a, b)) => format!("{base}#{a}-{b}"),
        None => base.into_owned(),
    }
}

impl IdeView {
    /// 当前选中的文字:只取非空、不含换行、不超过 200 字符的单行选区,否则 `None`(用来预填搜索框)。
    pub fn selected_text(&self, cx: &App) -> Option<String> {
        let doc = self.tabs.active().and_then(|p| self.docs.get(p))?;
        let ed = doc.editor.read(cx);
        let range = ed.selected_range();
        if range.is_empty() {
            return None;
        }
        let text = ed.text().slice(range).to_string();
        (!text.contains('\n') && text.chars().count() <= 200 && !text.trim().is_empty()).then_some(text)
    }

    /// 选中一处匹配并滚到可见,不抢焦点(搜索结果点击用)。参数:行号(1 起始)、起止列(字符数,0 起始)。
    pub fn select_range(&mut self, line: u32, start: u32, end: u32, cx: &mut Context<Self>) {
        if let Some(c) = self.focused_child() {
            c.update(cx, |c, cx| c.select_range(line, start, end, cx));
            return;
        }
        let Some(doc) = self.tabs.active().and_then(|p| self.docs.get(p)) else { return };
        use gpui_kit::component::input::Position;
        doc.editor.update(cx, |e, cx| {
            let (a, b) = (e.text().position_to_offset(&Position::new(line.saturating_sub(1), start)), e.text().position_to_offset(&Position::new(line.saturating_sub(1), end)));
            e.set_selected_range(a..b, cx);
        });
        if let Some(path) = self.tabs.active().map(std::path::Path::to_path_buf) {
            self.record_jump(&path, line);
        }
    }

    /// 当前文件和选区对应的引用文本;没有打开文件返回 `None`,没有选中内容则只引用文件。
    pub fn selection_reference(&self, cx: &App) -> Option<String> {
        let path = self.tabs.active()?;
        let lines = self.docs.get(path).and_then(|doc| {
            let ed = doc.editor.read(cx);
            let range = ed.selected_range();
            if range.is_empty() {
                return None;
            }
            let (s, e) = (ed.text().offset_to_position(range.start), ed.text().offset_to_position(range.end));
            Some(selection_lines((s.line, s.character), (e.line, e.character)))
        });
        Some(format_reference(&self.root, path, lines))
    }
}

impl crate::ide_window::IdeWindow {
    /// ⌘L:把当前文件 / 选区作为引用追加到对话栏的输入框;对话栏收起时顺便展开。
    pub fn quote_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(reference) = self.ide.read(cx).selection_reference(cx) else { return };
        self.chat_open = true;
        self.wb.update(cx, |wb, cx| wb.insert_reference(&reference, window, cx));
        cx.notify();
    }
}

impl Workbench {
    /// 把引用追加到当前会话的输入框并聚焦;没有打开会话时忽略。
    pub fn insert_reference(&mut self, reference: &str, window: &mut Window, cx: &mut Context<Self>) {
        let composer = self.active.as_ref().and_then(|id| self.views.get(id)).map(|v| v.composer.clone());
        if let Some(c) = composer {
            c.update(cx, |c, cx| c.insert_reference(reference, window, cx));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 选区换算成行范围() {
        // 同一行内选中几个字符
        assert_eq!(selection_lines((4, 2), (4, 9)), (5, 5));
        // 跨行
        assert_eq!(selection_lines((4, 2), (7, 3)), (5, 8));
        // 整行选中:终点落在下一行第 0 列,不算那一行
        assert_eq!(selection_lines((4, 0), (7, 0)), (5, 7));
        // 空选区在行首(起止相同)不会出现 末行 < 首行
        assert_eq!(selection_lines((4, 0), (4, 0)), (5, 5));
    }

    #[::core::prelude::v1::test]
    fn 拼引用文本() {
        let p = Path::new("/proj/src/main.rs");
        assert_eq!(format_reference("/proj", p, Some((10, 20))), "src/main.rs#10-20");
        assert_eq!(format_reference("/proj", p, Some((7, 7))), "src/main.rs#7");
        assert_eq!(format_reference("/proj", p, None), "src/main.rs");
        // 不在根目录下用绝对路径;根为空也用绝对路径
        assert_eq!(format_reference("/other", p, None), "/proj/src/main.rs");
        assert_eq!(format_reference("", p, Some((1, 2))), "/proj/src/main.rs#1-2");
    }
}
