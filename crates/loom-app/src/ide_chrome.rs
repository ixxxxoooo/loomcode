//! IDE 编辑区的外框:标签栏、面包屑、底部信息条、欢迎页。纯展示,事件回到 `IdeView`。
//!
//! @author ixxxxoooo

use crate::ide::IdeView;
use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Selectable as _, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::path::{Path, PathBuf};


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
    /// 来自哪个编辑器组
    pub from: WeakEntity<IdeView>,
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
pub fn tab_bar(items: Vec<TabItem>, scroll: ScrollHandle, root: &str, wrap: bool, split: bool, cx: &mut Context<IdeView>) -> impl IntoElement {
    let t = cx.theme().clone();
    let th = px(crate::tab_style::TAB_H);
    let tabs = h_flex()
        .id("ide-tabs")
        .flex_1()
        .min_w_0()
        // 多行:放不下就换行(最多 4 行,再多竖向滚动);单行:横向滚动
        .when(wrap, |d| d.flex_wrap().items_start().max_h(th * 4.).overflow_y_scroll())
        .when(!wrap, |d| d.h(th).overflow_x_scroll().track_scroll(&scroll).on_scroll_wheel(crate::tab_style::wheel_to_horizontal(scroll.clone())))
        .bg(t.sidebar)
        .children(items.into_iter().enumerate().map(|(i, TabItem { path, dirty, preview, active, pinned })| {
            let is_diff = crate::ide_diff::is_diff(&path);
            let name = if is_diff { crate::ide_diff::tab_title(&path) } else if let Some(n) = crate::ide_untitled::title(&path) { n } else { path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default() };
            let group = SharedString::from(format!("tab-{i}"));
            let (p_click, p_close, p_mid) = (path.clone(), path.clone(), path.clone());
            let menu = tab_menu(cx.entity().downgrade(), path.clone(), pinned, root.to_string());
            let (p_drag, p_drop, from) = (path.clone(), path.clone(), cx.weak_entity());
            let accent = t.accent;
            crate::tab_style::tab_shell(SharedString::from(format!("ide-tab-{i}")), group.clone(), active, cx)
                .max_w(px(200.))
                .h(th)
                // 底线只画在未选中的标签上:选中标签与下方内容区融为一体
                .when(!active, |d| d.border_b_1())
                .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                    this.activate(&p_click, window, cx);
                    if ev.click_count() >= 2 {
                        this.pin(&p_click, cx);
                    }
                }))
                .on_mouse_down(MouseButton::Middle, cx.listener(move |this, _, window, cx| this.close_tab(&p_mid, window, cx)))
                .on_drag(DraggedTab { path: p_drag, from }, |d, _, _, cx| cx.new(|_| d.clone()))
                .drag_over::<DraggedTab>(move |s, _, _, _| s.bg(accent))
                .on_drop(cx.listener(move |this, d: &DraggedTab, window, cx| this.drop_tab_on(d, Some(&p_drop), window, cx)))
                .context_menu(menu)
                .child(if pinned { icon(IconName::Pin, 13., t.muted_foreground).into_any_element() } else if is_diff { icon(IconName::FileDiff, 13., t.muted_foreground).into_any_element() } else { crate::file_icons::file_icon_img(&name, false, false, 14.).into_any_element() })
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
        // 标签右侧的空白:双击新建临时记事本(对标 VS Code)
        .child(div().id("ide-tabs-fill").flex_1().min_w(px(24.)).h(th).border_b_1().border_color(t.border)
            .drag_over::<DraggedTab>({ let accent = t.accent; move |s, _, _, _| s.bg(accent) })
            .on_drop(cx.listener(|this, d: &DraggedTab, window, cx| this.drop_tab_on(d, None, window, cx))).on_click(cx.listener(|this, ev: &ClickEvent, window, cx| {
            if ev.click_count() >= 2 {
                this.new_untitled(window, cx);
            }
        })));
    let view = cx.entity().downgrade();
    let (minimap_on, wrap_on) = { let e = crate::prefs(cx).editor; (e.minimap, e.soft_wrap) };
    let v_mm = view.clone();
    let v_ww = view.clone();
    let more = move |menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>| {
        let item = |label: &'static str| PopupMenuItem::new(label);
        let (v1, v2, v3) = (view.clone(), view.clone(), view.clone());
        let (s1, s2, s3, v4, v5) = (view.clone(), view.clone(), view.clone(), v_mm.clone(), v_ww.clone());
        let mut menu = menu
            .item(item("向右拆分").icon(IconName::PanelRight).on_click(move |_, window, cx| {
                let _ = s1.update(cx, |this, cx| this.split_active(false, window, cx));
            }))
            .item(item("向下拆分").icon(IconName::PanelBottom).on_click(move |_, window, cx| {
                let _ = s2.update(cx, |this, cx| this.split_active(true, window, cx));
            }));
        if split {
            menu = menu.item(item("合并拆分").on_click(move |_, window, cx| {
                let _ = s3.update(cx, |this, cx| this.close_split(window, cx));
            }));
        }
        menu.separator().item(item("多行标签").checked(wrap).on_click(move |_, _, cx| {
            let _ = v1.update(cx, |this, cx| {
                this.wrap_tabs = !this.wrap_tabs;
                cx.notify();
            });
        }))
        .item(item("显示迷你地图").checked(minimap_on).on_click(move |_, _, cx| {
            let _ = v4.update(cx, |this, cx| this.toggle_minimap(cx));
        }))
        .item(item("自动换行").checked(wrap_on).on_click(move |_, _, cx| {
            let _ = v5.update(cx, |this, cx| this.toggle_word_wrap(cx));
        }))
        .separator()
        .item(item("关闭已保存").on_click(move |_, window, cx| {
            let _ = v2.update(cx, |this, cx| this.close_saved(window, cx));
        }))
        .item(item("关闭全部").on_click(move |_, window, cx| {
            let _ = v3.update(cx, |this, cx| this.close_all(window, cx));
        }))
    };
    // 右侧操作区贴在第一行(多行时也不随换行下移),与标签同高、同底线
    h_flex().w_full().flex_none().items_start().bg(t.sidebar).child(tabs).child(
        h_flex().h(th).flex_none().px_1().items_center().border_b_1().border_color(t.border).child(Button::new("ide-tabs-split").xsmall().ghost().icon(IconName::PanelRight).tooltip("向右拆分").on_click(cx.listener(|this, _, window, cx| this.split_active(false, window, cx))))
            .child(Button::new("ide-tabs-more").xsmall().ghost().icon(IconName::Ellipsis).tooltip("更多操作").dropdown_menu(more)),
    )
}

/// 标签右键菜单:关闭 / 关闭其他 / 关闭右侧 / 复制路径。
fn tab_menu(view: WeakEntity<IdeView>, path: PathBuf, pinned: bool, root: String) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
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
        let (v, p) = (view.clone(), path.clone());
        let split_r = item("向右拆分", IconName::PanelRight).on_click(move |_, window, cx| {
            let _ = v.update(cx, |this, cx| {
                this.activate(&p, window, cx);
                this.split_active(false, window, cx);
            });
        });
        let (v, p) = (view.clone(), path.clone());
        let split_d = item("向下拆分", IconName::PanelBottom).on_click(move |_, window, cx| {
            let _ = v.update(cx, |this, cx| {
                this.activate(&p, window, cx);
                this.split_active(true, window, cx);
            });
        });
        let menu = menu.item(pin).separator().item(split_r).item(split_d).separator().item(close).item(others).item(right).separator();
        file_path_items(menu, &view, &path, &root)
    }
}

/// 在访达中显示。
fn reveal_in_finder(path: &Path) {
    let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
}

/// 面包屑右键菜单:在访达中显示 / 在资源管理器中定位 / 复制路径 / 复制相对路径。
fn crumb_menu(view: WeakEntity<IdeView>, path: PathBuf, root: String) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu, _, _| file_path_items(menu, &view, &path, &root)
}

/// 「在访达中显示 / 在资源管理器中定位 / 复制路径 / 复制相对路径」四项(标签菜单与面包屑菜单共用)。差异标签的伪路径没有对应文件,不加。
fn file_path_items(menu: PopupMenu, view: &WeakEntity<IdeView>, path: &Path, root: &str) -> PopupMenu {
    if crate::ide_diff::is_diff(path) || crate::ide_untitled::is_untitled(path) {
        return menu;
    }
    let item = |label: &'static str, ic: IconName| PopupMenuItem::new(label).icon(ic);
    let p = path.to_path_buf();
    let finder = item("在访达中显示", IconName::FolderOpen).on_click(move |_, _, _| reveal_in_finder(&p));
    let (v, p) = (view.clone(), path.to_path_buf());
    let tree = item("在资源管理器中定位", IconName::ListTree).on_click(move |_, _, cx| {
        let _ = v.update(cx, |_, cx| cx.emit(crate::ide::IdeEvent::RevealInExplorer(p.clone())));
    });
    let p = path.to_path_buf();
    let copy = item("复制路径", IconName::Copy).on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(p.display().to_string())));
    let (p, r) = (path.to_path_buf(), root.to_string());
    let copy_rel = item("复制相对路径", IconName::Copy).on_click(move |_, _, cx| {
        let rel = p.strip_prefix(&r).unwrap_or(&p).display().to_string();
        cx.write_to_clipboard(ClipboardItem::new_string(rel))
    });
    menu.item(finder).item(tree).separator().item(copy).item(copy_rel)
}

/// 面包屑:项目内的相对路径,最后一段为文件名;保存失败等提示显示在右侧。
///
/// 参数:`extra` 放在提示与追溯 / 刷新按钮之间的附加控件(Markdown 的「预览 / 源码 / 分栏」切换),没有传 `None`。
pub fn breadcrumbs(path: &Path, root: &str, saved: bool, note: Option<String>, blame: Option<bool>, extra: Option<AnyElement>, cx: &mut Context<IdeView>) -> impl IntoElement {
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
    let segs: Vec<String> = match crate::ide_untitled::title(path) {
        Some(t) => vec![t],
        None => rel.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect(),
    };
    let last = segs.len().saturating_sub(1);
    // 每一段对应的绝对路径(从外到内):路径的祖先链倒数。差异标签的伪路径不可点
    let clickable = !crate::ide_diff::is_diff(path) && !crate::ide_untitled::is_untitled(path);
    let abs: Vec<PathBuf> = (0..segs.len()).map(|i| path.ancestors().nth(last - i).map(Path::to_path_buf).unwrap_or_default()).collect();
    let file = path.to_path_buf();
    let crumb_menu = crumb_menu(cx.entity().downgrade(), path.to_path_buf(), root.to_string());
    h_flex()
        .id("ide-breadcrumbs")
        .context_menu(crumb_menu)
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
            // 点某一段:弹出该段所在目录的内容(同级文件 / 目录),可搜索、进入子目录、切换文件
            let dir = abs[i].parent().map(Path::to_path_buf).filter(|_| clickable);
            let (cur, hover) = (file.clone(), t.list_hover);
            parts.push(
                div().id(SharedString::from(format!("crumb-seg-{i}"))).flex_none().whitespace_nowrap().px_1().rounded(px(4.)).when(i == last, |d| d.text_color(t.foreground))
                    .when_some(dir, |d, dir| {
                        d.cursor_pointer().hover(move |d| d.bg(hover)).on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                            let pos = ev.position();
                            this.open_crumb(dir.clone(), cur.clone(), point(pos.x - px(12.), pos.y + px(12.)), window, cx);
                        }))
                    })
                    .child(s)
                    .into_any_element(),
            );
            parts
        })))
        .when_some(note, |d, n| d.child(div().text_color(t.warning).child(n)))
        .when(!saved, |d| d.child(div().text_color(t.muted_foreground).child("未保存 · ⌘S")))
        .children(extra)
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
    let hints = [("快速打开文件", "⌘P"), ("命令面板", "⇧⌘P"), ("在项目中搜索", "⇧⌘F"), ("在文件中查找", "⌘F"), ("转到符号", "⇧⌘O"), ("显示 / 隐藏终端", "⌃`"), ("引用选中内容到对话", "⌘L"), ("显示 / 隐藏侧栏 / 对话栏", "⌘B / ⌥⌘B"), ("切换到 Agent", "⇧⌘E")];
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
