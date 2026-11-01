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
use gpui_kit::component::{ActiveTheme, Sizable, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// 顶部行高度。
pub const HEADER_H: f32 = 40.;
/// 给 macOS 红绿灯留的左边距。
#[cfg(target_os = "macos")]
const TRAFFIC_PAD: f32 = 80.;
#[cfg(not(target_os = "macos"))]
const TRAFFIC_PAD: f32 = 12.;

/// 一个可拖动窗口的顶部行(固定高度)。
fn header_row() -> Div {
    h_flex().w_full().h(px(HEADER_H)).flex_none().items_center().gap_1().window_control_area(WindowControlArea::Drag)
}

/// 「侧栏开关 + 搜索」两个按钮。
fn left_buttons(cx: &mut Context<Workbench>) -> impl IntoElement {
    h_flex().gap_0p5()
        .child(no_drag(Button::new("hd-left").small().ghost().icon(IconName::PanelLeft).tooltip("侧栏(⌘B)")
            .on_click(cx.listener(|this, _, _, cx| { this.left_open = !this.left_open; cx.notify(); }))))
        .child(no_drag(Button::new("hd-search").small().ghost().icon(IconName::Search).tooltip("搜索(⌘K)")
            .on_click(cx.listener(|this, _, window, cx| this.toggle_palette(window, cx)))))
}

impl Workbench {
    /// 侧栏顶部行:红绿灯位置 + 侧栏开关 + 搜索。
    pub fn left_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        header_row().pl(px(TRAFFIC_PAD)).pr_2().child(left_buttons(cx))
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
        header_row().pl(px(if left_hidden { TRAFFIC_PAD } else { 12. })).pr_2()
            .when(left_hidden, |d| d.child(left_buttons(cx)))
            .when(self.page != Page::Chat, |d| d.child(no_drag(Button::new("hd-back").small().ghost().icon(IconName::ArrowLeft).tooltip("返回会话")
                .on_click(cx.listener(|this, _, _, cx| this.set_page(Page::Chat, cx))))))
            .when_some(title.filter(|s| !s.is_empty()), |d, title| d.child(
                h_flex().h(px(28.)).px_3().items_center().rounded(px(8.)).bg(t.muted).text_size(px(13.)).max_w(px(360.)).overflow_hidden()
                    .child(div().truncate().child(title)),
            ))
            .child(div().flex_1().h_full())
            .when(right_hidden, |d| d.child(self.right_buttons(cx)))
    }

    /// 右端按钮:右面板开关 + IDE 入口。右面板展开时放在它自己的标签行末尾。
    pub fn right_buttons(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex().gap_1().items_center()
            .child(no_drag(Button::new("hd-right").small().ghost().icon(IconName::PanelRight).tooltip("右侧面板(⌥⌘B)")
                .on_click(cx.listener(|this, _, _, cx| { this.right_open = !this.right_open; cx.notify(); }))))
            .child(no_drag(mode_switch("hd-ide", "IDE", if self.settings.fused_ide { IconName::Code } else { IconName::ExternalLink }, false)
                .w(px(92.)).tooltip("切换到 IDE(⇧⌘E)")
                .on_click(cx.listener(|this, _, window, cx| this.toggle_ide(window, cx)))))
    }
}
