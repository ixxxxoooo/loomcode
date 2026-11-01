//! IDE 编辑区的外框:标签栏、面包屑、底部信息条、欢迎页。纯展示,事件回到 `IdeView`。
//!
//! @author ixxxxoooo

use crate::ide::IdeView;
use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Selectable as _, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::path::{Path, PathBuf};

use crate::tab_style::TAB_H;

/// 标签栏上的一项。
pub struct TabItem {
    pub path: PathBuf,
    /// 有未保存的修改
    pub dirty: bool,
    /// 预览标签(斜体)
    pub preview: bool,
    /// 当前标签
    pub active: bool,
    /// 已固定
    pub pinned: bool,
}

/// 拖动标签时跟随鼠标的小浮标,同时携带被拖的路径。
#[derive(Clone)]
pub struct DraggedTab {
    pub path: PathBuf,
}

impl Render for DraggedTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let name = self.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        h_flex().gap_1p5().items_center().px_3().py_1().rounded(px(6.)).border_1().border_color(t.border).bg(t.background).text_size(px(12.5)).shadow_md().child(crate::file_icons::file_icon_img(&name, false, false, 14.)).child(name)
    }
}

/// 标签栏:预览标签用斜体;未保存显示圆点,悬停或当前标签显示关闭按钮;固定标签显示图钉;
/// 中键关闭、双击固定预览,可拖动排序(固定与非固定之间不互拖)。
pub fn tab_bar(items: Vec<TabItem>, cx: &mut Context<IdeView>) -> impl IntoElement {
    let t = cx.theme().clone();
    h_flex()
        .id("ide-tabs")
        .h(px(TAB_H))
        .w_full()
        .flex_none()
        .overflow_x_scroll()
        .bg(t.sidebar)
        .border_b_1()
        .border_color(t.border)
        .children(items.into_iter().enumerate().map(|(i, TabItem { path, dirty, preview, active, pinned })| {
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let group = SharedString::from(format!("tab-{i}"));
            let (p_click, p_close, p_mid) = (path.clone(), path.clone(), path.clone());
            let menu = tab_menu(cx.entity().downgrade(), path.clone(), pinned);
            let (p_drag, p_drop) = (path.clone(), path.clone());
            let accent = t.accent;
            crate::tab_style::tab_shell(SharedString::from(format!("ide-tab-{i}")), group.clone(), active, cx)
                .max_w(px(200.))
                .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                    this.activate(&p_click, window, cx);
                    if ev.click_count() >= 2 {
                        this.pin(&p_click, cx);
                    }
                }))
                .on_mouse_down(MouseButton::Middle, cx.listener(move |this, _, window, cx| this.close_tab(&p_mid, window, cx)))
                .on_drag(DraggedTab { path: p_drag }, |d, _, _, cx| cx.new(|_| d.clone()))
                .drag_over::<DraggedTab>(move |s, _, _, _| s.bg(accent))
                .on_drop(cx.listener(move |this, d: &DraggedTab, _, cx| this.move_tab(&d.path, &p_drop, cx)))
                .context_menu(menu)
                .child(if pinned { icon(IconName::Pin, 13., t.muted_foreground).into_any_element() } else { crate::file_icons::file_icon_img(&name, false, false, 14.).into_any_element() })
                .child(div().min_w_0().truncate().when(preview, |d| d.italic()).child(name))
                .child(
                    crate::tab_style::tab_close_box(SharedString::from(format!("ide-tab-x-{i}")), cx)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.close_tab(&p_close, window, cx);
                        }))
                        // 未保存:常驻圆点;悬停时换成关闭图标。其余:当前标签常显,其他标签悬停才显
                        .when(dirty, |d| d.child(div().size(px(8.)).rounded_full().bg(t.foreground).group_hover(group.clone(), |s| s.size(px(0.)))))
                        .child(
                            div()
                                .when(!active && !dirty, |d| d.invisible().group_hover(group.clone(), |s| s.visible()))
                                .when(dirty, |d| d.invisible().group_hover(group.clone(), |s| s.visible()))
                                .child(crate::tab_style::tab_close_icon(cx)),
                        ),
                )
        }))
}

/// 标签右键菜单:关闭 / 关闭其他 / 关闭右侧 / 复制路径。
fn tab_menu(view: WeakEntity<IdeView>, path: PathBuf, pinned: bool) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu, _, _| {
        let item = |label: &'static str, icon: IconName| PopupMenuItem::new(label).icon(icon);
        let (v, p) = (view.clone(), path.clone());
        let close = item("关闭", IconName::X).on_click(move |_, window, cx| {
            let _ = v.update(cx, |this, cx| this.close_tab(&p, window, cx));
        });
        let (v, p) = (view.clone(), path.clone());
        let others = item("关闭其他", IconName::X).on_click(move |_, window, cx| {
            let _ = v.update(cx, |this, cx| this.close_others(&p, window, cx));
        });
        let (v, p) = (view.clone(), path.clone());
        let right = item("关闭右侧", IconName::X).on_click(move |_, window, cx| {
            let _ = v.update(cx, |this, cx| this.close_right(&p, window, cx));
        });
        let (v, p) = (view.clone(), path.clone());
        let pin = item(if pinned { "取消固定" } else { "固定标签" }, IconName::Pin).on_click(move |_, _, cx| {
            let _ = v.update(cx, |this, cx| this.toggle_pin_tab(&p, cx));
        });
        let p = path.clone();
        let copy = item("复制路径", IconName::Copy).on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(p.display().to_string())));
        menu.item(pin).separator().item(close).item(others).item(right).separator().item(copy)
    }
}

/// 面包屑:项目内的相对路径,最后一段为文件名;保存失败等提示显示在右侧。
pub fn breadcrumbs(path: &Path, root: &str, saved: bool, note: Option<String>, blame: Option<bool>, cx: &mut Context<IdeView>) -> impl IntoElement {
    let t = cx.theme().clone();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let tilde;
    let rel = match path.strip_prefix(root) {
        Ok(r) => r,
        // 不在项目内:用 ~ 缩写家目录,不显示一长串绝对路径
        Err(_) => match home.as_deref().and_then(|h| path.strip_prefix(h).ok()) {
            Some(r) => {
                tilde = Path::new("~").join(r);
                tilde.as_path()
            }
            None => path,
        },
    };
    let segs: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
    let last = segs.len().saturating_sub(1);
    h_flex()
        .h(px(26.))
        .w_full()
        .flex_none()
        .px_3()
        .gap_1()
        .items_center()
        .text_size(px(12.))
        .text_color(t.muted_foreground)
        // 路径段放进可收缩、裁切的容器:窗口窄时只裁路径,右侧的提示与刷新按钮始终可见
        .child(h_flex().flex_1().min_w_0().overflow_hidden().gap_1().items_center().children(segs.into_iter().enumerate().flat_map(|(i, s)| {
            let mut parts = Vec::new();
            if i > 0 {
                parts.push(icon(IconName::ChevronRight, 11., t.muted_foreground).into_any_element());
            }
            parts.push(div().flex_none().whitespace_nowrap().when(i == last, |d| d.text_color(t.foreground)).child(s).into_any_element());
            parts
        })))
        .when_some(note, |d, n| d.child(div().text_color(t.warning).child(n)))
        .when(!saved, |d| d.child(div().text_color(t.muted_foreground).child("未保存 · ⌘S")))
        .when_some(blame, |d, on| {
            d.child(
                Button::new("ide-blame")
                    .xsmall()
                    .ghost()
                    .icon(IconName::GitCommitHorizontal)
                    .selected(on)
                    .tooltip(if on { "隐藏 Git 追溯栏" } else { "显示 Git 追溯栏(逐行作者与时间)" })
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_blame_gutter(cx))),
            )
        })
        .child(
            Button::new("ide-reload")
                .xsmall()
                .ghost()
                .icon(IconName::RefreshCw)
                .tooltip("从磁盘重新载入(丢弃未保存修改)")
                .on_click(cx.listener(|this, _, window, cx| this.reload(window, cx))),
        )
}

/// 没有打开文件时的欢迎页:常用快捷键。
pub fn welcome(cx: &App) -> impl IntoElement {
    let t = cx.theme();
    let hints = [("显示 / 隐藏侧栏", "⌘B"), ("显示 / 隐藏对话栏", "⌥⌘B"), ("查找 / 替换", "⌘F"), ("保存文件", "⌘S"), ("关闭标签", "⌘W"), ("切换标签", "⌥⌘← / →"), ("切换到 Agent", "⇧⌘E")];
    v_flex()
        .size_full()
        .overflow_hidden()
        .items_center()
        .justify_center()
        .gap_4()
        .child(icon(IconName::Code, 40., t.muted_foreground))
        // 嵌在 Agent 右栏时很窄:宽度随容器收缩(最宽 280),不再固定撑出去
        .child(v_flex().gap_1().w_full().max_w(px(280.)).px_3().children(hints.into_iter().map(|(label, key)| {
            h_flex().gap_2().text_size(px(12.)).text_color(t.muted_foreground).child(div().flex_1().min_w_0().truncate().child(label)).child(div().flex_none().child(key))
        })))
}
