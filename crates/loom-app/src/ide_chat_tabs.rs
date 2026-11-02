//! IDE 对话栏顶部的会话标签条(参考 JetBrains AI 对话栏):每个标签一个会话,
//! 右侧是「+ 新会话 / 历史会话 / 展开(对话栏占满窗口)」。
//!
//! @author ixxxxoooo

use crate::ide_window::IdeWindow;
use crate::ui::empty_state;
use crate::workbench::Workbench;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

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
    /// IDE 固定项目在 `wb.projects` 里的下标。
    pub(crate) fn pinned_ix(&self, wb: &Workbench) -> Option<usize> {
        let p = self.pinned.as_ref()?;
        wb.projects.iter().position(|x| &x.path == p)
    }

    /// 当前会话是否属于别的项目(只会出现在独立 IDE 窗口里:Agent 窗口切去了别的项目)。
    pub(crate) fn active_is_foreign(&self, wb: &Workbench) -> bool {
        let Some(pin) = self.pinned_ix(wb) else { return false };
        wb.active.as_ref().and_then(|id| project_of(wb, id)).is_some_and(|ix| ix != pin)
    }

    /// 每次进入 IDE(或首次渲染)把窗口固定到当前项目,并载入该项目自己的对话标签;有变化返回 true。
    /// 之后在 IDE 里切换标签、新建 / 打开历史会话都只在这个项目内,不会换目录。
    pub(crate) fn ensure_pin(&mut self, cx: &mut Context<Self>) -> bool {
        let (epoch, cur, tabs) = {
            let wb = self.wb.read(cx);
            let cur = wb.current_project_ix();
            let path = cur.and_then(|i| wb.projects.get(i)).map(|p| p.path.clone());
            let tabs: Vec<String> = match cur {
                Some(ix) => wb.ui_state.last.chat_tabs.iter().filter(|id| project_of(wb, id) == Some(ix)).cloned().collect(),
                None => Vec::new(),
            };
            (wb.ide_enter, path, tabs)
        };
        if self.pinned.is_some() && self.seen_enter == epoch {
            return false;
        }
        // 项目还没加载出来时先不固定,下次渲染再试
        let Some(cur) = cur else { return false };
        self.seen_enter = epoch;
        if self.pinned.as_ref() == Some(&cur) {
            return false;
        }
        self.pinned = Some(cur);
        self.chat_tabs = tabs;
        self.chat_last_active = None;
        self.chat_empty = false;
        true
    }

    /// 同步标签:当前会话变了(新建、从别处打开)就确保它在标签里;关光所有标签后保持空状态直到有新会话。
    pub(crate) fn sync_chat_tabs(&mut self, cx: &mut Context<Self>) {
        let before = self.chat_tabs.clone();
        let active = self.wb.read(cx).active.clone();
        let foreign = self.active_is_foreign(self.wb.read(cx));
        if active != self.chat_last_active {
            if let Some(id) = &active && !foreign && !self.chat_tabs.contains(id) {
                self.chat_tabs.push(id.clone());
            }
            if !foreign {
                self.chat_empty = false;
            }
            self.chat_last_active = active;
        }
        // 已被删除的、或不属于本项目的会话不留标签(历史还没加载完时先保留,免得误删)
        let wb = self.wb.read(cx);
        if let Some(pin) = self.pinned_ix(wb) && (!wb.projects.is_empty() || wb.demo) {
            self.chat_tabs.retain(|id| project_of(wb, id) == Some(pin));
        }
        if self.chat_tabs != before {
            // 渲染期间不能直接更新工作台,放到渲染之后保存
            let me = cx.entity();
            cx.defer(move |cx| me.update(cx, |this, cx| this.save_chat_tabs(cx)));
        }
    }

    /// 把对话栏标签写进 `state.json`(内容没变就不写),下次启动恢复。
    pub(crate) fn save_chat_tabs(&mut self, cx: &mut Context<Self>) {
        let mine = self.chat_tabs.clone();
        let pin = self.pinned_ix(self.wb.read(cx));
        self.wb.update(cx, |wb, _| {
            // 标签按项目分开记忆:保留其它项目已存的标签,只替换本项目的那部分
            let mut tabs: Vec<String> = wb.ui_state.last.chat_tabs.iter().filter(|id| {
                let ix = project_of(wb, id);
                ix.is_some() && ix != pin
            }).cloned().collect();
            tabs.extend(mine);
            if wb.ui_state.last.chat_tabs != tabs {
                wb.ui_state.last.chat_tabs = tabs;
                wb.persist_ui_state();
            }
        });
    }

    /// 切到某个标签(没有就加上)。
    pub(crate) fn activate_chat_tab(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !self.chat_tabs.iter().any(|t| t == id) {
            self.chat_tabs.push(id.to_string());
        }
        self.chat_empty = false;
        self.save_chat_tabs(cx);
        let target = id.to_string();
        let Some(ix) = project_of(self.wb.read(cx), id) else { return };
        // 只能切到本项目的会话:切到别的项目会换掉 IDE 的目录
        if self.pinned_ix(self.wb.read(cx)).is_some_and(|pin| pin != ix) {
            self.chat_tabs.retain(|t| t != id);
            return;
        }
        let _ = self.wb.update(cx, |wb, cx| wb.open_session(ix, &target, window, cx));
        cx.notify();
    }

    /// 关闭一个标签;关的是当前会话时切到相邻标签,没有了就进入空状态。
    pub(crate) fn close_chat_tab(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pos) = self.chat_tabs.iter().position(|t| t == id) else { return };
        self.chat_tabs.remove(pos);
        self.save_chat_tabs(cx);
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
            self.pinned_ix(wb).or_else(|| wb.active.as_ref().and_then(|id| project_of(wb, id))).unwrap_or_else(|| wb.target_project())
        };
        let _ = self.wb.update(cx, |wb, cx| wb.new_session(ix, window, cx));
    }

    /// 对话栏顶部标签条。
    pub(crate) fn chat_tabs_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let (active, history, running) = {
            let wb = self.wb.read(cx);
            let active = if self.chat_empty || self.active_is_foreign(wb) { None } else { wb.active.clone() };
            let ix = self.pinned_ix(wb).or_else(|| wb.active.as_ref().and_then(|id| project_of(wb, id)));
            let history: Vec<(String, String)> = ix.and_then(|i| wb.projects.get(i)).map(|p| {
                p.sessions.iter().filter(|s| !s.archived).take(MAX_HISTORY).map(|s| (s.id.clone(), s.title.clone())).collect()
            }).unwrap_or_default();
            let running: Vec<String> = self.chat_tabs.iter().filter(|id| wb.live_sig(id).is_some_and(|s| s.turn)).cloned().collect();
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
            crate::tab_style::tab_shell(SharedString::from(format!("ct-{id}")), group.clone(), on, cx)
                .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, window, cx| this.activate_chat_tab(&sel, window, cx)))
                .on_mouse_down(MouseButton::Middle, cx.listener({ let id = id.clone(); move |this, _, window, cx| this.close_chat_tab(&id, window, cx) }))
                .when(running.contains(&id), |d| d.child(crate::ui::breathe(div().id("chat-tab-run").size(px(6.)).rounded_full().bg(t.primary), "chat-tab-run-breath")))
                .child(div().max_w(px(TAB_MAX_W)).truncate().child(title))
                .child(
                    crate::tab_style::tab_close_box(SharedString::from(format!("ct-x-{id}")), cx)
                        .when(!on, |d| d.invisible().group_hover(group, |d| d.visible()))
                        .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, window, cx| { cx.stop_propagation(); this.close_chat_tab(&close, window, cx); }))
                        .child(crate::tab_style::tab_close_icon(cx)),
                )
        });
        let expanded = self.chat_wide;
        h_flex().h(px(crate::tab_style::TAB_H)).w_full().flex_none().items_center().border_b_1().border_color(t.border).bg(t.sidebar)
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
