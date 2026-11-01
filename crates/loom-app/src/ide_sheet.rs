//! IDE 里的表格文件(csv / tsv / xlsx / xls / ods):作为一种标签内容,与图片类似,不进文本文档表。
//! 分隔文本(csv / tsv)可以「以文本打开」改用代码编辑器,这个选择只在本次运行里记住。

use crate::ide::IdeView;
use crate::sheet_view::{SheetEvent, SheetView};
use gpui_kit::*;
use std::path::{Path, PathBuf};

impl IdeView {
    /// 这个路径是否要用表格视图打开(是表格文件,且没有被选成「以文本打开」)。
    pub(crate) fn wants_sheet(&self, path: &Path) -> bool {
        crate::sheet_view::is_sheet(path) && !self.as_text.contains(path)
    }

    /// 打开表格标签(已打开则复用)。
    pub(crate) fn open_sheet(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        if self.sheets.contains_key(path) {
            return;
        }
        let p = path.to_path_buf();
        let view = cx.new(|cx| SheetView::new(p.clone(), window, cx));
        let key = p.clone();
        let sub = cx.subscribe_in(&view, window, move |this: &mut Self, _, ev: &SheetEvent, window, cx| match ev {
            SheetEvent::OpenAsText => this.open_as_text(&key, window, cx),
        });
        self.sheet_subs.insert(p.clone(), sub);
        self.sheets.insert(p, view);
    }

    /// 把一个分隔文本文件改成用代码编辑器打开(标签位置不变)。
    pub(crate) fn open_as_text(&mut self, path: &PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.as_text.insert(path.clone());
        self.sheets.remove(path);
        self.sheet_subs.remove(path);
        match crate::ide::load(path, window, cx) {
            Ok(doc) => {
                self.docs.insert(path.clone(), doc);
                self.refresh_marks(path, cx);
                self.focus_active(window, cx);
            }
            Err(note) => self.note = Some(note),
        }
        cx.notify();
    }

    /// 丢弃一个标签的表格视图。
    pub(crate) fn drop_sheet(&mut self, path: &Path) {
        self.sheets.remove(path);
        self.sheet_subs.remove(path);
    }
}
