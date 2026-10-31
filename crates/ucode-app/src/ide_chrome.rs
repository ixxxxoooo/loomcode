//! IDE 编辑区的外框:标签栏、面包屑、底部信息条、欢迎页。纯展示,事件回到 `IdeView`。
//!
//! @author ixxxxoooo

use crate::ide::IdeView;
use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::path::{Path, PathBuf};

/// 标签栏高度。
const TAB_H: f32 = 35.;

/// 标签栏:每项 `(路径, 是否未保存, 是否预览, 是否当前)`。
/// 预览标签用斜体;未保存显示圆点,悬停或当前标签显示关闭按钮;中键关闭、双击固定。
pub fn tab_bar(items: Vec<(PathBuf, bool, bool, bool)>, cx: &mut Context<IdeView>) -> impl IntoElement {
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
        .children(items.into_iter().enumerate().map(|(i, (path, dirty, preview, active))| {
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let group = SharedString::from(format!("tab-{i}"));
            let (p_click, p_close, p_mid) = (path.clone(), path.clone(), path.clone());
            let menu = tab_menu(cx.entity().downgrade(), path.clone());
            h_flex()
                .id(SharedString::from(format!("ide-tab-{i}")))
                .group(group.clone())
                .h_full()
                .flex_none()
                .max_w(px(200.))
                .pl_3()
                .pr_1()
                .gap_1p5()
                .items_center()
                .cursor_pointer()
                .border_r_1()
                .border_color(t.border)
                .text_size(px(12.5))
                .when(active, |d| d.bg(t.background).text_color(t.foreground).border_t_2().border_color(t.primary))
                .when(!active, |d| d.text_color(t.muted_foreground).hover(|s| s.bg(t.muted)))
                .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                    this.activate(&p_click, window, cx);
                    if ev.click_count() >= 2 {
                        this.pin(&p_click, cx);
                    }
                }))
                .on_mouse_down(MouseButton::Middle, cx.listener(move |this, _, window, cx| this.close_tab(&p_mid, window, cx)))
                .context_menu(menu)
                .child(icon(IconName::FileText, 13., t.muted_foreground))
                .child(div().min_w_0().truncate().when(preview, |d| d.italic()).child(name))
                .child(
                    div()
                        .id(SharedString::from(format!("ide-tab-x-{i}")))
                        .size(px(18.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(4.))
                        .hover(|s| s.bg(t.accent))
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
                                .child(icon(IconName::X, 12., t.muted_foreground)),
                        ),
                )
        }))
}

/// 标签右键菜单:关闭 / 关闭其他 / 关闭右侧 / 复制路径。
fn tab_menu(view: WeakEntity<IdeView>, path: PathBuf) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
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
        let p = path.clone();
        let copy = item("复制路径", IconName::Copy).on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(p.display().to_string())));
        menu.item(close).item(others).item(right).separator().item(copy)
    }
}

/// 面包屑:项目内的相对路径,最后一段为文件名;保存失败等提示显示在右侧。
pub fn breadcrumbs(path: &Path, root: &str, saved: bool, note: Option<String>, cx: &mut Context<IdeView>) -> impl IntoElement {
    let t = cx.theme().clone();
    let rel = path.strip_prefix(root).unwrap_or(path);
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
        .children(segs.into_iter().enumerate().flat_map(|(i, s)| {
            let mut parts = Vec::new();
            if i > 0 {
                parts.push(icon(IconName::ChevronRight, 11., t.muted_foreground).into_any_element());
            }
            parts.push(div().when(i == last, |d| d.text_color(t.foreground)).child(s).into_any_element());
            parts
        }))
        .child(div().flex_1())
        .when_some(note, |d, n| d.child(div().text_color(t.warning).child(n)))
        .when(!saved, |d| d.child(div().text_color(t.muted_foreground).child("未保存 · ⌘S")))
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
    let hints = [("显示 / 隐藏侧栏", "⌘B"), ("显示 / 隐藏对话栏", "⌥⌘B"), ("查找 / 替换", "⌘F"), ("保存文件", "⌘S"), ("关闭标签", "⌘W"), ("切换标签", "⌥⌘← / →"), ("切换到 Agents Window", "⇧⌘E")];
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_4()
        .child(icon(IconName::Code, 40., t.muted_foreground))
        .child(v_flex().gap_1().w(px(280.)).children(hints.into_iter().map(|(label, key)| {
            h_flex().text_size(px(12.)).text_color(t.muted_foreground).child(div().flex_1().child(label)).child(div().child(key))
        })))
}
