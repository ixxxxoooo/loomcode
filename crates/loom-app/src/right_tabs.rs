//! 右侧面板的动态标签条(参考 OhMyAgent 的右栏):标签可关闭、右侧「+」菜单新增、
//! 没有标签时显示入口列表。标签内容仍由 [`RightTab`] 决定,IDE 窗口不显示标签条。
//!
//! @author ixxxxoooo

use crate::right_panel::RightPanel;
use crate::ui::{empty_state, icon, pill};
use crate::workbench::RightTab;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// 新打开面板时默认出现的标签。
pub const DEFAULT_TABS: [RightTab; 3] = [RightTab::Changes, RightTab::Files, RightTab::Terminal];

impl RightTab {
    /// 「+」菜单与空状态里的一句话说明。
    pub fn detail(self) -> &'static str {
        match self {
            Self::Changes => "Claude 的改动与 Git 更改",
            Self::Files => "浏览与预览项目文件",
            Self::Terminal => "在项目目录运行命令",
            Self::Plan => "查看待办与计划",
            Self::Output => "查看运行输出与日志",
        }
    }
}

impl RightPanel {
    /// 打开(或聚焦)一个标签。
    pub fn open_tab(&mut self, tab: RightTab, cx: &mut Context<Self>) {
        if !self.open.contains(&tab) {
            self.open.push(tab);
        }
        self.tab = tab;
        self.active_preview = None;
        cx.notify();
    }

    /// 在右栏打开文件预览:`pinned` 为假是预览标签(会被下一次单击替换),为真固定;已打开的直接选中。
    pub fn open_preview(&mut self, path: std::path::PathBuf, pinned: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.files.select(path.clone());
        if let Some(ix) = self.previews.iter().position(|p| p.path == path) {
            if pinned {
                self.previews[ix].preview = false;
            }
            self.active_preview = Some(ix);
        } else {
            let view = crate::file_preview::FilePreview::load(path, !pinned, window, cx);
            // 先替换已有的预览标签,没有就追加
            match (!pinned).then(|| self.previews.iter().position(|p| p.preview)).flatten() {
                Some(ix) => { self.previews[ix] = view; self.active_preview = Some(ix); }
                None => { self.previews.push(view); self.active_preview = Some(self.previews.len() - 1); }
            }
        }
        cx.notify();
    }

    /// 关闭一个文件预览标签。
    pub fn close_preview(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.previews.len() {
            return;
        }
        self.previews.remove(ix);
        self.active_preview = match self.active_preview {
            Some(a) if a == ix => (!self.previews.is_empty()).then(|| ix.min(self.previews.len() - 1)),
            Some(a) if a > ix => Some(a - 1),
            other => other,
        };
        cx.notify();
    }

    /// 关闭一个标签;关掉的是当前标签时,切到右边(没有则左边)的相邻标签。
    pub fn close_tab(&mut self, tab: RightTab, cx: &mut Context<Self>) {
        let Some(ix) = self.open.iter().position(|t| *t == tab) else { return };
        self.open.remove(ix);
        if self.tab == tab && let Some(next) = self.open.get(ix).or_else(|| self.open.last()) {
            self.tab = *next;
        }
        cx.notify();
    }

    /// 标签条:各标签(悬停显示关闭)+「+」新增菜单 + 末尾的外部控件。
    pub(crate) fn tab_bar(&self, changes: usize, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let (cur, open, trailing) = (self.tab, self.open.clone(), self.trailing.clone());
        let in_preview = self.active_preview.is_some();
        let weak = cx.entity().downgrade();
        let menu = move |menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>| {
            let mut menu = menu;
            for rt in RightTab::ALL {
                let (w, on) = (weak.clone(), open.contains(&rt));
                menu = menu.item(PopupMenuItem::new(format!("{}  ·  {}", rt.label(), rt.detail())).checked(on).on_click(move |_, _, cx| {
                    let _ = w.update(cx, |p, cx| p.open_tab(rt, cx));
                }));
            }
            menu
        };
        let tabs = self.open.iter().copied().map(|rt| {
            let on = rt == cur && !in_preview;
            let group = SharedString::from(format!("rt-g-{}", rt.key()));
            h_flex().id(SharedString::from(format!("rt-{}", rt.key()))).group(group.clone()).h(px(28.)).pl_2().pr_1().gap_1().items_center().rounded(px(6.)).cursor_pointer()
                .text_size(px(12.)).flex_none()
                .text_color(if on { t.foreground } else { t.muted_foreground })
                .when(on, |d| d.bg(t.muted))
                .hover(|d| d.bg(t.list_hover))
                .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _, cx| this.open_tab(rt, cx)))
                // 中键关闭
                .on_mouse_down(MouseButton::Middle, cx.listener(move |this, _, _, cx| this.close_tab(rt, cx)))
                .child(icon(rt.icon(), 12., if on { t.foreground } else { t.muted_foreground }))
                .child(rt.label())
                .when(rt == RightTab::Changes && changes > 0, |d| d.child(pill(changes.to_string(), t.primary_foreground, t.primary)))
                .child(
                    div().id(SharedString::from(format!("rt-x-{}", rt.key()))).size(px(16.)).flex().items_center().justify_center().rounded(px(4.))
                        .when(!on, |d| d.invisible().group_hover(group, |d| d.visible()))
                        .hover(|d| d.bg(t.border))
                        .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _, cx| { cx.stop_propagation(); this.close_tab(rt, cx); }))
                        .child(icon(IconName::X, 10., t.muted_foreground)),
                )
        });
        let active_preview = self.active_preview;
        let previews = self.previews.iter().enumerate().map(|(ix, p)| {
            let on = active_preview == Some(ix);
            let (name, preview) = (p.name(), p.preview);
            let group = SharedString::from(format!("rt-pg-{ix}"));
            h_flex().id(SharedString::from(format!("rt-p-{ix}"))).group(group.clone()).h(px(28.)).pl_2().pr_1().gap_1().items_center().rounded(px(6.)).cursor_pointer()
                .text_size(px(12.)).flex_none().max_w(px(160.))
                .text_color(if on { t.foreground } else { t.muted_foreground })
                .when(on, |d| d.bg(t.muted))
                .hover(|d| d.bg(t.list_hover))
                .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _, cx| { this.active_preview = Some(ix); cx.notify(); }))
                .on_mouse_down(MouseButton::Middle, cx.listener(move |this, _, _, cx| this.close_preview(ix, cx)))
                .child(crate::file_icons::file_icon_img(&name, false, false, 13.))
                .child(div().min_w_0().truncate().when(preview, |d| d.italic()).child(name))
                .child(
                    div().id(SharedString::from(format!("rt-px-{ix}"))).size(px(16.)).flex_none().flex().items_center().justify_center().rounded(px(4.))
                        .when(!on, |d| d.invisible().group_hover(group, |d| d.visible()))
                        .hover(|d| d.bg(t.border))
                        .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _, cx| { cx.stop_propagation(); this.close_preview(ix, cx); }))
                        .child(icon(IconName::X, 10., t.muted_foreground)),
                )
        });
        crate::window_header::draggable(h_flex().id("rt-tabbar").h(px(crate::window_header::HEADER_H)).flex_none().px_2().gap_0p5().items_center())
            // 标签放不下时横向滚动,「+」与右侧按钮始终固定可见
            .child(div().id("rt-tabs-scroll").min_w_0().h_full().flex().items_center().overflow_x_scroll().child(h_flex().gap_0p5().items_center().children(tabs).children(previews)))
            .child(Button::new("rt-add").xsmall().ghost().icon(IconName::Plus).tooltip("新增标签").dropdown_menu(menu))
            .child(div().flex_1().h_full())
            .when_some(trailing, |d, f| d.child(div().flex_none().child(f(window, cx))))
    }

    /// 没有任何标签时的入口列表。
    pub(crate) fn empty_tabs_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        v_flex().size_full().items_center().justify_center().gap_3().px_4()
            .child(empty_state(IconName::PanelRight, "右栏为空", "打开下面的入口,它们会以标签显示在这里", cx))
            .child(v_flex().w_full().max_w(px(300.)).gap_0p5().children(RightTab::ALL.into_iter().map(|rt| {
                h_flex().id(SharedString::from(format!("rt-entry-{}", rt.key()))).h(px(32.)).px_2().gap_2().items_center().rounded(px(6.)).cursor_pointer()
                    .hover(|d| d.bg(t.list_hover))
                    .on_click(cx.listener(move |this, _, _, cx| this.open_tab(rt, cx)))
                    .child(icon(rt.icon(), 14., t.muted_foreground))
                    .child(div().text_size(px(13.)).child(rt.label()))
                    .child(div().flex_1())
                    .child(div().text_size(px(11.)).text_color(t.muted_foreground).truncate().child(rt.detail()))
            })))
            .into_any_element()
    }
}
