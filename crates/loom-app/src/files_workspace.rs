//! 右栏「文件」标签的工作区(参考 OhMyAgent):左边是文件内容(在树里单击即原地打开,带 ← → 与路径面包屑),
//! 右边是可拖宽、可收起的资源管理器。IDE 窗口的左栏只用树本身(`files_view`),不走这里。
//!
//! @author ixxxxoooo

use crate::file_preview::FilePreview;
use crate::right_panel::RightPanel;
use crate::ui::{MONO, empty_state, icon};
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex, h_resizable, resizable_panel, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::path::PathBuf;

/// 树的默认宽度。
const TREE_W: f32 = 260.;
/// 导航栈最多保留的文件数。
const NAV_MAX: usize = 100;

impl RightPanel {
    /// 在左侧内容区原地打开文件,并记入 ← → 导航栈。
    ///
    /// 参数:`path` 文件绝对路径。
    pub fn open_in_place(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.nav.get(self.nav_ix) != Some(&path) {
            // 后退之后再打开新文件:丢掉前进记录
            self.nav.truncate(self.nav_ix + 1.min(self.nav.len()));
            self.nav.push(path.clone());
            if self.nav.len() > NAV_MAX {
                self.nav.remove(0);
            }
            self.nav_ix = self.nav.len() - 1;
        }
        self.show_current(window, cx);
    }

    /// 后退 / 前进一个文件。
    fn step_nav(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let next = if forward { self.nav_ix + 1 } else { self.nav_ix.wrapping_sub(1) };
        if next < self.nav.len() {
            self.nav_ix = next;
            self.show_current(window, cx);
        }
    }

    /// 按导航栈当前位置加载文件内容,并让树选中它。
    fn show_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.nav.get(self.nav_ix).cloned() else { return };
        self.files.reveal(&path);
        self.current = Some(FilePreview::load(path, window, cx));
        cx.emit(crate::right_panel::PanelEvent::StateChanged);
        cx.notify();
    }

    /// 清空文件内容与导航栈(切换项目时)。
    pub(crate) fn clear_files_workspace(&mut self) {
        self.current = None;
        self.nav.clear();
        self.nav_ix = 0;
    }

    /// 「文件」标签的内容:面包屑行 + 左内容 / 右资源管理器。
    pub(crate) fn files_workspace(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let (muted, fg, hover) = (t.muted_foreground, t.foreground, t.list_hover);
        let (can_back, can_fwd) = (self.nav_ix > 0, self.nav_ix + 1 < self.nav.len());
        let nav_btn = |id: &'static str, ic: IconName, on: bool, forward: bool| {
            div().id(id).p_1().rounded(px(4.))
                .when(on, |d| d.cursor_pointer().hover(move |d| d.bg(hover)).on_click(cx.listener(move |this, _, window, cx| this.step_nav(forward, window, cx))))
                .child(icon(ic, 13., if on { muted } else { muted.opacity(0.35) }))
        };
        let crumb = self.current.as_ref().map(|c| {
            let rel = c.path.strip_prefix(self.files.root_path()).unwrap_or(&c.path).to_string_lossy().to_string();
            let parts: Vec<String> = rel.split('/').map(String::from).collect();
            let last = parts.len().saturating_sub(1);
            h_flex().min_w_0().gap_1().items_center().text_size(px(12.)).font_family(MONO).overflow_hidden()
                .children(parts.into_iter().enumerate().flat_map(move |(i, p)| {
                    let mut v = Vec::new();
                    if i > 0 {
                        v.push(div().text_color(muted).child("›").into_any_element());
                    }
                    v.push(div().when(i < last, |d| d.text_color(muted)).when(i == last, |d| d.text_color(fg)).child(p).into_any_element());
                    v
                }))
        });
        let collapsed = self.tree_collapsed;
        let header = h_flex().h(px(30.)).flex_none().px_3().gap_1().items_center()
            .child(nav_btn("fw-back", IconName::ArrowLeft, can_back, false))
            .child(nav_btn("fw-fwd", IconName::ArrowRight, can_fwd, true))
            .child(div().w(px(4.)))
            .children(crumb)
            .child(div().flex_1())
            .child(
                div().id("fw-tree-toggle").p_1().rounded(px(4.)).cursor_pointer().hover(|d| d.bg(t.list_hover))
                    .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(if collapsed { "展开资源管理器" } else { "收起资源管理器" }).build(window, cx))
                    .on_click(cx.listener(|this, _, _, cx| { this.tree_collapsed = !this.tree_collapsed; cx.emit(crate::right_panel::PanelEvent::StateChanged); cx.notify(); }))
                    .child(icon(IconName::ListTree, 13., t.muted_foreground)),
            );
        let left = match &self.current {
            Some(c) => c.render(cx),
            None => empty_state(IconName::FileText, "选择一个文件", "在右侧资源管理器里单击文件,会在这里预览", cx).into_any_element(),
        };
        let tree = self.files_view(cx);
        v_flex().size_full()
            .child(header)
            .child(div().h(px(1.)).flex_none().bg(t.border))
            .child(
                div().flex_1().min_h_0().child(
                    h_resizable("files-split")
                        .child(resizable_panel().size_range(px(160.)..Pixels::MAX).child(div().size_full().child(left)))
                        .child(resizable_panel().size(px(TREE_W)).size_range(px(180.)..px(480.)).flex_none().visible(!collapsed).child(div().size_full().border_l_1().border_color(t.border).child(tree))),
                ),
            )
            .into_any_element()
    }
}
