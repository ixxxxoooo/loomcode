//! 左侧项目栏:项目 → 会话树,置顶、状态点、搜索入口、设置入口。

use crate::ui::{icon, status_dot};
use crate::workbench::{Page, Workbench};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ucode_core::*;

/// 渲染一个会话行。
fn session_row(wb: &Workbench, project_ix: usize, meta: &SessionMeta, cx: &mut Context<Workbench>) -> impl IntoElement {
    let t = cx.theme().clone();
    let active = wb.active.as_deref() == Some(meta.id.as_str());
    // 活动会话的实时状态优先于演示数据里的静态状态
    let badge = wb.live_badge(&meta.id, meta.badge, cx);
    let id = meta.id.clone();
    h_flex()
        .id(SharedString::from(format!("sess-{}", meta.id)))
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
        .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).child(meta.title.clone()))
        .child(div().text_size(px(11.)).text_color(t.muted_foreground).child(meta.updated.clone()))
        .when(meta.pinned, |d| d.child(icon(IconName::Pin, 11., t.muted_foreground)))
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
    v_flex()
        .w(px(wb.left_w))
        .h_full()
        .flex_none()
        .bg(t.sidebar)
        .border_r_1()
        .border_color(t.sidebar_border)
        .text_color(t.sidebar_foreground)
        .child(
            v_flex().px_2().pt_2().pb_1().gap_0p5()
                .child(nav_row("nav-new", IconName::SquarePen, "新对话", Some("⌘N"), false, cx,
                    |this, window, cx| this.new_session(this.target_project(), window, cx)))
                .child(nav_row("nav-auto-row", IconName::Timer, "自动化", None, wb.page == Page::Automation, cx,
                    |this, _, cx| this.set_page(Page::Automation, cx)))
                .child(nav_row("nav-custom", IconName::SlidersHorizontal, "自定义", None, false, cx,
                    |this, _, cx| this.set_page(Page::Settings, cx))),
        )
        .child(
            h_flex().px_3().pt_3().pb_1().gap_1().items_center()
                .child(div().flex_1().text_size(px(12.)).text_color(t.muted_foreground).child("工作区"))
                .when(!wb.demo, |d| d.child(Button::new("add-project").small().ghost().icon(IconName::FolderPlus).tooltip("添加项目目录")
                    .on_click(cx.listener(|this, _, window, cx| this.add_project(window, cx)))))
                .child(Button::new("search").small().ghost().icon(IconName::Search).tooltip("搜索(⌘K)")
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_palette(window, cx)))),
        )
        .child(
            v_flex().id("projects").flex_1().min_h_0().overflow_y_scroll().px_2().py_1().gap_0p5()
                .children(projects.iter().enumerate().map(|(pi, p)| {
                    let expanded = p.expanded;
                    v_flex()
                        .child(
                            h_flex().id(SharedString::from(format!("proj-{pi}"))).h(px(28.)).px_1().gap_1p5().items_center().rounded(px(6.)).cursor_pointer()
                                .hover(|d| d.bg(t.sidebar_accent))
                                .on_click(cx.listener(move |this, _, _, cx| this.toggle_project(pi, cx)))
                                .child(icon(if expanded { IconName::ChevronDown } else { IconName::ChevronRight }, 12., t.muted_foreground))
                                .child(icon(IconName::Folder, 14., t.muted_foreground))
                                .child(div().flex_1().truncate().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(p.name.clone()))
                                .when_some(p.branch.clone(), |d, b| {
                                    d.child(h_flex().gap_0p5().items_center().text_size(px(11.)).text_color(t.muted_foreground)
                                        .child(icon(IconName::GitBranch, 11., t.muted_foreground)).child(b))
                                }),
                        )
                        .when(expanded, |d| d.children(p.sessions.iter().map(|s| session_row(wb, pi, s, cx).into_any_element())))
                })),
        )
        .child(
            h_flex().h(px(40.)).px_2().gap_1().items_center().border_t_1().border_color(t.sidebar_border)
                .child(Button::new("nav-settings").small().ghost().icon(IconName::Settings).tooltip("设置")
                    .on_click(cx.listener(|this, _, _, cx| this.set_page(Page::Settings, cx))))
                .child(div().flex_1())
                .child(Button::new("nav-canvas").small().ghost().icon(IconName::LayoutDashboard)
                    .on_click(cx.listener(|this, _, _, cx| this.set_page(Page::Canvas, cx))))
                .child(Button::new("nav-auto").small().ghost().icon(IconName::Timer)
                    .on_click(cx.listener(|this, _, _, cx| this.set_page(Page::Automation, cx))))
                .child(Button::new("nav-browser").small().ghost().icon(IconName::Globe)
                    .on_click(cx.listener(|this, _, _, cx| this.set_page(Page::Browser, cx)))),
        )
}
