//! 提示词页:追加系统提示词(`--append-system-prompt`)与 CLAUDE.md 开关,都是客户端偏好,作用于所有新会话。
//!
//! @author ixxxxoooo

use super::widgets::{block_row, group, note, page_header, row, rows};
use crate::theme::R_MD;
use crate::workbench::Workbench;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Textarea;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex};
use gpui_kit::*;

/// 渲染提示词页。
pub fn render(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let saved = wb.settings.append_prompt.trim().to_string();
    let draft = wb.forms().fields.prompt.read(cx).value().trim().to_string();
    let dirty = draft != saved;
    let editor = div().flex().flex_col().w_full().gap_2()
        .child(div().w_full().h(px(140.)).p_2().rounded(px(R_MD)).border_1().border_color(if dirty { t.primary.opacity(0.5) } else { t.border }).bg(t.background)
            .child(Textarea::new(&wb.forms().fields.prompt).appearance(false).bordered(false).size_full()))
        .child(h_flex().w_full().gap_2().items_center()
            .child(div().flex_1().text_size(px(11.)).text_color(t.muted_foreground).child(format!("{} 字{}", draft.chars().count(), if dirty { " · 未保存" } else { "" })))
            .child(Button::new("prompt-clear").small().ghost().label("清空").on_click(cx.listener(|this, _, window, cx| {
                this.forms().fields.prompt.clone().update(cx, |i, cx| i.set_value(String::new(), window, cx));
                this.save_append_prompt(cx);
            })))
            .child(Button::new("save-prompt").small().primary().label("保存").on_click(cx.listener(|this, _, _, cx| this.save_append_prompt(cx)))));
    gpui_kit::component::v_flex().w_full()
        .child(page_header("提示词", "客户端级别的提示词设置,对之后新建或重新连接的会话生效", div(), cx))
        .child(group("追加系统提示词", rows![
            block_row("内容", "以 --append-system-prompt 传给 claude,会与多根工作区说明合并", editor, cx),
        ], cx))
        .child(group("记忆文件", rows![
            row("加载 CLAUDE.md", "关闭时注入 CLAUDE_CODE_DISABLE_CLAUDE_MDS=1,新会话不读任何记忆文件", Switch::new("sw-load-md").checked(wb.settings.load_claude_md)
                .on_click(cx.listener(|this, c: &bool, _, cx| { let v = *c; this.update_settings(cx, |s| s.load_claude_md = v) })), cx),
        ], cx))
        .child(note("项目与用户的记忆文件在「自定义 → 规则」里编辑", cx))
        .into_any_element()
}
