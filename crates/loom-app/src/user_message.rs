//! 会话流里的用户消息(参考 OhMyAgent):圆角卡片 + 72px 图片缩略图 + 去掉 `[Image #N]` 占位符的正文;
//! 悬停右下角出现「复制 / 编辑」,点卡片或编辑图标就地展开编辑器,「重新发送」作为新消息发出(原消息与回复保留)。
//!
//! @author ixxxxoooo

use crate::composer_send::send_button;
use crate::model::SessionModel;
use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::path::Path;

/// 超过这个行数的用户消息默认折叠。
const FOLD_LINES: usize = 6;
/// 缩略图边长。
const THUMB: f32 = 72.;

/// 去掉粘贴图片时留在正文里的占位符:`[Image #22]`、`[Image: source: …]`,并清理多余空白。
///
/// 参数:`text` 原始正文。返回:适合显示与编辑的正文。
pub fn display_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("[Image") {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        // 只处理 `[Image #数字]` 与 `[Image: …]` 两种;其它以 [Image 开头的普通文字原样保留
        let is_token = tail.starts_with("[Image #") || tail.starts_with("[Image:");
        match (is_token, tail.find(']')) {
            (true, Some(end)) => rest = &tail[end + 1..],
            _ => {
                out.push_str("[Image");
                rest = &tail["[Image".len()..];
            }
        }
    }
    out.push_str(rest);
    // 每行去掉行尾空白,首尾空行去掉
    let lines: Vec<&str> = out.lines().map(str::trim_end).collect();
    lines.join("\n").trim().to_string()
}

/// 渲染一条用户消息:编辑中显示编辑卡,否则显示消息卡。
pub fn render_user(session: &Entity<SessionModel>, id: &str, text: &str, images: &[String], files: &[String], cx: &App) -> AnyElement {
    let editing = session.read(cx).editing.as_ref().filter(|e| e.id == id).map(|e| e.input.clone());
    match editing {
        Some(input) => render_editor(session, &input, images, cx),
        None => render_card(session, id, text, images, files, cx),
    }
}

/// 图片缩略图:本地文件存在就画图(点击预览大图),否则(演示数据)退回文件名 chip。
fn thumb(name: &str, cx: &App) -> AnyElement {
    let t = cx.theme();
    if Path::new(name).is_file() {
        let label = Path::new(name).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        crate::image_viewer::thumbnail(&format!("u-{name}"), name, &label, THUMB, None, cx)
    } else {
        h_flex().gap_1().items_center().px_2().h(px(22.)).rounded(px(6.)).bg(t.background).border_1().border_color(t.border)
            .child(icon(IconName::Image, 12., t.muted_foreground))
            .child(div().text_size(px(12.)).child(name.to_string()))
            .into_any_element()
    }
}

/// 文件引用 chip。
fn file_chip(name: &str, cx: &App) -> AnyElement {
    let t = cx.theme();
    h_flex().gap_1().items_center().px_2().h(px(22.)).rounded(px(6.)).bg(t.background).border_1().border_color(t.border)
        .child(icon(IconName::FileText, 12., t.muted_foreground))
        .child(div().text_size(px(12.)).child(name.to_string()))
        .into_any_element()
}

/// 消息卡:附件行 + 正文(长文折叠)+ 悬停操作。
fn render_card(session: &Entity<SessionModel>, id: &str, text: &str, images: &[String], files: &[String], cx: &App) -> AnyElement {
    let t = cx.theme();
    let text = display_text(text);
    let lines = text.lines().count();
    let long = lines > FOLD_LINES;
    let folded = session.read(cx).is_expanded(id);
    let shown: String = if long && !folded { text.lines().take(FOLD_LINES).collect::<Vec<_>>().join("\n") } else { text.clone() };
    let group = SharedString::from(format!("umsg-{id}"));
    let (s_fold, id_fold) = (session.clone(), id.to_string());
    let (s_edit, id_edit, text_edit) = (session.clone(), id.to_string(), text.clone());
    let (s_retry, id_retry) = (session.clone(), id.to_string());
    let (s_click, id_click, text_click) = (session.clone(), id.to_string(), text.clone());
    let copy_text = text.clone();
    let icon_btn = |bid: String, ic: IconName, tip: &'static str| {
        div().id(SharedString::from(bid)).size(px(24.)).flex().items_center().justify_center().rounded(px(6.)).cursor_pointer()
            .hover(|d| d.bg(t.foreground.opacity(0.08)))
            .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tip).build(window, cx))
            .child(icon(ic, 12., t.muted_foreground))
    };
    v_flex().id(SharedString::from(format!("umsg-card-{id}"))).group(group.clone()).relative().w_full().gap_2().px(px(14.)).py(px(10.)).pr(px(76.)).rounded(px(12.)).bg(t.muted).cursor_pointer()
        .on_click(move |_, window, cx| {
            let (id, text) = (id_click.clone(), text_click.clone());
            s_click.update(cx, |m, cx| m.begin_edit(&id, &text, window, cx));
        })
        .when(!images.is_empty() || !files.is_empty(), |d| {
            d.child(h_flex().flex_wrap().gap_1p5().children(images.iter().map(|n| thumb(n, cx))).children(files.iter().map(|n| file_chip(n, cx))))
        })
        .when(!shown.is_empty(), |d| d.child(div().text_size(px(14.)).child(shown.clone())))
        .when(long, |d| {
            d.child(
                div().id(SharedString::from(format!("user-fold-{id}"))).text_size(px(12.)).text_color(t.primary).cursor_pointer()
                    .on_click(move |_, _, cx| { cx.stop_propagation(); s_fold.update(cx, |m, cx| m.toggle(&id_fold, cx)); })
                    .child(if folded { "收起".to_string() } else { format!("展开全部({lines} 行)") }),
            )
        })
        // 悬停时右下角出现:复制、编辑
        .child(
            h_flex().absolute().right(px(8.)).bottom(px(6.)).gap_0p5().invisible().group_hover(group, |d| d.visible())
                .child(icon_btn(format!("umsg-retry-{id}"), IconName::RotateCcw, "重试(原样重新发送)")
                    .on_click(move |_, _, cx| { cx.stop_propagation(); s_retry.update(cx, |m, cx| m.retry_user(&id_retry, cx)); }))
                .child(icon_btn(format!("umsg-copy-{id}"), IconName::Copy, "复制消息")
                    .on_click(move |_, _, cx| { cx.stop_propagation(); cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone())); }))
                .child(icon_btn(format!("umsg-edit-{id}"), IconName::SquarePen, "编辑并重新发送")
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        let (id, text) = (id_edit.clone(), text_edit.clone());
                        s_edit.update(cx, |m, cx| m.begin_edit(&id, &text, window, cx));
                    })),
        )
        .into_any_element()
}

/// 就地编辑卡:附件(只读)+ 文本框 + 取消 / 重新发送。
fn render_editor(session: &Entity<SessionModel>, input: &Entity<TextareaState>, images: &[String], cx: &App) -> AnyElement {
    let t = cx.theme();
    let running = session.read(cx).state.phase == loom_core::Phase::Running;
    let has_text = !input.read(cx).value().trim().is_empty();
    let (s_cancel, s_send, s_esc, s_out) = (session.clone(), session.clone(), session.clone(), session.clone());
    v_flex().id("umsg-editor").w_full().gap_2().px(px(14.)).py(px(10.)).rounded(px(12.)).bg(t.popover).border_1().border_color(t.border).shadow_sm()
        // 点编辑卡以外的任何地方:收起编辑(未发送的修改丢弃)
        .on_mouse_down_out(move |_, _, cx| { s_out.update(cx, |m, cx| m.end_edit(cx)); })
        .on_action(move |_: &crate::workbench::Escape, _, cx| { s_esc.update(cx, |m, cx| m.end_edit(cx)); })
        .when(!images.is_empty(), |d| d.child(h_flex().flex_wrap().gap_1p5().children(images.iter().map(|n| thumb(n, cx)))))
        .child(Textarea::new(input).appearance(false).bordered(false))
        .when(running, |d| d.child(div().text_size(px(11.)).text_color(t.muted_foreground).child("回合进行中,将排到本轮结束后发送。")))
        .child(
            h_flex().gap_2().items_center().justify_end()
                .child(Button::new("umsg-cancel").small().ghost().label("取消").on_click(move |_, _, cx| { s_cancel.update(cx, |m, cx| m.end_edit(cx)); }))
                .child(send_button(false, has_text, move |window, cx| { s_send.update(cx, |m, cx| m.submit_edit(window, cx)); }, cx)),
        )
        .into_any_element()
}

/// 编辑中的用户消息:消息 id 与文本框。
pub struct UserEdit {
    pub id: String,
    pub input: Entity<TextareaState>,
    pub(crate) _sub: Subscription,
}

impl SessionModel {
    /// 开始编辑一条用户消息(同一时刻只编辑一条)。
    ///
    /// 参数:`id` 消息 id;`text` 要放进文本框的正文(已去掉占位符)。
    pub fn begin_edit(&mut self, id: &str, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.as_ref().is_some_and(|e| e.id == id) {
            return;
        }
        if let Some(prev) = self.editing.take() {
            self.remeasure_id(&prev.id, cx);
        }
        self.scroller.update(cx, |s, _| s.pause_following());
        let input = cx.new(|cx| TextareaState::new(window, cx).auto_grow(1, 10).submit_on_enter(true));
        input.update(cx, |i, cx| { i.set_value(text.to_string(), window, cx); i.focus(window, cx); });
        let edit_id = id.to_string();
        let sub = cx.subscribe_in(&input, window, move |this, _, ev: &InputEvent, window, cx| match ev {
            // 文本框随内容增高:重新测量这一行
            InputEvent::Change => { this.remeasure_id(&edit_id, cx); cx.notify(); }
            InputEvent::PressEnter { shift: false, .. } => this.submit_edit(window, cx),
            _ => {}
        });
        self.editing = Some(UserEdit { id: id.to_string(), input, _sub: sub });
        self.remeasure_id(id, cx);
        cx.notify();
    }

    /// 重试:把这条用户消息原样重新发送(新消息,原消息与回复保留);回合进行中排到本轮结束后。
    ///
    /// 参数:`id` 消息 id。
    pub fn retry_user(&mut self, id: &str, cx: &mut Context<Self>) {
        let found = self.state.items.iter().find_map(|i| match i {
            loom_core::TimelineItem::User { id: uid, text, images, files } if uid == id => Some((text.clone(), images.clone(), files.clone())),
            _ => None,
        });
        let Some((text, images, files)) = found else { return };
        let text = display_text(&text);
        if text.is_empty() && images.is_empty() {
            return;
        }
        if self.state.phase == loom_core::Phase::Running && images.is_empty() {
            self.queue_prompt(text, cx);
        } else {
            self.send_prompt_with(text, images, files, cx);
        }
    }

    /// 结束编辑(取消或发送后)。
    pub fn end_edit(&mut self, cx: &mut Context<Self>) {
        if let Some(e) = self.editing.take() {
            self.remeasure_id(&e.id, cx);
        }
        cx.notify();
    }

    /// 把编辑后的文本作为新消息发送(原消息与回复保留);回合进行中排到本轮结束后。
    pub fn submit_edit(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(e) = self.editing.as_ref() else { return };
        let text = e.input.read(cx).value().trim().to_string();
        let (images, files) = match self.state.items.iter().find(|i| matches!(i, loom_core::TimelineItem::User { id, .. } if id == &e.id)) {
            Some(loom_core::TimelineItem::User { images, files, .. }) => (images.clone(), files.clone()),
            _ => (Vec::new(), Vec::new()),
        };
        if text.is_empty() && images.is_empty() {
            return;
        }
        let running = self.state.phase == loom_core::Phase::Running;
        self.end_edit(cx);
        if running && images.is_empty() {
            self.queue_prompt(text, cx);
        } else {
            self.send_prompt_with(text, images, files, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 去掉图片占位符() {
        assert_eq!(display_text("[Image #22] 这里的样式调优一下。"), "这里的样式调优一下。");
        assert_eq!(display_text("看这个 [Image #3] 和 [Image #4] 都有问题"), "看这个  和  都有问题");
        assert_eq!(display_text("[Image: source: /tmp/a.png]\n正文"), "正文");
        assert_eq!(display_text("文字\n\n[Image #1]\n"), "文字");
    }

    #[::core::prelude::v1::test]
    fn 普通方括号文字保留() {
        assert_eq!(display_text("数组 [Image] 与 [ImageData #1] 不是占位符"), "数组 [Image] 与 [ImageData #1] 不是占位符");
        assert_eq!(display_text("没有结束的 [Image #9"), "没有结束的 [Image #9");
    }
}
