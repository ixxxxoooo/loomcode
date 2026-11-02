//! 编辑器偏好落到 `EditorState`:新建文档时套用,设置变化时对所有已打开文档即时套用。
//!
//! @author ixxxxoooo

use gpui_kit::component::input::{EditorState, TabSize};
use gpui_kit::*;
use loom_config::EditorPrefs;

/// 把偏好套到一个编辑器上(行号、折叠、换行、缩进宽度)。
///
/// 参数:`ed` 编辑器状态;`p` 偏好。
pub fn apply(ed: &mut EditorState, p: &EditorPrefs, window: &mut Window, cx: &mut Context<EditorState>) {
    ed.set_line_number(p.line_numbers, window, cx);
    ed.set_folding(p.folding, window, cx);
    ed.set_soft_wrap(p.soft_wrap, window, cx);
    ed.set_tab_size(TabSize { tab_size: usize::from(p.tab_size.clamp(1, 16)), hard_tabs: false }, cx);
}

/// 统一的编辑器工厂:IDE 文档、设置里的文档编辑、Canvas 源码编辑都用它创建,
/// 因此行号 / 折叠 / 换行 / 缩进 / 语法高亮开关等偏好对所有编辑器一致。
///
/// 参数:`lang` 语言名;`text` 初始内容;`searchable` 是否带查找替换;`p` 偏好。返回:编辑器实体。
pub fn new_editor<T: 'static>(lang: &'static str, text: String, searchable: bool, p: &EditorPrefs, window: &mut Window, cx: &mut Context<T>) -> Entity<EditorState> {
    let ed = cx.new(|cx| EditorState::new(window, cx).language(language(lang, p)).searchable(searchable).default_value(text));
    ed.update(cx, |e, cx| {
        apply(e, p, window, cx);
        crate::ide_rainbow::install(e, cx);
    });
    ed
}

/// 语法高亮语言:关闭语法高亮时用纯文本。
pub fn language(path_lang: &'static str, p: &EditorPrefs) -> &'static str {
    if p.syntax_highlight { path_lang } else { "text" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 关闭语法高亮用纯文本() {
        let mut p = EditorPrefs::default();
        assert_eq!(language("rust", &p), "rust");
        p.syntax_highlight = false;
        assert_eq!(language("rust", &p), "text");
    }
}
