//! 编辑器正文的右键菜单,顺序与措辞对标 VS Code:
//! 转到定义 / 类型定义 / 实现 / 引用 → 重命名符号 / 更改所有匹配项 / 格式化文档 / 重构 / 源代码操作 →
//! 剪切 / 复制 / 粘贴 → Git Blame 注释 / 显示历史 / 比较 ▸ → 自动换行 → 命令面板。
//! 需要语言服务器的项在没有服务器或服务器不支持时显示为灰色;剪切 / 复制在没有选区时灰色。
//!
//! @author ixxxxoooo

use crate::ide::*;
use gpui_kit::component::input::{Copy, Cut, EditorState, GoToDefinition, Paste};
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::*;

/// 构造右键菜单的闭包(每次弹出时按当前状态决定哪些项可用)。
///
/// 参数:`editor` 当前文档的编辑器;`view` 所在视图(查语言服务器能力与开关状态)。
pub fn editor_menu(editor: &Entity<EditorState>, view: WeakEntity<IdeView>) -> impl Fn(NativeMenu, &mut Window, &mut App) -> NativeMenu + 'static {
    let editor = editor.clone();
    move |menu, _window, cx| {
        let has_selection = !editor.read(cx).selected_range().is_empty();
        let (sup, blame) = view
            .read_with(cx, |v, cx| {
                let s = |cap: &str| v.lsp_supports(cap, cx);
                ([s("definitionProvider"), s("typeDefinitionProvider"), s("implementationProvider"), s("referencesProvider"), s("renameProvider"), s("documentFormattingProvider"), s("codeActionProvider")], v.blame_gutter)
            })
            .unwrap_or_default();
        let [def, ty, imp, refs, rename, format, actions] = sup;
        let compare = NativeMenu::new()
            .menu("与已保存文件比较", Box::new(CompareWithSaved))
            .menu("与剪贴板比较", Box::new(CompareWithClipboard));
        menu.menu_with_disabled("转到定义", !def, Box::new(GoToDefinition))
            .menu_with_disabled("转到类型定义", !ty, Box::new(GoToTypeDefinition))
            .menu_with_disabled("转到实现", !imp, Box::new(GoToImplementation))
            .menu_with_disabled("转到引用", !refs, Box::new(FindReferences))
            .separator()
            .menu_with_disabled("重命名符号", !rename, Box::new(RenameSymbol))
            .menu("更改所有匹配项", Box::new(ChangeAllOccurrences))
            .menu_with_disabled("格式化文档", !format, Box::new(FormatDocument))
            .menu_with_disabled("重构...", !actions, Box::new(ShowRefactor))
            .menu_with_disabled("源代码操作...", !actions, Box::new(ShowSourceActions))
            .separator()
            .menu_with_disabled("剪切", !has_selection, Box::new(Cut))
            .menu_with_disabled("复制", !has_selection, Box::new(Copy))
            .menu("粘贴", Box::new(Paste))
            .separator()
            .menu_with_check("用 Git Blame 注释", blame, Box::new(ToggleBlameAnnotations))
            .menu("显示历史", Box::new(ShowFileHistory))
            .submenu("比较", compare)
            .separator()
            .menu("自动换行", Box::new(ToggleWordWrap))
            .separator()
            .menu("命令面板...", Box::new(OpenCommandPalette))
    }
}

impl IdeView {
    /// 切换自动换行(写回全局偏好,所有编辑器一起变)。
    pub(crate) fn toggle_word_wrap(&mut self, cx: &mut Context<Self>) {
        if let Some(wb) = cx.try_global::<crate::WorkbenchRef>().and_then(|r| r.0.upgrade()) {
            let on = crate::prefs(cx).editor.soft_wrap;
            wb.update(cx, |wb, cx| wb.update_settings(cx, |s| s.editor.soft_wrap = !on));
        }
    }
}
