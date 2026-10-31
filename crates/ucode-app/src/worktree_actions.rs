//! Git worktree 的界面操作:新建(并在其中开会话)、移除。
//!
//! @author ixxxxoooo

use crate::ui::dialog_footer;
use crate::workbench::Workbench;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::*;
use std::cell::Cell;
use std::path::PathBuf;
use ucode_core::git;

impl Workbench {
    /// worktree 的统一存放目录(`~/.ucode/worktrees`,演示模式在沙盒里)。
    fn worktrees_root(&self) -> PathBuf {
        self.settings_path.with_file_name("worktrees")
    }

    /// 这个项目是不是 UCode 创建的 worktree(路径在统一存放目录下)。
    pub fn is_worktree_project(&self, path: &str) -> bool {
        std::path::Path::new(path).starts_with(self.worktrees_root())
    }

    /// 弹出「新建 Worktree」对话框:输入新分支名,在统一目录里检出并开一个会话。
    ///
    /// 参数:`dir` 主仓库目录。
    pub fn new_worktree_dialog(&mut self, dir: &str, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("新分支名,例如 feature/login"));
        let (wb, dir, root) = (cx.entity().downgrade(), dir.to_string(), self.worktrees_root());
        let first = Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false) {
                input.focus_handle(cx).focus(window, cx);
            }
            let (wb, dir, root, input) = (wb.clone(), dir.clone(), root.clone(), input.clone());
            dialog.title("新建 Worktree").child(Input::new(&input.clone())).footer(dialog_footer("创建")).on_ok(move |_, window, cx| {
                let branch = input.read(cx).value().trim().to_string();
                if let Err(e) = git::validate_branch_name(&branch) {
                    window.push_notification(Notification::warning(e), cx);
                    return false;
                }
                let _ = wb.update(cx, |this, cx| this.create_worktree(&dir, &root, &branch, window, cx));
                true
            })
        });
    }

    /// 后台创建 worktree,成功后把它作为项目打开并新建会话。
    fn create_worktree(&mut self, dir: &str, root: &std::path::Path, branch: &str, window: &mut Window, cx: &mut Context<Self>) {
        let (dir, root, branch) = (dir.to_string(), root.to_path_buf(), branch.to_string());
        cx.spawn_in(window, async move |this, cx| {
            let d = dir.clone();
            let result = cx
                .background_spawn(async move {
                    let repo = git::repo_root(&d).ok_or("当前目录不在 Git 仓库里")?;
                    let path = git::worktree_path(&root, &repo, &branch);
                    if path.exists() {
                        return Err(format!("目录已存在:{}", path.display()));
                    }
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败:{e}"))?;
                    }
                    git::add_worktree(&d, &path, &branch, None).map(|_| path)
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok(path) => {
                    window.push_notification(Notification::success("已创建 worktree"), cx);
                    this.refresh_git(dir, cx);
                    this.register_project(path.to_string_lossy().to_string(), window, cx);
                }
                Err(e) => window.push_notification(Notification::error(format!("创建 worktree 失败:{e}")), cx),
            });
        })
        .detach();
    }

    /// 弹出移除确认。分支会保留,只删 worktree 目录;有未提交改动时 git 会拒绝。
    pub fn remove_worktree_dialog(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let (wb, path) = (cx.entity().downgrade(), path.to_string());
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (wb, path) = (wb.clone(), path.clone());
            alert
                .confirm()
                .title("移除 Worktree?")
                .description(format!("将删除目录 {path}。对应分支会保留;有未提交改动时会拒绝移除。"))
                .ok_text("移除")
                .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    let _ = wb.update(cx, |this, cx| this.remove_worktree(&path, window, cx));
                    true
                })
        });
    }

    /// 后台移除 worktree,成功后把项目从侧栏拿掉。
    fn remove_worktree(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let path = path.to_string();
        cx.spawn_in(window, async move |this, cx| {
            let p = path.clone();
            let result = cx
                .background_spawn(async move {
                    // 列表第一项是主工作区,从它那里执行 remove(不能在 worktree 自己里面删自己)
                    let main = git::worktrees(&p)?.into_iter().next().map(|w| w.path).ok_or("找不到主工作区")?;
                    git::remove_worktree(&main, &p).map(|_| main)
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok(main) => {
                    window.push_notification(Notification::success("已移除 worktree"), cx);
                    this.remove_project(&path, cx);
                    this.refresh_git(main, cx);
                }
                Err(e) => window.push_notification(Notification::error(format!("移除 worktree 失败:{e}")), cx),
            });
        })
        .detach();
    }
}
