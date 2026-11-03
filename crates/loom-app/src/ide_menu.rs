//! 编辑器正文的右键菜单,顺序与措辞对标 VS Code:
//! 转到定义 / 类型定义 / 实现 / 引用 → 重命名符号 / 更改所有匹配项 / 格式化文档 / 重构 / 源代码操作 →
//! 剪切 / 复制 / 粘贴 → Git Blame 注释 / 显示历史 / 比较 ▸ → 自动换行 → 命令面板。
//! 需要语言服务器的项在没有服务器或服务器不支持时显示为灰色;剪切 / 复制在没有选区时灰色。
//!
//! @author ixxxxoooo

use crate::ide::*;
use gpui_kit::component::input::{Copy, Cut, EditorState, GoToDefinition, Paste};
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::*;

/// 构造右键菜单的闭包(每次弹出时按当前状态决定哪些项可用)。菜单是应用自己的主题化弹出菜单(跟随浅色 / 深色主题),
/// 每项右侧自动显示它的快捷键(按键绑定里有的就显示,如 F12、⇧F12、F2、⌥⇧F、⌘X)。
///
/// 参数:`editor` 当前文档的编辑器(动作发给它、判断有没有选区);`view` 所在视图(查语言服务器能力与开关状态)。
pub fn editor_menu(editor: &Entity<EditorState>, view: WeakEntity<IdeView>) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    let editor = editor.clone();
    move |menu, window, cx| {
        let focus = editor.read(cx).focus_handle(cx);
        let has_selection = !editor.read(cx).selected_range().is_empty();
        let (sup, blame) = view
            .upgrade()
            .map(|v| {
                let v = v.read(cx);
                let s = |cap: &str| v.lsp_supports(cap, cx);
                ([s("definitionProvider"), s("typeDefinitionProvider"), s("implementationProvider"), s("referencesProvider"), s("renameProvider"), s("documentFormattingProvider"), s("codeActionProvider")], v.blame_gutter)
            })
            .unwrap_or_default();
        let [def, ty, imp, refs, rename, format, actions] = sup;
        menu.action_context(focus)
            .min_w(px(220.))
            .menu_with_disabled("转到定义", Box::new(GoToDefinition), !def)
            .menu_with_disabled("转到类型定义", Box::new(GoToTypeDefinition), !ty)
            .menu_with_disabled("转到实现", Box::new(GoToImplementation), !imp)
            .menu_with_disabled("转到引用", Box::new(FindReferences), !refs)
            .separator()
            .menu_with_disabled("重命名符号", Box::new(RenameSymbol), !rename)
            .menu("更改所有匹配项", Box::new(ChangeAllOccurrences))
            .menu_with_disabled("格式化文档", Box::new(FormatDocument), !format)
            .menu_with_disabled("重构...", Box::new(ShowRefactor), !actions)
            .menu_with_disabled("源代码操作...", Box::new(ShowSourceActions), !actions)
            .separator()
            .menu_with_disabled("剪切", Box::new(Cut), !has_selection)
            .menu_with_disabled("复制", Box::new(Copy), !has_selection)
            .menu("粘贴", Box::new(Paste))
            .separator()
            .menu_with_check("用 Git Blame 注释", blame, Box::new(ToggleBlameAnnotations))
            .menu("显示历史", Box::new(ShowFileHistory))
            .submenu("比较", window, cx, |m, _, _| m.menu("与已保存文件比较", Box::new(CompareWithSaved)).menu("与剪贴板比较", Box::new(CompareWithClipboard)))
            .separator()
            .menu("自动换行", Box::new(ToggleWordWrap))
            .separator()
            .menu("命令面板...", Box::new(OpenCommandPalette))
    }
}

impl IdeView {
    /// 切换迷你地图(写回全局偏好,所有编辑器一起变;设置 → 编辑器里是同一个开关)。
    pub(crate) fn toggle_minimap(&mut self, cx: &mut Context<Self>) {
        if let Some(wb) = cx.try_global::<crate::WorkbenchRef>().and_then(|r| r.0.upgrade()) {
            let on = crate::prefs(cx).editor.minimap;
            wb.update(cx, |wb, cx| wb.update_settings(cx, |s| s.editor.minimap = !on));
        }
    }

    /// 切换自动换行(写回全局偏好,所有编辑器一起变)。
    pub(crate) fn toggle_word_wrap(&mut self, cx: &mut Context<Self>) {
        if let Some(wb) = cx.try_global::<crate::WorkbenchRef>().and_then(|r| r.0.upgrade()) {
            let on = crate::prefs(cx).editor.soft_wrap;
            wb.update(cx, |wb, cx| wb.update_settings(cx, |s| s.editor.soft_wrap = !on));
        }
    }
}
