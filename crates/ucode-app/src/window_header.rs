//! Agent 窗口去掉整条标题栏后,左 / 中 / 右三列各自的顶部行。
//!
//! 红绿灯浮在最左边那一列的顶部:侧栏展开时是侧栏顶部行留出位置,侧栏收起后换成对话区顶部行留位置。
//! 顶部行空白处可拖动窗口,按钮用 [`no_drag`] 包住避免误拖。
//!
//! @author ixxxxoooo

use crate::ui::{mode_switch, no_drag};
use crate::workbench::{Page, Workbench};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::Disableable as _;
use gpui_kit::component::{ActiveTheme, InteractiveElementExt as _, Sizable, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// 顶部行高度。
pub const HEADER_H: f32 = 40.;
/// 给 macOS 红绿灯留的左边距。
#[cfg(target_os = "macos")]
const TRAFFIC_PAD: f32 = 80.;
#[cfg(not(target_os = "macos"))]
const TRAFFIC_PAD: f32 = 12.;

thread_local! {
    /// 鼠标在可拖动区域按下后「待拖动」:随后第一次移动才真正开始移动窗口(同 TitleBar 的做法,
    /// 这样单击、双击不会被当成拖动)。
    static DRAG_ARMED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// 把一个有 id 的元素变成可拖动窗口的区域:按下后移动即拖动,双击等同标题栏双击(缩放 / 最大化)。
/// 里面的按钮要用 [`no_drag`] 包住,否则点按钮也会触发拖动。
pub fn draggable(el: Stateful<Div>) -> Stateful<Div> {
    el.window_control_area(WindowControlArea::Drag)
        .on_mouse_down(MouseButton::Left, |_, _, _| DRAG_ARMED.with(|a| a.set(true)))
        .on_mouse_up(MouseButton::Left, |_, _, _| DRAG_ARMED.with(|a| a.set(false)))
        .on_mouse_down_out(|_, _, _| DRAG_ARMED.with(|a| a.set(false)))
        .on_mouse_move(|_, window, _| {
            if DRAG_ARMED.with(|a| a.replace(false)) {
                window.start_window_move();
            }
        })
        .on_double_click(|_, window, _| window.titlebar_double_click())
}

/// 一个可拖动窗口的顶部行(固定高度)。`id` 在同一窗口内要唯一。
fn header_row(id: &'static str) -> Stateful<Div> {
    draggable(h_flex().id(id).w_full().h(px(HEADER_H)).flex_none().items_center().gap_1())
}

/// 「侧栏开关 + 搜索」两个按钮。
fn left_buttons(wb: &Workbench, cx: &mut Context<Workbench>) -> impl IntoElement {
    let (can_back, can_fwd) = (wb.nav.can_back(), wb.nav.can_forward());
    h_flex().gap_0p5()
        .child(no_drag(Button::new("hd-left").small().ghost().icon(IconName::PanelLeft).tooltip("侧栏(⌘B)")
            .on_click(cx.listener(|this, _, _, cx| { this.left_open = !this.left_open; cx.notify(); }))))
        .child(no_drag(Button::new("hd-search").small().ghost().icon(IconName::Search).tooltip("搜索(⌘K)")
            .on_click(cx.listener(|this, _, window, cx| this.toggle_palette(window, cx)))))
        .child(no_drag(Button::new("hd-back-nav").small().ghost().icon(IconName::ArrowLeft).tooltip("后退").disabled(!can_back)
            .on_click(cx.listener(|this, _, window, cx| this.navigate(false, window, cx)))))
        .child(no_drag(Button::new("hd-fwd-nav").small().ghost().icon(IconName::ArrowRight).tooltip("前进").disabled(!can_fwd)
            .on_click(cx.listener(|this, _, window, cx| this.navigate(true, window, cx)))))
}

impl Workbench {
    /// 按访问历史前进 / 后退一个会话;已经不存在的会话(被删除或项目被移除)会被跳过。
    ///
    /// 参数:`forward` true 前进、false 后退。
    pub fn navigate(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        loop {
            let target = if forward { self.nav.forward() } else { self.nav.back() };
            let Some(id) = target else { return };
            let project_ix = self.views.get(&id).map(|v| v.project_ix)
                .or_else(|| self.projects.iter().position(|p| p.sessions.iter().any(|s| s.id == id)));
            if let Some(pi) = project_ix {
                self.nav_navigating = true;
                self.open_session(pi, &id, window, cx);
                self.nav_navigating = false;
                return;
            }
            self.nav.remove(&id);
        }
    }

    /// 侧栏顶部行:红绿灯位置 + 侧栏开关 + 搜索。
    pub fn left_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        header_row("hd-left-row").pl(px(TRAFFIC_PAD)).pr_2().child(left_buttons(self, cx))
    }

    /// 对话区顶部行:侧栏收起时承接红绿灯与按钮,中间是当前会话标题,右面板收起时右端放面板开关与 IDE 入口。
    pub fn center_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let left_hidden = !self.left_visible();
        let title = match self.page {
            Page::Chat => self.active_model().map(|m| m.read(cx).state.title.clone()),
            Page::Settings => Some("设置".to_string()),
            Page::Canvas => Some("Canvas".to_string()),
            Page::Automation => Some("自动化".to_string()),
            Page::Browser => Some("浏览器".to_string()),
        };
        let right_hidden = !self.right_visible();
        header_row("hd-center-row").pl(px(if left_hidden { TRAFFIC_PAD } else { 12. })).pr_2()
            .when(left_hidden, |d| d.child(left_buttons(self, cx)))
            .when(self.page != Page::Chat, |d| d.child(no_drag(Button::new("hd-back").small().ghost().icon(IconName::ArrowLeft).tooltip("返回会话")
                .on_click(cx.listener(|this, _, _, cx| this.set_page(Page::Chat, cx))))))
            .when_some(title.filter(|s| !s.is_empty()), |d, title| d.child(
                h_flex().h(px(28.)).px_3().items_center().rounded(px(8.)).bg(t.muted).text_size(px(13.)).max_w(px(360.)).overflow_hidden()
                    .child(div().truncate().child(title)),
            ))
            .child(div().flex_1().h_full())
            .when(right_hidden, |d| d.child(self.right_buttons(false, cx)))
    }

    /// 右端按钮:右面板开关 + IDE 入口。右面板展开时放在它自己的标签行末尾。
    ///
    /// 参数:`compact` 为 true 时 IDE 入口只显示图标(右面板较窄,标签行放不下带文字的按钮)。
    pub fn right_buttons(&self, compact: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let ide_icon = if self.settings.fused_ide { IconName::Code } else { IconName::ExternalLink };
        h_flex().gap_1().items_center()
            .child(no_drag(Button::new("hd-right").small().ghost().icon(IconName::PanelRight).tooltip("右侧面板(⌥⌘B)")
                .on_click(cx.listener(|this, _, _, cx| { this.right_open = !this.right_open; cx.notify(); }))))
            .child(no_drag(if compact {
                Button::new("hd-ide").small().ghost().icon(ide_icon).tooltip("切换到 IDE(⇧⌘E)")
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_ide(window, cx)))
            } else {
                mode_switch("hd-ide", "IDE", ide_icon, false).w(px(92.)).tooltip("切换到 IDE(⇧⌘E)")
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_ide(window, cx)))
            }))
    }
}
