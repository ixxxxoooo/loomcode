//! 左侧项目栏:项目 → 会话树,置顶、状态点、搜索入口、设置入口。

use crate::project_add;
use crate::project_menu;
use crate::session_menu;
use crate::sidebar_filter::visible_sessions;
use crate::ui::{icon, status_dot};
use crate::workbench::{Page, Workbench};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _};
use gpui_kit::component::{ActiveTheme, Selectable, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ucode_core::*;

/// 渲染一个会话行:状态点 + 标题 + 右侧 44pt(平时显示时间,悬停换成置顶 / 归档按钮)。
fn session_row(wb: &Workbench, project_ix: usize, meta: &SessionMeta, cx: &mut Context<Workbench>) -> impl IntoElement {
    let t = cx.theme().clone();
    let active = wb.active.as_deref() == Some(meta.id.as_str());
    // 活动会话的实时状态优先于演示数据里的静态状态
    let badge = wb.live_badge(&meta.id, meta.badge, cx);
    let id = meta.id.clone();
    let group = SharedString::from(format!("grp-{}", meta.id));
    let (pin_id, arch_id) = (meta.id.clone(), meta.id.clone());
    let (pinned, archived) = (meta.pinned, meta.archived);
    // 按钮点击不应触发整行的「打开会话」
    let stop = |_: &MouseDownEvent, _: &mut Window, cx: &mut App| cx.stop_propagation();
    let cwd = wb.projects.get(project_ix).map(|p| p.path.clone()).unwrap_or_default();
    let menu = session_menu::build(cx.entity().downgrade(), meta, cwd);
    h_flex()
        .id(SharedString::from(format!("sess-{}", meta.id)))
        .group(group.clone())
        .w_full()
        .h(px(30.))
        .pl(px(26.))
        .pr_2()
        .gap_2()
        .items_center()
        .rounded(px(6.))
        .cursor_pointer()
        .when(active, |d| d.bg(t.sidebar_accent))
        .hover(|d| d.bg(t.sidebar_accent))
        .on_click(cx.listener(move |this, _, window, cx| this.open_session(project_ix, &id, window, cx)))
        .child(div().w(px(8.)).flex_none().children(status_dot(badge, cx)))
        .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).when(archived, |d| d.text_color(t.muted_foreground)).child(meta.title.clone()))
        .child(
            div().relative().w(px(44.)).h_full().flex_none()
                .child(
                    h_flex().size_full().justify_end().items_center().gap_1().text_size(px(11.)).text_color(t.muted_foreground)
                        .when(pinned, |d| d.child(icon(IconName::Pin, 11., t.muted_foreground)))
                        .child(meta.updated.clone()),
                )
                .child(
                    h_flex().absolute().inset_0().justify_end().items_center().gap_0p5().bg(t.sidebar_accent).invisible()
                        .group_hover(group, |d| d.visible())
                        .on_mouse_down(MouseButton::Left, stop)
                        .child(Button::new(SharedString::from(format!("pin-{}", meta.id))).xsmall().ghost()
                            .icon(IconName::Pin).tooltip(if pinned { "取消置顶" } else { "置顶" })
                            .on_click(cx.listener(move |this, _, _, cx| this.toggle_pin_session(&pin_id, cx))))
                        .child(Button::new(SharedString::from(format!("arch-{}", meta.id))).xsmall().ghost()
                            .icon(IconName::Archive).tooltip(if archived { "取消归档" } else { "归档" })
                            .on_click(cx.listener(move |this, _, _, cx| this.set_session_archived(&arch_id, !archived, cx)))),
                ),
        )
        .context_menu(menu)
}

/// 侧栏顶部的一行入口(图标 + 文字 + 可选快捷键)。
fn nav_row(
    id: &'static str,
    ic: IconName,
    label: &'static str,
    shortcut: Option<&'static str>,
    active: bool,
    cx: &mut Context<Workbench>,
    on_click: impl Fn(&mut Workbench, &mut Window, &mut Context<Workbench>) + 'static,
) -> impl IntoElement {
    let t = cx.theme().clone();
    h_flex().id(id).h(px(32.)).px_2().gap_2().items_center().rounded(px(6.)).cursor_pointer()
        .when(active, |d| d.bg(t.sidebar_accent))
        .hover(|d| d.bg(t.sidebar_accent))
        .on_click(cx.listener(move |this, _, window, cx| on_click(this, window, cx)))
        .child(icon(ic, 15., t.muted_foreground))
        .child(div().flex_1().text_size(px(13.)).child(label))
        .when_some(shortcut, |d, k| d.child(div().text_size(px(11.)).text_color(t.muted_foreground).child(k)))
}

/// 渲染整个左侧栏。
pub fn render(wb: &Workbench, cx: &mut Context<Workbench>) -> impl IntoElement {
    let t = cx.theme().clone();
    let projects = wb.projects.clone();
    // 先算出每个项目要显示的会话(已归档的按开关取舍);搜索统一走 ⌘K 命令面板,侧栏不再放过滤框
    let shown: Vec<(usize, Vec<SessionMeta>)> = projects
        .iter()
        .enumerate()
        .filter(|(_, p)| !wb.ui_state.hidden_projects.contains(&p.path))
        .filter_map(|(pi, p)| visible_sessions("", p, wb.show_archived, wb.active.as_deref()).map(|v| (pi, v.into_iter().cloned().collect())))
        .collect();
    v_flex()
        .size_full()
        .bg(t.sidebar)
        .border_r_1()
        .border_color(t.sidebar_border)
        .text_color(t.sidebar_foreground)
        .child(
            v_flex().px_2().pt_2().pb_1().gap_0p5()
                .child(nav_row("nav-new", IconName::SquarePen, "新对话", Some("⌘N"), false, cx,
                    |this, window, cx| this.new_session(this.target_project(), window, cx)))
                .child(nav_row("nav-auto-row", IconName::Timer, "自动化", None, wb.page == Page::Automation, cx,
                    |this, _, cx| this.set_page(Page::Automation, cx))),
        )
        .child(
            h_flex().px_3().pt_2().pb_1().gap_1().items_center()
                .child(div().flex_1().text_size(px(12.)).text_color(t.muted_foreground).child("工作区"))
                .when(!wb.demo, |d| d.child(Button::new("add-project").small().ghost().icon(IconName::FolderPlus).tooltip("添加项目")
                    .dropdown_menu(project_add::build_menu(cx.entity().downgrade(), project_add::existing_recent(&wb.settings.recent_projects)))))
                .child(Button::new("show-archived").small().ghost().icon(IconName::Archive)
                    .tooltip(if wb.show_archived { "隐藏已归档会话" } else { "显示已归档会话" })
                    .selected(wb.show_archived)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_show_archived(cx)))),
        )
        .child(
            v_flex().id("projects").flex_1().min_h_0().overflow_y_scroll().px_2().py_1().gap_0p5()
                .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| this.drop_projects(paths.paths(), window, cx)))
                .children(shown.into_iter().map(|(pi, sessions)| {
                    let p = &projects[pi];
                    let expanded = p.expanded;
                    let pinned = wb.ui_state.pinned_projects.contains(&p.path);
                    let workspace = wb.ui_state.workspace_by_dir(&p.path).cloned();
                    let worktree = wb.is_worktree_project(&p.path);
                    let menu = project_menu::build(cx.entity().downgrade(), p.path.clone(), p.name.clone(), pinned, workspace.is_some(), worktree);
                    v_flex()
                        .child(
                            h_flex().id(SharedString::from(format!("proj-{pi}"))).h(px(28.)).px_1().gap_1p5().items_center().rounded(px(6.)).cursor_pointer()
                                .hover(|d| d.bg(t.sidebar_accent))
                                .on_click(cx.listener(move |this, _, _, cx| this.toggle_project(pi, cx)))
                                .child(icon(if expanded { IconName::ChevronDown } else { IconName::ChevronRight }, 12., t.muted_foreground))
                                .child(icon(if workspace.is_some() { IconName::Folders } else if worktree { IconName::GitFork } else { IconName::Folder }, 14., t.muted_foreground))
                                .child(div().flex_1().truncate().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(p.name.clone()))
                                .when(pinned, |d| d.child(icon(IconName::Pin, 11., t.muted_foreground)))
                                .when_some(workspace.as_ref(), |d, w| d.child(div().text_size(px(11.)).text_color(t.muted_foreground).child(format!("{} 个文件夹", w.folders.len()))))
                                .when_some(p.branch.clone(), |d, b| {
                                    d.child(h_flex().gap_0p5().items_center().text_size(px(11.)).text_color(t.muted_foreground)
                                        .child(icon(IconName::GitBranch, 11., t.muted_foreground)).child(b))
                                })
                                .context_menu(menu),
                        )
                        .when(expanded, |d| d.children(sessions.iter().map(|s| session_row(wb, pi, s, cx).into_any_element())))
                })),
        )
        .child(
            h_flex().h(px(40.)).px_2().gap_1().items_center().border_t_1().border_color(t.sidebar_border)
                .child(Button::new("nav-settings").small().ghost().icon(IconName::Settings).tooltip("设置")
                    .on_click(cx.listener(|this, _, _, cx| this.set_page(Page::Settings, cx))))
                .when_some(crate::ui::env_badges(wb.demo, cx), |d, b| d.child(b))
                .child(div().flex_1())
                .child(Button::new("nav-canvas").small().ghost().icon(IconName::LayoutDashboard)
                    .on_click(cx.listener(|this, _, _, cx| this.set_page(Page::Canvas, cx))))
                .child(Button::new("nav-auto").small().ghost().icon(IconName::Timer)
                    .on_click(cx.listener(|this, _, _, cx| this.set_page(Page::Automation, cx))))
                .child(Button::new("nav-browser").small().ghost().icon(IconName::Globe)
                    .on_click(cx.listener(|this, _, _, cx| this.set_page(Page::Browser, cx)))),
        )
}
