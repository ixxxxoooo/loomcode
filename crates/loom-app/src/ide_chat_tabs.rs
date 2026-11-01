//! IDE 对话栏顶部的会话标签条(参考 JetBrains AI 对话栏):每个标签一个会话,
//! 右侧是「+ 新会话 / 历史会话 / 展开(对话栏占满窗口)」。
//!
//! @author ixxxxoooo

use crate::ide_window::IdeWindow;
use crate::ui::{empty_state, icon};
use crate::workbench::Workbench;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::Phase;

/// 历史菜单里最多列出的会话数。
const MAX_HISTORY: usize = 40;
/// 标签标题最大宽度。
const TAB_MAX_W: f32 = 150.;

/// 找到某个会话所在项目的下标。
fn project_of(wb: &Workbench, id: &str) -> Option<usize> {
    wb.views.get(id).map(|v| v.project_ix).or_else(|| wb.projects.iter().position(|p| p.sessions.iter().any(|s| s.id == id)))
}

/// 会话标题(在所有项目里找,找不到显示「新会话」)。
fn title_of(wb: &Workbench, id: &str) -> String {
    wb.projects.iter().flat_map(|p| p.sessions.iter()).find(|s| s.id == id).map(|s| s.title.clone()).unwrap_or_else(|| "新会话".into())
}

impl IdeWindow {
    /// 同步标签:当前会话变了(新建、从别处打开)就确保它在标签里;关光所有标签后保持空状态直到有新会话。
    pub(crate) fn sync_chat_tabs(&mut self, cx: &App) {
        let active = self.wb.read(cx).active.clone();
        if active != self.chat_last_active {
            if let Some(id) = &active && !self.chat_tabs.contains(id) {
                self.chat_tabs.push(id.clone());
            }
            self.chat_empty = false;
            self.chat_last_active = active;
        }
        // 已被删除的会话不再留标签
        let wb = self.wb.read(cx);
        self.chat_tabs.retain(|id| wb.views.contains_key(id) || wb.projects.iter().any(|p| p.sessions.iter().any(|s| &s.id == id)));
    }

    /// 切到某个标签(没有就加上)。
    pub(crate) fn activate_chat_tab(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !self.chat_tabs.iter().any(|t| t == id) {
            self.chat_tabs.push(id.to_string());
        }
        self.chat_empty = false;
        let target = id.to_string();
        let Some(ix) = project_of(self.wb.read(cx), id) else { return };
        let _ = self.wb.update(cx, |wb, cx| wb.open_session(ix, &target, window, cx));
        cx.notify();
    }

    /// 关闭一个标签;关的是当前会话时切到相邻标签,没有了就进入空状态。
    pub(crate) fn close_chat_tab(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pos) = self.chat_tabs.iter().position(|t| t == id) else { return };
        self.chat_tabs.remove(pos);
        if self.wb.read(cx).active.as_deref() == Some(id) {
            match self.chat_tabs.get(pos).or_else(|| self.chat_tabs.last()).cloned() {
                Some(next) => self.activate_chat_tab(&next, window, cx),
                None => self.chat_empty = true,
            }
        }
        cx.notify();
    }

    /// 在当前项目里新开一个会话(它会自动成为新标签)。
    fn new_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ix = {
            let wb = self.wb.read(cx);
            wb.active.as_ref().and_then(|id| project_of(wb, id)).unwrap_or_else(|| wb.target_project())
        };
        let _ = self.wb.update(cx, |wb, cx| wb.new_session(ix, window, cx));
    }

    /// 对话栏顶部标签条。
    pub(crate) fn chat_tabs_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let (active, history, running) = {
            let wb = self.wb.read(cx);
            let active = if self.chat_empty { None } else { wb.active.clone() };
            let ix = wb.active.as_ref().and_then(|id| project_of(wb, id));
            let history: Vec<(String, String)> = ix.and_then(|i| wb.projects.get(i)).map(|p| {
                p.sessions.iter().filter(|s| !s.archived).take(MAX_HISTORY).map(|s| (s.id.clone(), s.title.clone())).collect()
            }).unwrap_or_default();
            let running: Vec<String> = self.chat_tabs.iter().filter(|id| wb.views.get(*id).is_some_and(|v| v.model.read(cx).state.phase == Phase::Running)).cloned().collect();
            (active, history, running)
        };
        let open_ids = self.chat_tabs.clone();
        let this = cx.entity().downgrade();
        let menu = move |menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>| {
            let mut menu = menu;
            if history.is_empty() {
                return menu.item(PopupMenuItem::new("暂无历史会话").disabled(true));
            }
            for (id, title) in history.clone() {
                let (w, target) = (this.clone(), id.clone());
                menu = menu.item(PopupMenuItem::new(title).checked(open_ids.contains(&id)).on_click(move |_, window, cx| {
                    let _ = w.update(cx, |this, cx| this.activate_chat_tab(&target, window, cx));
                }));
            }
            menu
        };
        let tabs = self.chat_tabs.iter().cloned().map(|id| {
            let on = active.as_deref() == Some(id.as_str());
            let title = title_of(self.wb.read(cx), &id);
            let group = SharedString::from(format!("ct-g-{id}"));
            let (sel, close) = (id.clone(), id.clone());
            h_flex().id(SharedString::from(format!("ct-{id}"))).group(group.clone()).relative().h_full().flex_none().pl_3().pr_1().gap_1p5().items_center().cursor_pointer()
                .text_size(px(13.))
                .border_r_1().border_color(t.border)
                .when(on, |d| d.bg(t.background).text_color(t.foreground).child(div().absolute().top_0().left_0().right_0().h(px(2.)).bg(t.primary)))
                .when(!on, |d| d.text_color(t.muted_foreground).hover(|d| d.bg(t.list_hover)))
                .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, window, cx| this.activate_chat_tab(&sel, window, cx)))
                .on_mouse_down(MouseButton::Middle, cx.listener({ let id = id.clone(); move |this, _, window, cx| this.close_chat_tab(&id, window, cx) }))
                .when(running.contains(&id), |d| d.child(div().size(px(6.)).rounded_full().bg(t.primary)))
                .child(div().max_w(px(TAB_MAX_W)).truncate().child(title))
                .child(
                    div().id(SharedString::from(format!("ct-x-{id}"))).size(px(18.)).flex().items_center().justify_center().rounded(px(4.))
                        .when(!on, |d| d.invisible().group_hover(group, |d| d.visible()))
                        .hover(|d| d.bg(t.border))
                        .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, window, cx| { cx.stop_propagation(); this.close_chat_tab(&close, window, cx); }))
                        .child(icon(IconName::X, 11., t.muted_foreground)),
                )
        });
        let expanded = self.chat_wide;
        h_flex().h(px(36.)).w_full().flex_none().items_center().border_b_1().border_color(t.border).bg(t.sidebar)
            .child(div().id("ide-chat-tabs").h_full().flex_1().min_w_0().overflow_x_scroll().child(h_flex().h_full().children(tabs)))
            .child(
                h_flex().h_full().flex_none().px_1().gap_0p5().items_center()
                    .child(Button::new("ct-new").small().ghost().icon(IconName::Plus).tooltip("新会话")
                        .on_click(cx.listener(|this, _, window, cx| this.new_chat(window, cx))))
                    .child(Button::new("ct-history").small().ghost().icon(IconName::Clock).tooltip("历史会话").dropdown_menu(menu))
                    .child(Button::new("ct-expand").small().ghost().icon(if expanded { IconName::Minimize2 } else { IconName::Maximize2 })
                        .tooltip(if expanded { "还原对话栏" } else { "展开对话栏" })
                        .on_click(cx.listener(|this, _, _, cx| { this.chat_wide = !this.chat_wide; cx.notify(); }))),
            )
    }

    /// 所有标签都关掉后的空状态。
    pub(crate) fn chat_empty_view(&self, cx: &mut Context<Self>) -> AnyElement {
        h_flex().size_full().items_center().justify_center().child(
            gpui_kit::component::v_flex().items_center().gap_3()
                .child(empty_state(IconName::MessageSquare, "没有打开的会话", "点「+」新建,或从历史会话里打开", cx).h_auto().w_auto())
                .child(Button::new("ct-empty-new").small().primary().icon(IconName::Plus).label("新会话")
                    .on_click(cx.listener(|this, _, window, cx| this.new_chat(window, cx)))),
        ).into_any_element()
    }
}
