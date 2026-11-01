//! 对话底部的分支行:显示当前分支,点开是分支 / worktree 菜单。
//!
//! @author ixxxxoooo

use crate::ui::icon;
use crate::workbench::Workbench;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::git::GitSnapshot;

/// 菜单里最多列出的分支数,多了靠滚动。
const MAX_MENU_H: f32 = 320.;

/// 渲染对话区的页脚:左边分支(真实模式且在仓库里时可点开菜单),右边运行模式说明。
pub fn footer(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let branch = wb.active_branch();
    let dir = wb.active_git_dir();
    let snapshot = dir.as_ref().and_then(|d| wb.git.get(d)).cloned();
    let left = match (wb.demo, dir, snapshot) {
        // 真实模式、在仓库里:分支按钮 + 下拉菜单
        (false, Some(dir), Some(snap)) => {
            let label = snap.current().map(str::to_string).or(branch).unwrap_or_else(|| "游离 HEAD".into());
            Button::new("branch-menu").ghost().xsmall().icon(IconName::GitBranch).label(label)
                .dropdown_menu(build_menu(cx.entity().downgrade(), dir, snap))
                .into_any_element()
        }
        _ => h_flex().gap_1().items_center().when_some(branch, |d, b| d.child(icon(IconName::GitBranch, 12., t.muted_foreground)).child(b)).into_any_element(),
    };
    h_flex().h(px(26.)).px_1().items_center().text_size(px(12.)).text_color(t.muted_foreground)
        .child(left)
        .child(div().flex_1())
        .child(if wb.demo { "" } else { "Claude Code" })
        .into_any_element()
}

/// 构造分支菜单:切换分支、新建分支、已有 worktree、新建 worktree。
///
/// 参数:`wb` 工作台弱引用;`dir` 仓库目录;`snap` 当前的分支快照。
fn build_menu(wb: WeakEntity<Workbench>, dir: String, snap: GitSnapshot) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |mut menu, _, _| {
        menu = menu.max_h(px(MAX_MENU_H)).scrollable(true).item(PopupMenuItem::label("切换分支"));
        for b in &snap.branches {
            let (w, d, name, current) = (wb.clone(), dir.clone(), b.name.clone(), b.current);
            let item = PopupMenuItem::new(b.name.clone()).checked(current).on_click(move |_, window, cx| {
                // 点当前分支不用切
                if !current {
                    let _ = w.update(cx, |this, cx| this.switch_branch(&d, &name, window, cx));
                }
            });
            menu = menu.item(item);
        }
        let (w, d) = (wb.clone(), dir.clone());
        menu = menu.separator().item(PopupMenuItem::new("新建分支…").icon(IconName::Plus).on_click(move |_, window, cx| {
            let _ = w.update(cx, |this, cx| this.create_branch_dialog(&d, window, cx));
        }));
        // 已有的 worktree(不含主工作区):点一下作为项目打开
        let others: Vec<_> = snap.worktrees.iter().filter(|w| !w.is_main).collect();
        if !others.is_empty() {
            menu = menu.separator().item(PopupMenuItem::label("Worktree"));
            for wt in others {
                let (w, p) = (wb.clone(), wt.path.clone());
                let label = wt.branch.clone().unwrap_or_else(|| loom_core::project_name_from_path(&wt.path));
                menu = menu.item(PopupMenuItem::new(label).icon(IconName::GitFork).on_click(move |_, window, cx| {
                    let _ = w.update(cx, |this, cx| this.register_project(p.clone(), window, cx));
                }));
            }
        }
        let (w, d) = (wb.clone(), dir.clone());
        menu.separator().item(PopupMenuItem::new("新建 Worktree…").icon(IconName::GitFork).on_click(move |_, window, cx| {
            let _ = w.update(cx, |this, cx| this.new_worktree_dialog(&d, window, cx));
        }))
    }
}
