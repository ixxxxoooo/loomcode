//! 临时记事本(对标 VS Code 的「无标题」文件):双击标签栏空白处新建,只在保存时才选路径。
//!
//! 标签用伪路径 `untitled:/Untitled-N`:不落盘、不进标签记忆 / 最近文件 / 位置历史,
//! 不接语言服务器、Git 与磁盘监视;内容为空时关闭不提示,有内容关闭时走「保存 / 不保存」确认。
//!
//! @author ixxxxoooo

use crate::ide::IdeView;
use gpui_kit::*;
use std::path::{Path, PathBuf};

/// 临时标签伪路径的前缀。
const PREFIX: &str = "untitled:";

/// 是否为临时记事本的伪路径。
///
/// 参数:`path` 标签路径。返回:是则为真。
pub fn is_untitled(path: &Path) -> bool {
    path.to_str().is_some_and(|s| s.starts_with(PREFIX))
}

/// 临时标签的显示名(如 `Untitled-1`);非临时路径返回 `None`。
pub fn title(path: &Path) -> Option<String> {
    is_untitled(path).then(|| path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())
}

impl IdeView {
    /// 新建一个临时记事本标签并聚焦。
    pub fn new_untitled(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.restore_pending(window, cx);
        self.untitled_seq += 1;
        let path = PathBuf::from(format!("{PREFIX}/Untitled-{}", self.untitled_seq));
        let doc = crate::ide::build_doc(&path, String::new(), None, window, cx);
        self.docs.insert(path.clone(), doc);
        self.tabs.open(&path, false);
        self.focus_active(window, cx);
        cx.notify();
    }

    /// 为临时标签弹出系统「另存为」面板;选定后写盘,并在下次渲染时换成真实文件标签。
    ///
    /// 参数:`path` 临时标签。返回:恒为假(保存是异步的,调用方不要据此关闭标签)。
    pub(crate) fn save_untitled(&mut self, path: &Path, cx: &mut Context<Self>) -> bool {
        let Some(doc) = self.docs.get(path) else { return false };
        let text = doc.editor.read(cx).value().to_string();
        let base = if self.root.is_empty() { std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default() } else { PathBuf::from(&self.root) };
        let name = format!("{}.txt", title(path).unwrap_or_default());
        let rx = cx.prompt_for_new_path(&base, Some(&name));
        let old = path.to_path_buf();
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(new))) = rx.await else { return };
            if new.file_name().is_none() {
                return;
            }
            if let Err(e) = std::fs::write(&new, &text) {
                loom_diag::error!("保存临时文件失败 {}:{e}", new.display());
                let _ = this.update(cx, |this, cx| {
                    this.note = Some(format!("保存失败:{e}"));
                    cx.notify();
                });
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.saved_as.push((old, new));
                cx.notify();
            });
        })
        .detach();
        false
    }

    /// 渲染时处理已另存的临时标签:打开真实文件,关闭临时标签。
    pub(crate) fn finish_save_as(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for (old, new) in std::mem::take(&mut self.saved_as) {
            self.open_inner(new, false, true, window, cx);
            self.drop_tab(&old);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 伪路径识别与标题() {
        let p = Path::new("untitled:/Untitled-3");
        assert!(is_untitled(p));
        assert_eq!(title(p).as_deref(), Some("Untitled-3"));
        assert!(!is_untitled(Path::new("/tmp/a.rs")));
        assert_eq!(title(Path::new("/tmp/a.rs")), None);
    }
}
