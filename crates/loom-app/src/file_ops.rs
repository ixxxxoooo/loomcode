//! 文件树的文件操作:新建文件 / 文件夹、重命名、移到废纸篓、在访达中显示,以及行右键菜单。
//!
//! @author ixxxxoooo

use crate::right_panel::RightPanel;
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::WindowExt as _;
use gpui_kit::*;
use std::cell::Cell;
use std::path::{Path, PathBuf};

/// 校验一个文件名:非空、不含路径分隔符、不是 `.` / `..`。
///
/// 返回:合法返回 `Ok(())`,否则返回给用户看的中文原因。
pub fn validate_name(name: &str) -> Result<(), String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("名称不能为空".into());
    }
    if n.contains('/') || n.contains('\0') || n == "." || n == ".." {
        return Err("名称不能包含 / 或为 . / ..".into());
    }
    Ok(())
}

impl RightPanel {
    /// 重新读取目录,并在后台线程刷新 Git 状态(不阻塞界面)。
    pub fn reload_files(&mut self, cx: &mut Context<Self>) {
        self.files.reload();
        let root = self.files.root_path().to_path_buf();
        cx.spawn(async move |this, cx| {
            let git = cx.background_spawn(async move { crate::git_status::GitStatus::read(&root) }).await;
            let _ = this.update(cx, |this, cx| {
                this.files.set_git(git);
                cx.notify();
            });
        })
        .detach();
    }

    /// 弹出「新建文件 / 文件夹」输入框。
    ///
    /// 参数:`dir` 父目录;`is_dir` 为真时新建文件夹。
    pub fn new_entry_dialog(&mut self, dir: PathBuf, is_dir: bool, window: &mut Window, cx: &mut Context<Self>) {
        let title = if is_dir { "新建文件夹" } else { "新建文件" };
        self.name_dialog(title, String::new(), window, cx, move |this, name, cx| {
            let target = dir.join(name.trim());
            if target.exists() {
                return Err("同名文件已存在".into());
            }
            let res = if is_dir { std::fs::create_dir_all(&target) } else { std::fs::write(&target, "") };
            res.map_err(|e| format!("创建失败:{e}"))?;
            this.files.expand_to(&target);
            this.reload_files(cx);
            if !is_dir {
                cx.emit(crate::right_panel::PanelEvent::OpenFile(target));
            }
            Ok(())
        });
    }

    /// 弹出重命名输入框。
    pub fn rename_dialog(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let old = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        self.name_dialog("重命名", old, window, cx, move |this, name, cx| {
            let target = path.with_file_name(name.trim());
            if target != path && target.exists() {
                return Err("同名文件已存在".into());
            }
            std::fs::rename(&path, &target).map_err(|e| format!("重命名失败:{e}"))?;
            this.reload_files(cx);
            Ok(())
        });
    }

    /// 通用的「输入名称」对话框;`apply` 返回 `Err(原因)` 时对话框保持打开并提示。
    fn name_dialog(&mut self, title: &'static str, initial: String, window: &mut Window, cx: &mut Context<Self>, apply: impl Fn(&mut Self, String, &mut Context<Self>) -> Result<(), String> + 'static) {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("名称").default_value(initial));
        let (panel, apply) = (cx.entity().downgrade(), std::rc::Rc::new(apply));
        let first = Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false) {
                input.focus_handle(cx).focus(window, cx);
            }
            let (panel, input, apply) = (panel.clone(), input.clone(), apply.clone());
            dialog.title(title).child(Input::new(&input.clone())).footer(crate::ui::dialog_footer("确定")).on_ok(move |_, window, cx| {
                let name = input.read(cx).value().to_string();
                let result = panel.update(cx, |this, cx| {
                    let r = validate_name(&name).and_then(|_| apply(this, name.clone(), cx));
                    cx.notify();
                    r
                });
                match result {
                    Ok(Ok(())) => true,
                    Ok(Err(msg)) => {
                        crate::notice::warning(window, cx, msg);
                        false
                    }
                    Err(_) => true,
                }
            })
        });
    }

    /// 弹出「移到废纸篓」确认(可从废纸篓恢复)。
    pub fn delete_dialog(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let panel = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (panel, path) = (panel.clone(), path.clone());
            alert
                .confirm()
                .title(format!("要把「{name}」移到废纸篓吗?"))
                .description("之后可以在废纸篓里恢复。")
                .ok_text("移到废纸篓")
                .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    match trash_path(&path) {
                        Ok(()) => {
                            let _ = panel.update(cx, |this, cx| {
                                this.reload_files(cx);
                                cx.notify();
                            });
                        }
                        Err(e) => crate::notice::error(window, cx, format!("移到废纸篓失败:{e}")),
                    }
                    true
                })
        });
    }
}

/// 把文件或目录移到废纸篓(通过访达,保留「放回原处」)。
///
/// 参数:`path` 目标路径。返回:失败原因。
pub fn trash_path(path: &Path) -> Result<(), String> {
    let escaped = path.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!("tell application \"Finder\" to delete POSIX file \"{escaped}\"");
    let out = std::process::Command::new("osascript").args(["-e", &script]).output().map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_string()) }
}

/// 文件树行的右键菜单:新建、重命名、删除、复制路径、在访达中显示。
///
/// 参数:`panel` 文件面板弱引用;`path` 目标路径;`is_dir` 是否目录。
pub fn row_menu(panel: WeakEntity<RightPanel>, path: PathBuf, is_dir: bool, root: PathBuf) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu, _, _| {
        let item = |label: &'static str, icon: IconName| PopupMenuItem::new(label).icon(icon);
        // 新建的位置:目录自身,或文件所在目录
        let base: PathBuf = if is_dir { path.clone() } else { path.parent().map(Path::to_path_buf).unwrap_or_default() };
        let (p, b) = (panel.clone(), base.clone());
        let new_file = item("新建文件…", IconName::FilePlus).on_click(move |_, window, cx| {
            let b = b.clone();
            let _ = p.update(cx, |this, cx| this.new_entry_dialog(b, false, window, cx));
        });
        let (p, b) = (panel.clone(), base);
        let new_dir = item("新建文件夹…", IconName::FolderPlus).on_click(move |_, window, cx| {
            let b = b.clone();
            let _ = p.update(cx, |this, cx| this.new_entry_dialog(b, true, window, cx));
        });
        let (p, t) = (panel.clone(), path.clone());
        let rename = item("重命名…", IconName::Pencil).on_click(move |_, window, cx| {
            let t = t.clone();
            let _ = p.update(cx, |this, cx| this.rename_dialog(t, window, cx));
        });
        let (p, t) = (panel.clone(), path.clone());
        let delete = item("移到废纸篓…", IconName::Trash).on_click(move |_, window, cx| {
            let t = t.clone();
            let _ = p.update(cx, |this, cx| this.delete_dialog(t, window, cx));
        });
        let t = path.clone();
        let copy = item("复制路径", IconName::Copy).on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(t.display().to_string())));
        let (t, r) = (path.clone(), root.clone());
        let copy_rel = item("复制相对路径", IconName::Copy).on_click(move |_, _, cx| {
            let rel = t.strip_prefix(&r).unwrap_or(&t).display().to_string();
            cx.write_to_clipboard(ClipboardItem::new_string(rel))
        });
        let t = path.clone();
        let reveal = item("在访达中显示", IconName::ExternalLink).on_click(move |_, _, _| {
            let _ = std::process::Command::new("open").arg("-R").arg(&t).spawn();
        });
        menu.item(new_file).item(new_dir).separator().item(rename).item(delete).separator().item(copy).item(copy_rel).item(reveal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 校验文件名() {
        assert!(validate_name("a.rs").is_ok());
        assert!(validate_name("  ").is_err());
        assert!(validate_name("a/b").is_err());
        assert!(validate_name("..").is_err());
    }
}
