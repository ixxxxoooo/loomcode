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
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::path::PathBuf;

/// 行高。
const ROW_H: f32 = 28.;
/// 每级缩进。
const INDENT: f32 = 16.;
/// 大纲区展开时的高度。
const OUTLINE_H: f32 = 220.;

impl RightPanel {
    /// 资源管理器视图。
    pub(crate) fn files_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let root = self.files.root_path().to_path_buf();
        let root_name = root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let rows = self.files.rows();
        let (folder_open, outline_open) = (self.folder_open, self.outline_open);
        // 区块标题行:左边是「展开箭头 + 名称」(点它折叠 / 展开),右边是操作按钮
        let toolbar = h_flex().h(px(28.)).px_2().gap_1().items_center().text_size(px(11.)).text_color(t.muted_foreground)
            .child(
                h_flex().id("ft-folder-head").flex_1().min_w_0().h_full().gap_1().items_center().cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| { this.folder_open = !this.folder_open; cx.notify(); }))
                    .child(icon(if folder_open { IconName::ChevronDown } else { IconName::ChevronRight }, 12., t.muted_foreground))
                    .child(div().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).child(root_name.to_uppercase())),
            )
            .children([
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
        // 大纲区标题行
        let outline_head = h_flex().id("ft-outline-head").h(px(28.)).px_2().gap_1().items_center().flex_none().cursor_pointer().border_t_1().border_color(t.border)
            .text_size(px(11.)).text_color(t.muted_foreground)
            .on_click(cx.listener(|this, _, _, cx| { this.outline_open = !this.outline_open; cx.notify(); }))
            .child(icon(if outline_open { IconName::ChevronDown } else { IconName::ChevronRight }, 12., t.muted_foreground))
            .child(div().font_weight(FontWeight::SEMIBOLD).child("大纲"));
        v_flex().size_full()
            .child(toolbar)
            .when(folder_open, |d| d.child(v_flex().flex_1().min_h_0().child(body)))
            // 项目文件夹收起后,大纲占满剩下的高度;否则固定一块高度放在树的下面
            .when_some(self.outline.clone(), |d, outline| {
                d.child(outline_head).when(outline_open, |d| {
                    d.child(div().when(folder_open, |d| d.h(px(OUTLINE_H)).flex_none()).when(!folder_open, |d| d.flex_1()).min_h_0().child(outline))
                })
            })
            .into_any_element()
    }
}

/// 项目根目录的显示文本。
fn root_display(files: &crate::file_tree::FileTree) -> String {
    files.root_path().display().to_string()
}
