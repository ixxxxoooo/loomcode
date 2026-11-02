//! 分支与 worktree 的界面操作:刷新快照、切换分支、新建分支。git 命令都在后台线程跑。
//!
//! @author ixxxxoooo

use crate::ui::dialog_footer;
use crate::workbench::Workbench;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::*;
use std::cell::Cell;
use loom_core::git::{self, GitSnapshot};

impl Workbench {
    /// 当前会话所在项目用来跑 git 的目录:普通项目是它自己,工作区取主目录。
    pub fn active_git_dir(&self) -> Option<String> {
        self.active_project_path()
    }

    /// 后台重新读取某个目录的分支 / worktree 快照,读完写进 `git` 缓存并重绘。
    pub fn refresh_git(&mut self, dir: String, cx: &mut Context<Self>) {
        if self.demo {
            return;
        }
        let query = dir.clone();
        cx.spawn(async move |this, cx| {
            let snap = cx.background_spawn(async move { GitSnapshot::load(&query) }).await;
            let _ = this.update(cx, |this, cx| {
                match snap {
                    Some(s) => this.git.insert(dir, s),
                    None => this.git.remove(&dir),
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// 在后台执行一个会改动仓库的 git 操作,完成后刷新快照和项目分支;失败时弹出错误通知。
    ///
    /// 参数:`dir` 仓库目录;`ok_msg` 成功提示;`op` 要执行的操作(返回 `Err` 为用户可读原因)。
    fn run_git(&mut self, dir: &str, ok_msg: String, window: &mut Window, cx: &mut Context<Self>, op: impl FnOnce(&str) -> Result<(), String> + Send + 'static) {
        let dir = dir.to_string();
        let target = dir.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx.background_spawn(async move { op(&target) }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(()) => crate::notice::success(window, cx, ok_msg),
                    Err(e) => crate::notice::error(window, cx, format!("Git 操作失败:{e}")),
                }
                this.refresh_git(dir.clone(), cx);
                this.refresh_branches(&dir, cx);
            });
        })
        .detach();
    }

    /// 刷新侧栏里与该目录对应的项目的分支标签。
    fn refresh_branches(&mut self, dir: &str, cx: &mut Context<Self>) {
        let paths: Vec<String> = self.projects.iter().filter(|p| p.path == dir || self.work_root(&p.path) == dir).map(|p| p.path.clone()).collect();
        for p in paths {
            self.refresh_project(p, cx);
        }
    }

    /// 切换到已有分支。
    pub fn switch_branch(&mut self, dir: &str, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let name = name.to_string();
        let msg = format!("已切换到 {name}");
        self.run_git(dir, msg, window, cx, move |d| git::switch_branch(d, &name));
    }

    /// 切换分支(分支选择器用):`remote` 为真时检出远程分支并建立跟踪。
    pub fn switch_ref(&mut self, dir: &str, name: &str, remote: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !remote {
            return self.switch_branch(dir, name, window, cx);
        }
        let name = name.to_string();
        let msg = format!("已检出远程分支 {name}");
        self.run_git(dir, msg, window, cx, move |d| git::switch_remote_branch(d, &name));
    }

    /// 以给定名称新建分支并切换(分支选择器里直接输入新名字)。
    pub fn create_branch_named(&mut self, dir: &str, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let name = name.to_string();
        let msg = format!("已创建并切换到 {name}");
        self.run_git(dir, msg, window, cx, move |d| git::create_branch(d, &name));
    }

    /// 弹出「新建分支」对话框:输入名称,基于当前 HEAD 建分支并切换过去。
    pub fn create_branch_dialog(&mut self, dir: &str, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("新分支名,例如 feature/login"));
        let (wb, dir) = (cx.entity().downgrade(), dir.to_string());
        let first = Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false) {
                input.focus_handle(cx).focus(window, cx);
            }
            let (wb, dir, input) = (wb.clone(), dir.clone(), input.clone());
            dialog.title("新建分支").child(Input::new(&input.clone())).footer(dialog_footer("创建")).on_ok(move |_, window, cx| {
                let name = input.read(cx).value().trim().to_string();
                // 名称不合法就留在对话框里并提示原因
                if let Err(e) = git::validate_branch_name(&name) {
                    crate::notice::warning(window, cx, e);
                    return false;
                }
                let _ = wb.update(cx, |this, cx| {
                    let n = name.clone();
                    this.run_git(&dir, format!("已创建并切换到 {name}"), window, cx, move |d| git::create_branch(d, &n));
                });
                true
            })
        });
    }
}
