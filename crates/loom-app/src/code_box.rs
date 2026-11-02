//! 会话流里的代码块 / 长输出:限高滚动容器。
//!
//! 做法参考 ohmyagent 的 `CollapsibleOutput`(也是 Cursor / Claude Code / Zed 的通行做法):
//! 行数不超过 [`FOLD_LINES`] 直接完整显示;更长的限高([`COLLAPSED_H`])并在内部滚动,
//! 底部给出「展开 · 共 N 行」,展开后仍有上限([`EXPANDED_H`]),再长就内部滚动;
//! 流式输出时自动跟随末尾,复制按钮始终复制完整内容。
//! 内层滚动到头后,滚轮会继续交给外层会话列表(gpui 只在偏移变化时才吞掉滚轮事件)。
//!
//! @author ixxxxoooo

use crate::ui::{MONO, icon};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// 超过这个行数才启用限高。
pub const FOLD_LINES: usize = 12;
/// 收起时的最大高度。
pub const COLLAPSED_H: f32 = 196.;
/// 展开后的最大高度。
pub const EXPANDED_H: f32 = 480.;
/// 流式输出时超过这个字节数就不再逐帧做语法高亮(整段重新高亮太慢)。
const STREAM_HL_MAX: usize = 6_000;

thread_local! {
    /// 各限高容器的滚动句柄(按元素 id),流式输出时用来贴底跟随。
    static HANDLES: RefCell<HashMap<SharedString, ScrollHandle>> = RefCell::new(HashMap::new());
}

/// 取(或新建)某个容器的滚动句柄。
pub(crate) fn handle_for(id: &SharedString) -> ScrollHandle {
    HANDLES.with(|h| {
        let mut h = h.borrow_mut();
        // 会话很长时句柄会越攒越多,到上限就整体清掉(只丢滚动位置,下次渲染自动重建)
        if h.len() > 512 && !h.contains_key(id) {
            h.clear();
        }
        h.entry(id.clone()).or_insert_with(ScrollHandle::new).clone()
    })
}

/// 展开状态与切换回调(由调用方持有状态:会话的展开集合)。
#[derive(Clone)]
pub struct Fold {
    /// 当前是否展开
    pub expanded: bool,
    /// 点「展开 / 收起」时调用
    pub on_toggle: Rc<dyn Fn(&mut Window, &mut App)>,
    /// 收起时的最大高度;`None` 用默认的 [`COLLAPSED_H`](编辑 Diff 用更小的预览高度)
    pub collapsed_h: Option<f32>,
}

/// 这段文本是否需要限高。
///
/// 参数:`total_lines` 总行数。
pub fn needs_fold(total_lines: usize) -> bool {
    total_lines > FOLD_LINES
}

/// 当前应使用的最大高度。
///
/// 参数:`expanded` 是否展开。
pub fn max_height(expanded: bool) -> f32 {
    if expanded { EXPANDED_H } else { COLLAPSED_H }
}

/// 限高滚动容器:短内容原样返回,长内容限高滚动并在底部加展开 / 收起条。
///
/// 参数:`id` 元素 id(同一内容稳定不变);`body` 正文元素;`total_lines` 总行数;
/// `fold` 展开状态(`None` 表示调用方不支持展开,不限高);`follow_tail` 流式输出中,贴底跟随。
pub fn fold_body(id: SharedString, body: impl IntoElement, total_lines: usize, fold: Option<&Fold>, follow_tail: bool, cx: &App) -> AnyElement {
    let Some(fold) = fold.filter(|_| needs_fold(total_lines)) else {
        return body.into_any_element();
    };
    let t = cx.theme();
    let handle = handle_for(&id);
    if follow_tail {
        handle.scroll_to_bottom();
    }
    let on_toggle = fold.on_toggle.clone();
    let label = if fold.expanded { "收起".to_string() } else { format!("展开 · 共 {total_lines} 行") };
    v_flex().w_full()
        .child(div().id(id.clone()).w_full().max_h(px(if fold.expanded { EXPANDED_H } else { fold.collapsed_h.unwrap_or(COLLAPSED_H) })).overflow_y_scroll().track_scroll(&handle).child(body))
        .child(
            h_flex().id(SharedString::from(format!("{id}-fold"))).h(px(22.)).px_3().gap_1().items_center().cursor_pointer()
                .text_size(px(11.)).text_color(t.muted_foreground).hover(|d| d.text_color(t.foreground))
                .on_click(move |_, window, cx| on_toggle(window, cx))
                .child(icon(if fold.expanded { IconName::ChevronsDownUp } else { IconName::ChevronsUpDown }, 11., t.muted_foreground))
                .child(label),
        )
        .into_any_element()
}

/// 助手正文里的围栏代码块:顶部语言名与复制按钮,正文语法高亮,过长限高。
///
/// 参数:`id` 稳定 id;`lang` 围栏语言(空串不着色);`code` 代码;`streaming` 围栏尚未闭合(流式输出中);
/// `fold` 展开状态。
pub fn render_code(id: SharedString, lang: &str, code: &str, streaming: bool, fold: Option<&Fold>, cx: &App) -> AnyElement {
    let t = cx.theme();
    let code = code.trim_end_matches('\n');
    let total = code.lines().count().max(1);
    let hl = if streaming && code.len() > STREAM_HL_MAX { Vec::new() } else { crate::code_hl::styles(code, lang, cx) };
    let body = div().w_full().px_3().py_2().font_family(MONO).text_size(px(12.)).text_color(t.foreground).child(StyledText::new(code.to_string()).with_highlights(hl));
    let copy_text = code.to_string();
    v_flex().w_full().rounded(px(8.)).border_1().border_color(t.border).bg(t.muted).overflow_hidden()
        .child(
            h_flex().h(px(26.)).px_3().gap_2().items_center()
                .child(div().flex_1().min_w_0().truncate().text_size(px(11.)).text_color(t.muted_foreground).child(if lang.is_empty() { "代码".to_string() } else { lang.to_string() }))
                .child(
                    Button::new(SharedString::from(format!("{id}-copy"))).xsmall().ghost().icon(IconName::Copy).tooltip("复制")
                        .on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone()))),
                ),
        )
        .child(fold_body(id, body, total, fold, streaming, cx))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 行数阈值与高度档位() {
        assert!(!needs_fold(FOLD_LINES));
        assert!(needs_fold(FOLD_LINES + 1));
        assert_eq!(max_height(false), COLLAPSED_H);
        assert_eq!(max_height(true), EXPANDED_H);
        assert!(EXPANDED_H > COLLAPSED_H);
    }
}
