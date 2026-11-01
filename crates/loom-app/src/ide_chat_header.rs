//! IDE 对话栏顶部:当前会话标题(点开可切换同项目的其他会话)和「新会话」按钮。
//!
//! @author ixxxxoooo

use crate::ide_window::IdeWindow;
use crate::ui::icon;
use crate::workbench::Workbench;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex};
use gpui_kit::*;

/// 菜单里最多列出的会话数。
const MAX_LISTED: usize = 12;

/// 构造对话栏头部。没有激活会话时返回 `None`。
///
/// 参数:`wb` 工作台实体,`cx` 为 IDE 窗口上下文。
pub fn chat_header(wb: &Entity<Workbench>, cx: &mut Context<IdeWindow>) -> Option<impl IntoElement> {
    let t = cx.theme().clone();
    let (ix, active, title, sessions) = {
        let w = wb.read(cx);
        let id = w.active.clone()?;
        let ix = w.views.get(&id)?.project_ix;
        let project = w.projects.get(ix)?;
        let title = project.sessions.iter().find(|s| s.id == id).map(|s| s.title.clone()).unwrap_or_else(|| "新会话".into());
        let sessions: Vec<(String, String)> = project.sessions.iter().filter(|s| !s.archived).take(MAX_LISTED).map(|s| (s.id.clone(), s.title.clone())).collect();
        (ix, id, title, sessions)
    };
    let weak = wb.downgrade();
    let menu = move |menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>| {
        let mut menu = menu;
        for (id, title) in sessions.clone() {
            let (w, target) = (weak.clone(), id.clone());
            let item = PopupMenuItem::new(title).checked(id == active).on_click(move |_, window, cx| {
                let _ = w.update(cx, |this, cx| this.open_session(ix, &target, window, cx));
            });
            menu = menu.item(item);
        }
        menu
    };
    let new_wb = wb.downgrade();
    Some(
        h_flex().h(px(34.)).w_full().flex_none().px_2().gap_1().items_center().border_b_1().border_color(t.border)
            .child(icon(IconName::MessageSquare, 13., t.muted_foreground))
            .child(
                div().flex_1().min_w_0().flex().justify_start().child(
                    Button::new("ide-session-switch").small().ghost().label(title).icon(IconName::ChevronDown).dropdown_menu(menu),
                ),
            )
            .child(
                Button::new("ide-new-session").small().ghost().icon(IconName::Plus).tooltip("新会话").on_click(move |_, window, cx| {
                    let _ = new_wb.update(cx, |this, cx| this.new_session(ix, window, cx));
                }),
            ),
    )
}
