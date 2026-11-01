//! 与 Claude Code 配置相关的公共部件与记忆页。
//!
//! @author ixxxxoooo

use super::widgets::page_title;
use crate::ui::{MONO, caption, pill};
use crate::workbench::Workbench;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Editor;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_config::Scope;

/// 来源层小标签。
pub fn scope_pill(scope: Scope, cx: &App) -> impl IntoElement {
    let t = cx.theme();
    pill(scope.label(), t.muted_foreground, t.muted)
}

/// 记忆页:编辑当前项目根目录的 CLAUDE.md。
pub fn memory(wb: &mut Workbench, window: &mut Window, cx: &mut Context<Workbench>) -> AnyElement {
    wb.ensure_memory(window, cx);
    let t = cx.theme().clone();
    let Some(doc) = wb.memory.as_ref() else {
        return v_flex().child(page_title("记忆")).child(caption("先打开一个项目会话", cx)).into_any_element();
    };
    let path = doc.path.display().to_string();
    let saved = doc.saved;
    v_flex().w_full().gap_2()
        .child(page_title("记忆(CLAUDE.md)"))
        .child(h_flex().gap_2().items_center()
            .child(div().flex_1().min_w_0().truncate().font_family(MONO).text_size(px(12.)).text_color(t.muted_foreground).child(path))
            .child(Button::new("memory-save").small().when(!saved, |b| b.primary()).when(saved, |b| b.ghost()).label(if saved { "已保存" } else { "保存" })
                .on_click(cx.listener(|this, _, _, cx| this.save_memory(cx)))))
        .child(div().w_full().h(px(460.)).rounded(px(8.)).border_1().border_color(t.border).overflow_hidden()
            .child(Editor::new(&doc.editor).bordered(false).appearance(false).size_full()))
        .into_any_element()
}
