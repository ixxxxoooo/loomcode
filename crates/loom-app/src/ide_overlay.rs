//! 编辑器上的浮层(右键菜单的结果面板):引用列表、重命名输入框、代码操作列表、差异对比。
//! 浮层是居中的卡片,点外部 / 按 Esc 关闭;同一时间只有一个。
//!
//! @author ixxxxoooo

use crate::diff::{DiffLine, render_flat};
use crate::ide::IdeView;
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::*;

/// 代码操作列表里的一项:标题 + 服务器给的原始 JSON(执行时需要)。
#[derive(Debug, Clone)]
pub struct ActionItem {
    pub title: String,
    pub raw: serde_json::Value,
}

/// 浮层内容。
pub enum Overlay {
    Refs(crate::ide_peek::PeekState),
    /// 重命名:`local` 为真是「更改所有匹配项」(只改当前文件里的整词匹配),否则走语言服务器的重命名
    Rename { input: Entity<InputState>, local: bool, old: String },
    Actions { title: &'static str, items: Vec<ActionItem> },
    Diff { title: String, lines: Vec<DiffLine>, lang: &'static str },
}

impl IdeView {
    /// 显示浮层(替换已有的)。
    pub(crate) fn set_overlay(&mut self, overlay: Overlay, cx: &mut Context<Self>) {
        self.overlay = Some(overlay);
        cx.notify();
    }

    /// 关闭浮层,焦点还给编辑器。
    pub(crate) fn close_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.overlay.take().is_some() {
            self.overlay_sub = None;
            self.focus_active(window, cx);
            cx.notify();
        }
    }

    /// 浮层元素(没有浮层返回 `None`)。
    pub(crate) fn overlay_element(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let o = self.overlay.as_ref()?;
        let t = cx.theme().clone();
        let weak = cx.weak_entity();
        let (title, body): (String, AnyElement) = match o {
            Overlay::Refs(st) => crate::ide_peek::peek_body(st, cx),
            Overlay::Actions { title, items } => {
                let rows = items.iter().enumerate().map(|(i, a)| {
                    h_flex().id(("ov-act", i)).h(px(28.)).px_2().items_center().rounded(px(4.)).cursor_pointer().hover(|d| d.bg(t.list_hover)).text_size(px(12.5))
                        .on_click(cx.listener(move |this, _, window, cx| this.run_action_item(i, window, cx)))
                        .child(div().truncate().child(a.title.clone()))
                }).collect::<Vec<_>>();
                (title.to_string(), div().id("ov-list").max_h(px(420.)).overflow_y_scroll().children(rows).into_any_element())
            }
            Overlay::Rename { input, local, old } => (
                if *local { format!("更改所有匹配项:{old}") } else { format!("重命名符号:{old}") },
                v_flex().gap_1().child(Input::new(input)).child(div().text_size(px(11.)).text_color(t.muted_foreground).child(if *local { "回车确认,把当前文件里所有同名的整词替换成新名称(不经过语言服务器)" } else { "回车确认,由语言服务器改写所有引用(可能涉及多个文件)" })).into_any_element(),
            ),
            Overlay::Diff { title, lines, lang } => {
                let body = if lines.iter().all(|l| l.tag == similar::ChangeTag::Equal) { div().p_3().text_size(px(12.)).text_color(t.muted_foreground).child("没有差异").into_any_element() } else { div().id("ov-diff").max_h(px(480.)).overflow_y_scroll().child(render_flat(lines, 3000, lang, cx)).into_any_element() };
                (title.clone(), body)
            }
        };
        let width = match o { Overlay::Diff { .. } => 900., Overlay::Refs(_) => 1040., _ => 560. };
        let esc = canvas(|b, _, _| b, move |_, _, window, _| {
            let w = weak.clone();
            window.on_key_event(move |e: &KeyDownEvent, _, window, cx| {
                let key = e.keystroke.key.clone();
                let _ = w.update(cx, |this, cx| match key.as_str() {
                    "escape" => this.close_overlay(window, cx),
                    // 引用预览窗:↑↓ 切换、回车打开
                    "up" if matches!(this.overlay, Some(Overlay::Refs(_))) => this.peek_move(-1, cx),
                    "down" if matches!(this.overlay, Some(Overlay::Refs(_))) => this.peek_move(1, cx),
                    "enter" => {
                        if let Some(Overlay::Refs(st)) = &this.overlay {
                            let i = st.selected;
                            this.peek_open(i, window, cx);
                        }
                    }
                    _ => {}
                });
            });
        }).absolute().size_full();
        Some(
            deferred(
                div().id("ide-overlay").absolute().inset_0().flex().justify_center().pt(px(70.))
                    .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| this.close_overlay(window, cx)))
                    .child(
                        v_flex().w(px(width)).max_w_full().max_h(px(560.)).p_2().gap_1().rounded(px(10.)).border_1().border_color(t.border).bg(t.popover).shadow_lg().occlude()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(esc)
                            .child(
                                h_flex().px_2().h(px(26.)).items_center().justify_between()
                                    .child(div().text_size(px(12.5)).font_weight(FontWeight::MEDIUM).truncate().child(title))
                                    .child(div().id("ov-close").size(px(20.)).flex().items_center().justify_center().rounded(px(4.)).cursor_pointer().hover(|d| d.bg(t.list_hover))
                                        .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))).child(crate::ui::icon(IconName::X, 12., t.muted_foreground))),
                            )
                            .child(body),
                    ),
            ).into_any_element(),
        )
    }
}
