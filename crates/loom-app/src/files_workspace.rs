//! 右栏「文件」标签的工作区:左边嵌入全工作台唯一的编辑器(`IdeView`,与 IDE 完全同一份代码与状态,
//! 标签、预览标签、面包屑、Markdown 预览、迷你地图都一样,宽度不够时迷你地图自动隐藏),
//! 右边是可拖宽、可收起的资源管理器。树里单击打开预览标签、双击固定。
//! IDE 窗口的左栏只用树本身(`files_view`),不走这里。
//!
//! @author ixxxxoooo

use crate::right_panel::{PanelEvent, RightPanel};
use crate::ui::{empty_state, icon};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::Button;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, h_resizable, resizable_panel, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// 树的默认宽度。
const TREE_W: f32 = 260.;

impl RightPanel {
    /// 按编辑器的文件 / 行位置历史后退或前进,并在树里定位。
    fn step_nav(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ed) = self.editor.clone() else { return };
        if let Some(path) = ed.update(cx, |v, cx| v.navigate(forward, window, cx)) {
            self.files.reveal(&path);
            cx.notify();
        }
    }

    /// 「文件」标签的内容:导航行 + 左编辑器 / 右资源管理器。
    pub(crate) fn files_workspace(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let (muted, hover) = (t.muted_foreground, t.list_hover);
        let (can_back, can_fwd) = self.editor.as_ref().map_or((false, false), |e| {
            let nav = &e.read(cx).nav;
            (nav.can_back(), nav.can_forward())
        });
        let nav_btn = |id: &'static str, ic: IconName, on: bool, forward: bool| {
            div().id(id).p_1().rounded(px(4.))
                .when(on, |d| d.cursor_pointer().hover(move |d| d.bg(hover)).on_click(cx.listener(move |this, _, window, cx| this.step_nav(forward, window, cx))))
                .child(icon(ic, 13., if on { muted } else { muted.opacity(0.35) }))
        };
        let collapsed = self.tree_collapsed;
        let header = h_flex().h(px(28.)).flex_none().px_3().gap_1().items_center()
            .child(nav_btn("fw-back", IconName::ArrowLeft, can_back, false))
            .child(nav_btn("fw-fwd", IconName::ArrowRight, can_fwd, true))
            .child(div().flex_1())
            .child(
                div().id("fw-tree-toggle").p_1().rounded(px(4.)).cursor_pointer().hover(|d| d.bg(t.list_hover))
                    .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(if collapsed { "展开资源管理器" } else { "收起资源管理器" }).build(window, cx))
                    .on_click(cx.listener(|this, _, _, cx| { this.tree_collapsed = !this.tree_collapsed; cx.emit(PanelEvent::StateChanged); cx.notify(); }))
                    .child(icon(IconName::ListTree, 13., t.muted_foreground)),
            );
        let left = match (&self.editor, self.editor_elsewhere) {
            (_, true) => v_flex().size_full().items_center().justify_center().gap_2()
                .child(empty_state(IconName::ExternalLink, "编辑器在独立 IDE 窗口中", "为避免同一个编辑器在两个窗口里同时显示,文件在 IDE 窗口里编辑", cx))
                .child(Button::new("fw-open-ide").small().outline().label("切到 IDE 窗口").on_click(cx.listener(|_, _, _, cx| cx.emit(PanelEvent::OpenIdeWindow))))
                .into_any_element(),
            (Some(ed), false) => div().size_full().child(ed.clone()).into_any_element(),
            (None, false) => empty_state(IconName::FileText, "选择一个文件", "在右侧资源管理器里单击文件,会在这里打开", cx).into_any_element(),
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
