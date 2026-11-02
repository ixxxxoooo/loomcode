//! 右键菜单「比较」子菜单:与已保存文件比较、与剪贴板比较(结果以差异浮层显示)。
//!
//! @author ixxxxoooo

use crate::diff::compute;
use crate::ide::IdeView;
use crate::ide_overlay::Overlay;
use gpui_kit::*;

impl IdeView {
    /// 当前文档与磁盘上已保存的版本比较。
    pub(crate) fn compare_with_saved(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.tabs.active().map(|p| p.to_path_buf()) else { return };
        let Some(doc) = self.docs.get(&path) else { return };
        let current = doc.editor.read(cx).value().to_string();
        match std::fs::read_to_string(&path) {
            Ok(saved) => {
                let (lines, _, _) = compute(&saved, &current);
                let lang = crate::file_tree::language_for(&path);
                self.set_overlay(Overlay::Diff { title: "已保存 ↔ 当前(左:磁盘上的版本,右:编辑器里的内容)".into(), lines, lang }, cx);
            }
            Err(e) => crate::notice::error(window, cx, format!("读取已保存文件失败:{e}")),
        }
    }

    /// 剪贴板内容与当前选区(没有选区就是整个文档)比较。
    pub(crate) fn compare_with_clipboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.tabs.active().map(|p| p.to_path_buf()) else { return };
        let Some(doc) = self.docs.get(&path) else { return };
        let Some(clip) = cx.read_from_clipboard().and_then(|c| c.text()) else {
            crate::notice::info(window, cx, "剪贴板里没有文本");
            return;
        };
        let ed = doc.editor.read(cx);
        let (text, range) = (ed.value().to_string(), ed.selected_range());
        let (current, what) = if range.is_empty() { (text, "整个文档") } else { (text.get(range).unwrap_or("").to_string(), "选区") };
        let (lines, _, _) = compute(&clip, &current);
        let lang = crate::file_tree::language_for(&path);
        self.set_overlay(Overlay::Diff { title: format!("剪贴板 ↔ {what}"), lines, lang }, cx);
    }
}
