//! 「文件」面板(资源管理器)的界面:工具条 + 铺满高度的文件树。
//! 样式参考 OhMyAgent:28px 行高、圆角内缩的选中 / 悬停底、Git 状态着色;单击预览、双击固定,可拖放。
//!
//! @author ixxxxoooo

use crate::file_dnd::DraggedPath;
use crate::file_icons::file_icon_img;
use crate::file_ops::row_menu;
use crate::git_status::GitMark;
use crate::right_panel::{PanelEvent, RightPanel};
use crate::ui::{MONO, empty_state, icon};
use gpui_kit::assets::IconName;
use crate::explorer_sections::Section;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::Sizable;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::path::PathBuf;

/// 行高。
const ROW_H: f32 = 28.;
/// 每级缩进。
const INDENT: f32 = 16.;
/// 大纲 / 时间线区展开时的高度。
const SECTION_H: f32 = 200.;

impl RightPanel {
    /// 资源管理器视图。
    pub(crate) fn files_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let root = self.files.root_path().to_path_buf();
        let root_name = root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let rows = self.files.rows();
        let folder_actions = h_flex().gap_1().items_center().children([
            ("ft-refresh", IconName::RefreshCw, "刷新", 0u8),
            ("ft-new-file", IconName::FilePlus, "新建文件", 1),
            ("ft-new-dir", IconName::FolderPlus, "新建文件夹", 2),
            ("ft-collapse", IconName::ChevronsDownUp, "全部折叠", 3),
        ].map(|(id, ic, tip, kind)| {
            div().id(id).p_1().rounded(px(4.)).cursor_pointer().hover(|d| d.bg(t.list_hover))
                .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tip).build(window, cx))
                .on_click(cx.listener(move |this, _, window, cx| {
                    let dir = this.files.target_dir();
                    match kind {
                        0 => this.reload_files(cx),
                        1 => this.new_entry_dialog(dir, false, window, cx),
                        2 => this.new_entry_dialog(dir, true, window, cx),
                        _ => this.files.collapse_all(),
                    }
                    cx.notify();
                }))
                .child(icon(ic, 13., t.muted_foreground))
        }));
        // 「打开的编辑器」标题行的操作:保存全部 / 关闭全部
        let editor_actions = h_flex().gap_1().items_center().children([
            ("oe-save-all", IconName::SaveAll, "全部保存", true),
            ("oe-close-all", IconName::ListX, "全部关闭", false),
        ].map(|(id, ic, tip, save)| {
            div().id(id).p_1().rounded(px(4.)).cursor_pointer().hover(|d| d.bg(t.list_hover))
                .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tip).build(window, cx))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(ed) = this.src.clone() {
                        ed.update(cx, |v, cx| if save { v.save_all(cx) } else { v.close_all(window, cx) });
                    }
                }))
                .child(icon(ic, 13., t.muted_foreground))
        }));
        let st = self.sections.clone();
        if let Some(tl) = &self.timeline {
            let on = st.open(Section::Timeline);
            tl.update(cx, |v, cx| v.set_enabled(on, cx));
        }
        let accent = t.primary.opacity(0.18);
        let root_for_drop = root.clone();
        let root_for_ext = root.clone();
        let tree = v_flex().id("file-tree-scroll").flex_1().min_h_0().w_full().overflow_y_scroll().py_1()
            // 空白处:拖进来 = 放进项目根目录
            .drag_over::<DraggedPath>(move |s, _, _, _| s.bg(accent))
            .on_drop(cx.listener(move |this, d: &DraggedPath, window, cx| this.drop_into(&[d.path.clone()], &root_for_drop, false, window, cx)))
            .on_drop(cx.listener(move |this, p: &ExternalPaths, window, cx| this.drop_into(p.paths(), &root_for_ext, true, window, cx)))
            .children(rows.into_iter().enumerate().map(|(ix, (depth, node))| {
                let (selected, open) = (self.files.is_selected(&node.path), node.is_dir && self.files.is_open(&node.path));
                let (path, is_dir) = (node.path.clone(), node.is_dir);
                let mark = self.files.git_mark(&node.path);
                let color = match mark {
                    Some(GitMark::Modified) => t.warning,
                    Some(GitMark::Added) => t.success,
                    Some(GitMark::Deleted) => t.danger,
                    None => t.foreground,
                };
                // 放置目标:文件夹本身,或文件所在的文件夹
                let target: PathBuf = if is_dir { node.path.clone() } else { node.path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| root.clone()) };
                let (t_move, t_copy) = (target.clone(), target);
                let rel = node.path.strip_prefix(&root).map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|_| node.path.to_string_lossy().to_string());
                let drag = DraggedPath { path: node.path.clone(), rel };
                div().id(SharedString::from(format!("ft-{ix}"))).w_full().px_1().child(
                    h_flex().id(SharedString::from(format!("ft-row-{ix}"))).h(px(ROW_H)).w_full().pr_2().gap_1p5().items_center().cursor_pointer().rounded(px(6.)).text_size(px(13.))
                        .pl(px(8. + depth as f32 * INDENT))
                        .when(selected, |d| d.bg(t.list_active))
                        .hover(|d| d.bg(t.list_hover))
                        .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| {
                            if is_dir {
                                // 目录只在单击时展开 / 收起,双击的第二下不再翻回去
                                if ev.click_count() < 2 {
                                    this.files.toggle(&path);
                                    cx.notify();
                                }
                            } else if ev.click_count() >= 2 {
                                cx.emit(PanelEvent::OpenFilePinned(path.clone()));
                            } else {
                                cx.emit(PanelEvent::OpenFile(path.clone()));
                            }
                        }))
                        .on_drag(drag, |d, _, _, cx| cx.new(|_| d.clone()))
                        .drag_over::<DraggedPath>(move |s, _, _, _| s.bg(accent))
                        .on_drop(cx.listener(move |this, d: &DraggedPath, window, cx| this.drop_into(&[d.path.clone()], &t_move, false, window, cx)))
                        .on_drop(cx.listener(move |this, p: &ExternalPaths, window, cx| this.drop_into(p.paths(), &t_copy, true, window, cx)))
                        .context_menu(row_menu(cx.entity().downgrade(), node.path.clone(), is_dir, self.files.root_path().to_path_buf()))
                        .child(div().w(px(12.)).flex_none().when(is_dir, |d| d.child(icon(if open { IconName::ChevronDown } else { IconName::ChevronRight }, 12., t.muted_foreground))))
                        .child(file_icon_img(&node.name, is_dir, open, 16.))
                        .child(div().flex_1().min_w_0().truncate().text_color(color).child(node.name))
                        .when_some(mark.filter(|_| !is_dir), |d, m| {
                            d.child(div().text_size(px(11.)).font_family(MONO).text_color(color).child(match m {
                                GitMark::Modified => "M",
                                GitMark::Added => "U",
                                GitMark::Deleted => "D",
                            }))
                        })
                        .when(mark.is_some() && is_dir, |d| d.child(div().size(px(6.)).rounded_full().bg(color))),
                )
            }));
        let body = if self.files.rows().is_empty() {
            empty_state(IconName::Folder, "目录为空", &root_display(&self.files), cx).into_any_element()
        } else {
            tree.context_menu(crate::file_ops::empty_menu(cx.entity().downgrade(), root.clone())).into_any_element()
        };
        let mut actions: Vec<(Section, AnyElement)> = vec![(Section::Folder, folder_actions.into_any_element()), (Section::OpenEditors, editor_actions.into_any_element())];
        let mut col = v_flex().size_full();
        let folder_open = st.open(Section::Folder);
        let mut body = Some(body);
        for sec in Section::ALL {
            if !st.visible(sec) {
                continue;
            }
            let extra = actions.iter().position(|(s, _)| *s == sec).map(|i| actions.remove(i).1);
            col = col.child(self.section_head(sec, &root_name, extra, st.first_visible() == Some(sec), cx));
            if !st.open(sec) {
                continue;
            }
            col = match sec {
                Section::OpenEditors => col.children(self.open_editors.clone()),
                Section::Folder => col.child(v_flex().flex_1().min_h(px(80.)).child(body.take().unwrap_or_else(|| div().into_any_element()))),
                Section::Outline => col.children(self.outline.clone().map(|o| Self::fixed_or_fill(o, folder_open))),
                Section::Timeline => col.children(self.timeline.clone().map(|o| Self::fixed_or_fill(o, folder_open))),
            };
        }
        col.into_any_element()
    }

    /// 大纲 / 时间线的容器:文件夹区展开时给固定高度(空间不够会收缩),否则平分剩余高度。
    fn fixed_or_fill(view: impl IntoElement, folder_open: bool) -> impl IntoElement {
        div().when(folder_open, |d| d.h(px(SECTION_H)).min_h(px(48.))).when(!folder_open, |d| d.flex_1().min_h_0()).child(view)
    }

    /// 区块标题行:展开箭头 + 名称(点它折叠 / 展开)、该区块的操作按钮,第一个可见区块还带 `···` 菜单(显示 / 隐藏区块)。
    fn section_head(&self, sec: Section, root_name: &str, actions: Option<AnyElement>, first: bool, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let open = self.sections.open(sec);
        let label = if sec == Section::Folder { root_name.to_uppercase() } else { sec.label().to_string() };
        let weak = cx.entity().downgrade();
        let st = self.sections.clone();
        let menu = move |menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>| {
            let mut menu = menu;
            for s in Section::ALL {
                let (w, on) = (weak.clone(), st.visible(s));
                // 至少留一个区块,否则这个菜单也没了
                let only = on && Section::ALL.iter().filter(|x| st.visible(**x)).count() == 1;
                menu = menu.item(PopupMenuItem::new(s.label()).checked(on).disabled(only).on_click(move |_, _, cx| {
                    let _ = w.update(cx, |this, cx| {
                        this.sections.toggle_hidden(s);
                        cx.emit(PanelEvent::StateChanged);
                        cx.notify();
                    });
                }));
            }
            menu
        };
        h_flex().h(px(28.)).px_2().gap_1().items_center().flex_none().text_size(px(11.)).text_color(t.muted_foreground)
            .when(!first, |d| d.border_t_1().border_color(t.border))
            .child(
                h_flex().id(SharedString::from(format!("sec-head-{}", sec.key()))).flex_1().min_w_0().h_full().gap_1().items_center().cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.sections.toggle_collapsed(sec);
                        cx.emit(PanelEvent::StateChanged);
                        cx.notify();
                    }))
                    .child(icon(if open { IconName::ChevronDown } else { IconName::ChevronRight }, 12., t.muted_foreground))
                    .child(div().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).child(label)),
            )
            .children(actions.filter(|_| open))
            .when(first, |d| d.child(Button::new("sec-more").xsmall().ghost().icon(IconName::Ellipsis).tooltip("显示 / 隐藏区块").dropdown_menu(menu)))
            .into_any_element()
    }
}

/// 项目根目录的显示文本。
fn root_display(files: &crate::file_tree::FileTree) -> String {
    files.root_path().display().to_string()
}
