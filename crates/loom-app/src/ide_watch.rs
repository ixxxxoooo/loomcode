//! 监视已打开文件在磁盘上的变化(Claude 在后台改文件时编辑器要跟着更新)。
//! 没有未保存修改的标签自动重新载入;有未保存修改的标签只提示冲突,不覆盖用户的输入。
//!
//! @author ixxxxoooo

use crate::ide::IdeView;
use gpui_kit::*;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// 轮询间隔。
const TICK: Duration = Duration::from_millis(1200);

/// 磁盘变化的处理结论。
#[derive(Debug, PartialEq, Eq)]
pub enum DiskChange {
    /// 没变
    None,
    /// 标签干净,应当重新载入
    Reload,
    /// 标签有未保存修改,标记冲突
    Conflict,
}

/// 根据修改时间与是否有未保存修改,决定怎么处理。
///
/// 参数:`known` 上次读取 / 保存时的修改时间;`disk` 现在磁盘上的修改时间;`saved` 标签是否干净。
pub fn classify(known: Option<SystemTime>, disk: Option<SystemTime>, saved: bool) -> DiskChange {
    if known == disk || disk.is_none() {
        DiskChange::None
    } else if saved {
        DiskChange::Reload
    } else {
        DiskChange::Conflict
    }
}

impl IdeView {
    /// 启动轮询:窗口存在期间每隔 `TICK` 检查一次所有已打开文件。
    pub fn start_watch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor().timer(TICK).await;
            if this.update_in(cx, |this, window, cx| this.check_disk(window, cx)).is_err() {
                break;
            }
        })
        .detach();
    }

    /// 对比每个文档的修改时间,按结论重新载入或标记冲突。
    fn check_disk(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths: Vec<PathBuf> = self.docs.keys().filter(|p| !crate::ide_untitled::is_untitled(p)).cloned().collect();
        let mut changed = false;
        let mut reloaded = Vec::new();
        for path in paths {
            let disk = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
            let Some(doc) = self.docs.get_mut(&path) else { continue };
            match classify(doc.mtime, disk, doc.saved) {
                DiskChange::None => {}
                DiskChange::Conflict => {
                    doc.mtime = disk;
                    doc.stale = true;
                    changed = true;
                }
                DiskChange::Reload => {
                    let Ok(text) = std::fs::read_to_string(&path) else { continue };
                    doc.mtime = disk;
                    doc.crlf = text.contains("\r\n");
                    let editor = doc.editor.clone();
                    let focused = editor.read(cx).focus_handle(cx).is_focused(window);
                    let pos = editor.read(cx).cursor_position();
                    editor.update(cx, |e, cx| {
                        e.set_value(text, window, cx);
                        if focused {
                            e.set_cursor_position(pos, window, cx);
                        }
                    });
                    reloaded.push(path.clone());
                    changed = true;
                }
            }
        }
        for p in reloaded {
            self.rebuild_minimap(&p, None, cx);
            self.refresh_marks(&p, cx);
        }
        if changed {
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 按修改时间与脏状态分类() {
        let t0 = SystemTime::UNIX_EPOCH;
        let t1 = t0 + Duration::from_secs(5);
        assert_eq!(classify(Some(t0), Some(t0), true), DiskChange::None);
        assert_eq!(classify(Some(t0), Some(t1), true), DiskChange::Reload);
        assert_eq!(classify(Some(t0), Some(t1), false), DiskChange::Conflict);
        assert_eq!(classify(Some(t0), None, true), DiskChange::None);
    }
}
