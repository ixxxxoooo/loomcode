//! 编辑器页:IDE 编辑器的显示与行为偏好(迷你地图、行号、折叠、换行、缩进、自动保存…),改动立即作用于已打开的文件。
//!
//! @author ixxxxoooo

use super::widgets::{group, note, page_header, row, rows, segmented, stepper, switch_row};
use crate::workbench::Workbench;
use gpui_kit::component::v_flex;
use gpui_kit::*;
use loom_config::AutoSave;

/// 渲染编辑器页。
pub fn render(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let e = wb.settings.editor.clone();
    v_flex().w_full()
        .child(page_header("编辑器", "IDE 窗口里文件编辑器的显示与行为;改动立即作用于已打开的文件", div(), cx))
        .child(group("迷你地图", rows![
            switch_row("ed-minimap", "显示迷你地图", "编辑器右侧的缩略图,带语法颜色、可点击或拖动滚动;编辑器过窄时自动隐藏", e.minimap, cx, |this, v, cx| this.update_settings(cx, |s| s.editor.minimap = v)),
            row("滑块", "视口滑块何时显示", segmented("ed-slider", vec![(false, "悬停时"), (true, "始终")], e.minimap_slider_always, cx,
                |this, v, cx| this.update_settings(cx, |s| s.editor.minimap_slider_always = v)), cx),
        ], cx))
        .child(group("显示", rows![
            switch_row("ed-lines", "行号", "", e.line_numbers, cx, |this, v, cx| this.update_settings(cx, |s| s.editor.line_numbers = v)),
            switch_row("ed-fold", "代码折叠", "在行号旁显示折叠按钮", e.folding, cx, |this, v, cx| this.update_settings(cx, |s| s.editor.folding = v)),
            switch_row("ed-wrap", "自动换行", "长行在视口边缘折行,而不是横向滚动", e.soft_wrap, cx, |this, v, cx| this.update_settings(cx, |s| s.editor.soft_wrap = v)),
            switch_row("ed-bracket", "括号配对高亮", "光标贴近括号时高亮与它配对的括号", e.bracket_match, cx, |this, v, cx| this.update_settings(cx, |s| s.editor.bracket_match = v)),
            switch_row("ed-syntax", "语法高亮", "之后新打开的文件生效;关闭可以让超大文件更流畅", e.syntax_highlight, cx, |this, v, cx| this.update_settings(cx, |s| s.editor.syntax_highlight = v)),
        ], cx))
        .child(group("编辑", rows![
            row("缩进宽度", "Tab 键缩进使用的列数", segmented("ed-tab", vec![(2u8, "2"), (4u8, "4"), (8u8, "8")], e.tab_size, cx,
                |this, v, cx| this.update_settings(cx, |s| s.editor.tab_size = v)), cx),
            row("自动保存", "关闭 / 停止输入 1 秒后 / 切换标签或窗口失焦时", segmented("ed-autosave", vec![(AutoSave::Off, "关闭"), (AutoSave::Delay, "延迟"), (AutoSave::Blur, "失焦")], e.auto_save, cx,
                |this, v, cx| this.update_settings(cx, |s| s.editor.auto_save = v)), cx),
            row("文件大小上限", "超过这个大小的文件不在编辑器里打开", stepper("ed-max", e.max_file_kb as i64, (256, 8192, 256), " KB", cx,
                |this, v, cx| this.update_settings(cx, |s| s.editor.max_file_kb = v as u32)), cx),
        ], cx))
        .child(note("缩进对齐线的显示由编辑器内置,暂不提供开关", cx))
        .into_any_element()
}
