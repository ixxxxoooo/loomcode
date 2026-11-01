//! 编辑器正文的右键菜单(对标 VS Code 的顺序与措辞):
//! 转到定义 / 转到类型定义 → 格式化文档 → 剪切 / 复制 / 粘贴 → 命令面板。
//! 没有语言服务器或服务器不支持的项显示为灰色;剪切 / 复制在没有选区时灰色。
//!
//! @author ixxxxoooo

use crate::ide::{FormatDocument, GoToTypeDefinition, IdeView};
use gpui_kit::component::input::{Copy, Cut, EditorState, GoToDefinition, Paste};
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::*;

/// 构造右键菜单的闭包(每次弹出时按当前状态决定哪些项可用)。
///
/// 参数:`editor` 当前文档的编辑器;`view` 所在视图(查语言服务器能力)。
pub fn editor_menu(editor: &Entity<EditorState>, view: WeakEntity<IdeView>) -> impl Fn(NativeMenu, &mut Window, &mut App) -> NativeMenu + 'static {
    let editor = editor.clone();
    move |menu, _window, cx| {
        let has_selection = !editor.read(cx).selected_range().is_empty();
        let (has_def, has_type, has_format) = view
            .read_with(cx, |v, cx| (v.lsp_supports("definitionProvider", cx), v.lsp_supports("typeDefinitionProvider", cx), v.lsp_supports("documentFormattingProvider", cx)))
            .unwrap_or_default();
        menu.menu_with_disabled("转到定义", !has_def, Box::new(GoToDefinition))
            .menu_with_disabled("转到类型定义", !has_type, Box::new(GoToTypeDefinition))
            .separator()
            .menu_with_disabled("格式化文档", !has_format, Box::new(FormatDocument))
            .separator()
            .menu_with_disabled("剪切", !has_selection, Box::new(Cut))
            .menu_with_disabled("复制", !has_selection, Box::new(Copy))
            .menu("粘贴", Box::new(Paste))
    }
}
