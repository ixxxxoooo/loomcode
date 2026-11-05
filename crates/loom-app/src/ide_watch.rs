//! 监视已打开文件在磁盘上的变化(Claude 在后台改文件时编辑器要跟着更新)。
//! 没有未保存修改的标签自动重新载入;有未保存修改的标签只提示冲突,不覆盖用户的输入。
//!
//! @author ixxxxoooo

use crate::ide::IdeView;
use gpui_kit::component::RopeExt as _;
use gpui_kit::*;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// 节拍:只读一个标志位,几乎没有开销;文件监视报告有改动时在下一拍就检查磁盘。
const TICK: Duration = Duration::from_millis(200);
/// 兜底:没有监视事件时每隔这么多拍(5 秒)也按修改时间检查一次,覆盖监视不到的文件(项目根之外、事件丢失)。
const FALLBACK_TICKS: u32 = 25;

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
    /// 启动检查循环:有监视事件(`fs_dirty`)就立刻检查所有已打开文件,否则每 5 秒兜底检查一次。
    pub fn start_watch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor().timer(TICK).await;
            let alive = this.update_in(cx, |this, window, cx| {
                this.fs_tick += 1;
                if this.fs_dirty || this.fs_tick >= FALLBACK_TICKS {
                    this.fs_dirty = false;
                    this.fs_tick = 0;
                    this.check_disk(window, cx);
                }
            });
            if alive.is_err() {
                break;
            }
        })
        .detach();
    }

    /// 文件监视的一批改动:涉及已打开的文件(或系统报告丢了事件)就标记,下一拍检查磁盘;拆分出的次组同理。
    pub(crate) fn on_fs_change(&mut self, b: &crate::fs_watch::Batch, cx: &mut Context<Self>) {
        if b.rescan || self.docs.keys().any(|p| b.paths.contains(p)) {
            self.fs_dirty = true;
        }
        if let Some(child) = self.split.as_ref().map(|s| s.child.clone()) {
            child.update(cx, |c, cx| c.on_fs_change(b, cx));
        }
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
                    // 无论是否有焦点(软件在后台时就没有),都要保住光标与滚动位置:
                    // `set_value` 会把两者重置到文件开头,外部改动重载后不该把人带走
                    let (pos, scroll) = { let e = editor.read(cx); (e.cursor_position(), e.scroll_offset()) };
                    editor.update(cx, |e, cx| {
                        e.set_value(text, window, cx);
                        // 用 `set_selected_range` 而不是 `set_cursor_position`:后者会抢焦点
                        let off = e.text().position_to_offset(&pos);
                        e.set_selected_range(off..off, cx);
                        e.set_scroll_offset(scroll, cx);
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
