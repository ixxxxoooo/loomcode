//! IDE 标签的关闭流程:单个 / 其他 / 右侧,带未保存修改时先询问「保存并关闭 / 不保存 / 取消」。
//!
//! @author ixxxxoooo

use crate::ide::IdeView;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::*;
use std::path::{Path, PathBuf};

impl IdeView {
    /// 立即关闭一个标签并丢弃其文档。
    pub(crate) fn drop_tab(&mut self, path: &Path) {
        self.docs.remove(path);
        self.images.remove(path);
        self.drop_sheet(path);
        self.drop_diff(path);
        self.tabs.close(path);
    }

    /// 请求关闭一批标签:有未保存修改时先弹「保存 / 不保存 / 取消」,否则直接关闭。
    ///
    /// 参数:`paths` 要关闭的标签。
    pub fn request_close(&mut self, paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        let dirty: Vec<PathBuf> = paths.iter().filter(|p| self.docs.get(*p).is_some_and(|d| !d.saved)).cloned().collect();
        if dirty.is_empty() {
            self.close_now(&paths, window, cx);
            return;
        }
        let names: Vec<String> = dirty.iter().map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()).collect();
        let (view, list) = (cx.entity().downgrade(), names.join("、"));
        window.open_dialog(cx, move |dialog, _, _| {
            let (v1, v2, p1, p2, d1) = (view.clone(), view.clone(), paths.clone(), paths.clone(), dirty.clone());
            let footer = DialogFooter::new()
                .child(Button::new("close-cancel").label("取消").on_click(|_, window, cx| window.close_all_dialogs(cx)))
                .child(Button::new("close-discard").danger().label("不保存").on_click(move |_, window, cx| {
                    window.close_all_dialogs(cx);
                    let _ = v1.update(cx, |this, cx| this.close_now(&p1, window, cx));
                }))
                .child(Button::new("close-save").primary().label("保存并关闭").on_click(move |_, window, cx| {
                    window.close_all_dialogs(cx);
                    let _ = v2.update(cx, |this, cx| {
                        // 全部保存成功才关闭;失败的留在原处,错误显示在面包屑里
                        if d1.iter().all(|p| this.save_path(p, cx)) {
                            this.close_now(&p2, window, cx);
                        }
                    });
                }));
            dialog.title("保存修改?").child(format!("「{list}」有未保存的修改。")).footer(footer)
        });
    }

    /// 关闭一批标签并让编辑器聚焦到新的当前标签。
    fn close_now(&mut self, paths: &[PathBuf], window: &mut Window, cx: &mut Context<Self>) {
        for p in paths {
            self.drop_tab(p);
        }
        self.focus_active(window, cx);
        // 次组的标签全关完了:请主组合并拆分
        if self.parent.is_some() && self.tabs.tabs().is_empty() {
            cx.emit(crate::ide::IdeEvent::GroupEmpty);
        }
        cx.notify();
    }

    /// 关闭一个标签(有未保存修改会先确认)。
    pub fn close_tab(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        self.request_close(vec![path.to_path_buf()], window, cx);
    }

    /// 关闭除 `keep` 之外的所有标签。
    pub fn close_others(&mut self, keep: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let list = self.tabs.others(keep);
        self.request_close(list, window, cx);
    }

    /// 关闭 `path` 右侧的所有标签。
    pub fn close_right(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let list = self.tabs.right_of(path);
        self.request_close(list, window, cx);
    }

    /// 保存所有有未保存修改的标签。
    pub fn save_all(&mut self, cx: &mut Context<Self>) {
        let dirty: Vec<PathBuf> = self.tabs.tabs().iter().filter(|p| self.docs.get(*p).is_some_and(|d| !d.saved)).cloned().collect();
        for p in dirty {
            self.save_path(&p, cx);
        }
    }

    /// 关闭所有标签(有未保存修改会先确认)。
    pub fn close_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let all = self.tabs.tabs().to_vec();
        self.request_close(all, window, cx);
    }

    /// 关闭所有没有未保存修改的标签(带修改的保留)。
    pub fn close_saved(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let saved: Vec<PathBuf> = self.tabs.tabs().iter().filter(|p| self.docs.get(*p).is_none_or(|d| d.saved)).cloned().collect();
        self.close_now(&saved, window, cx);
    }

    /// 关闭当前标签。
    pub fn close_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(c) = self.focused_child() {
            c.update(cx, |c, cx| c.close_active(window, cx));
            return;
        }
        if let Some(p) = self.tabs.active().map(Path::to_path_buf) {
            self.close_tab(&p, window, cx);
        }
    }
}
