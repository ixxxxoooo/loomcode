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

/// 超过这个(估算的显示)行数的用户消息默认折叠成固定高度的窗口(参考 OhMyAgent `CollapsibleUserText`)。
const FOLD_LINES: usize = 6;
/// 展开后最多显示的行数,再长就在卡内滚动。
const EXPANDED_LINES: usize = 24;
/// 估算折行用的每行列数(汉字等宽字符算 2 列)。
const COLS: usize = 44;
/// 正文行高。
const LINE_H: f32 = 21.;
/// 折叠时底部渐隐的高度。
const FADE_H: f32 = 22.;
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

/// 估算正文显示出来占几行:显式换行 + 长行折行(汉字等宽字符按 2 列算,每行 [`COLS`] 列)。
///
/// 折行取决于卡片实际宽度,这里只用来决定「要不要折叠」;窗口高度本身是固定的上限,估多了不会出错。
pub fn estimated_lines(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    text.split('\n').map(|l| {
        let cols: usize = l.chars().map(|c| if c.is_ascii() { 1 } else { 2 }).sum();
        cols.div_ceil(COLS).max(1)
    }).sum()
}

/// 用户消息卡的底色:与底部输入框同款的「浮起卡片」底色(浅色主题下是纸白,比会话背景亮一档),
/// 配描边与极淡的阴影,和会话背景一眼可分。不透明(吸顶卡压在会话流上,不能透出后面的文字)。
pub fn card_bg(cx: &App) -> Hsla {
    cx.theme().popover
}

/// 用户消息卡的描边:比普通分割线略深一点,让卡片边缘清晰。
pub fn card_border(cx: &App) -> Hsla {
    let t = cx.theme();
    crate::theme::mix(t.border, t.foreground, 0.06)
}

/// 渲染一条用户消息:编辑中显示编辑卡,否则显示消息卡。
pub fn render_user(session: &Entity<SessionModel>, id: &str, text: &str, images: &[String], files: &[String], cx: &App) -> AnyElement {
    let editing = session.read(cx).editing.iter().find(|e| e.id == id && !e.pinned).map(|e| e.input.clone());
    match editing {
        Some(input) => render_editor(session, id, &input, images, None, cx),
        None => render_card(session, id, text, images, files, cx),
    }
}

/// 悬浮编辑卡(从吸顶卡 / 消息卡发起的编辑)的消息 id,按打开顺序(后打开的盖在上面)。
pub fn pinned_edit_ids(session: &Entity<SessionModel>, cx: &App) -> Vec<String> {
    session.read(cx).editing.iter().filter(|e| e.pinned).map(|e| e.id.clone()).collect()
}

/// 某条消息的悬浮编辑卡;不在悬浮编辑中时返回 None。
///
/// 参数:`id` 消息 id;`max_body_h` 正文区(附件 + 文本框)的最大高度,超出在卡内滚动。
pub fn render_pinned_editor(session: &Entity<SessionModel>, id: &str, max_body_h: f32, cx: &App) -> Option<AnyElement> {
    let m = session.read(cx);
    let e = m.editing.iter().find(|e| e.pinned && e.id == id)?;
    let images = m.state.items.iter().find_map(|i| match i {
        loom_core::TimelineItem::User { id, images, .. } if id == &e.id => Some(images.clone()),
        _ => None,
    }).unwrap_or_default();
    Some(render_editor(session, id, &e.input.clone(), &images, Some(max_body_h), cx))
}

/// 悬停操作条上的小图标按钮:按下有底色反馈,`done` 为真时显示绿色对勾(复制 / 重试后短暂显示)。
///
/// 参数:`bid` 元素 id;`ic` 图标;`tip` 提示;`done` 是否显示完成态。
pub fn action_btn(bid: String, ic: IconName, tip: &'static str, done: bool, cx: &App) -> Stateful<Div> {
    let t = cx.theme();
    div().id(SharedString::from(bid)).size(px(24.)).flex().items_center().justify_center().rounded(px(6.)).cursor_pointer()
        // 按下不再冒泡给消息卡,避免点按钮时进入选择文本状态
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .hover(|d| d.bg(t.foreground.opacity(0.08)))
        .active(|d| d.bg(t.foreground.opacity(0.18)))
        .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tip).build(window, cx))
        .child(if done { icon(IconName::Check, 13., t.success) } else { icon(ic, 12., t.muted_foreground) })
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
    let long = estimated_lines(&text) > FOLD_LINES;
    let folded = session.read(cx).is_expanded(id);
    let group = SharedString::from(format!("umsg-{id}"));
    let (s_fold, id_fold) = (session.clone(), id.to_string());
    let (s_retry, id_retry) = (session.clone(), id.to_string());
    let (s_click, id_click, text_click) = (session.clone(), id.to_string(), text.clone());
    let copy_text = text.clone();
    let (copied, retried) = {
        let c = session.read(cx).copied.as_deref();
        (c == Some(format!("c-{id}").as_str()), c == Some(format!("r-{id}").as_str()))
    };
    let (s_copy_mark, id_copy_mark) = (session.clone(), id.to_string());
    let (s_retry_mark, id_retry_mark) = (session.clone(), id.to_string());
    // 有检查点 uuid 的消息多一个「回退文件」按钮,右侧留白随按钮数变化
    let rewindable = session.read(cx).can_rewind(id);
    let (s_rewind, id_rewind) = (session.clone(), id.to_string());
    v_flex().id(SharedString::from(format!("umsg-card-{id}"))).group(group.clone()).relative().w_full().gap_2().px(px(14.)).py(px(10.)).min_h(px(40.)).pr(px(if rewindable { 88. } else { 60. })).rounded(px(12.)).bg(card_bg(cx)).border_1().border_color(card_border(cx)).shadow_xs().cursor_pointer()
        .on_click(move |_, window, cx| {
            let (id, text) = (id_click.clone(), text_click.clone());
            s_click.update(cx, |m, cx| m.begin_edit(&id, &text, true, window, cx));
        })
        .when(!images.is_empty() || !files.is_empty(), |d| {
            d.child(h_flex().flex_wrap().gap_1p5().children(images.iter().map(|n| thumb(n, cx))).children(files.iter().map(|n| file_chip(n, cx))))
        })
        .when(!text.is_empty(), |d| d.child(text_window(id, &text, long, folded, cx)))
        .when(long, |d| {
            d.child(
                h_flex().id(SharedString::from(format!("user-fold-{id}"))).gap_1().items_center().text_size(px(12.)).text_color(t.muted_foreground).cursor_pointer()
                    .hover(|d| d.text_color(t.foreground))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(move |_, _, cx| { cx.stop_propagation(); s_fold.update(cx, |m, cx| m.toggle(&id_fold, cx)); })
                    .child(if folded { "收起" } else { "展开" })
                    .child(icon(if folded { IconName::ChevronUp } else { IconName::ChevronDown }, 11., t.muted_foreground)),
            )
        })
        // 悬停时右下角出现:回退文件、重试、复制
        .child(
            h_flex().absolute().right(px(8.)).bottom(px(8.)).gap_0p5().when(!(copied || retried), |d| d.invisible().group_hover(group, |d| d.visible()))
                .when(rewindable, |d| d.child(action_btn(format!("umsg-rewind-{id}"), IconName::Undo, "回退文件:把 Claude 之后改过的文件恢复到这条消息发出之前", false, cx)
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        if let Err(why) = s_rewind.update(cx, |m, cx| m.begin_rewind(&id_rewind, cx)) {
                            crate::notice::warning(window, cx, why);
                        }
                    })))
                .child(action_btn(format!("umsg-retry-{id}"), IconName::RotateCcw, "重试(原样重新发送)", retried, cx)
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        s_retry_mark.update(cx, |m, cx| m.mark_copied(&format!("r-{id_retry_mark}"), cx));
                        s_retry.update(cx, |m, cx| m.retry_user(&id_retry, cx));
                    }))
                .child(action_btn(format!("umsg-copy-{id}"), IconName::Copy, "复制消息", copied, cx)
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone()));
                        s_copy_mark.update(cx, |m, cx| m.mark_copied(&format!("c-{id_copy_mark}"), cx));
                    }))

        )
        .into_any_element()
}

/// 正文窗口:短消息原样显示;长消息折叠成固定行数的窗口(底部渐隐),展开后最多 [`EXPANDED_LINES`] 行、再长在卡内滚动。
fn text_window(id: &str, text: &str, long: bool, folded: bool, cx: &App) -> AnyElement {
    let body = div().min_w_0().text_size(px(14.)).line_height(px(LINE_H));
    // 含文件引用时按胶囊排版,否则原样文本
    let body = if crate::user_mentions::has_mentions(text) { body.child(crate::user_mentions::render(text, LINE_H, cx)) } else { body.child(text.to_string()) };
    if !long {
        return body.into_any_element();
    }
    if folded {
        return div().id(SharedString::from(format!("user-body-{id}"))).max_h(px(LINE_H * EXPANDED_LINES as f32)).overflow_y_scroll().child(body).into_any_element();
    }
    let bg = card_bg(cx);
    div().relative().max_h(px(LINE_H * FOLD_LINES as f32)).overflow_hidden().child(body)
        .child(div().absolute().bottom_0().left_0().right_0().h(px(FADE_H)).bg(linear_gradient(0., linear_color_stop(bg.opacity(0.), 0.), linear_color_stop(bg, 1.))))
        .into_any_element()
}

/// 就地编辑卡:附件(只读)+ 文本框 + 取消 / 重新发送。
pub(crate) fn render_editor(session: &Entity<SessionModel>, id: &str, input: &Entity<TextareaState>, images: &[String], max_body_h: Option<f32>, cx: &App) -> AnyElement {
    let t = cx.theme();
    let running = session.read(cx).state.phase == loom_core::Phase::Running;
    let has_text = !input.read(cx).value().trim().is_empty();
    let (s_cancel, s_send, s_esc, s_out) = (session.clone(), session.clone(), session.clone(), session.clone());
    let id_out = id.to_string();
    let (id_cancel, id_send, id_esc) = (id.to_string(), id.to_string(), id.to_string());
    v_flex().id(SharedString::from(format!("umsg-editor-{id}"))).w_full().gap_2().px(px(14.)).py(px(10.)).rounded(px(12.)).bg(t.popover).border_1().border_color(t.border).shadow_sm()
        // 点编辑卡以外的任何地方:收起这张编辑卡(未发送的修改丢弃);取消 / 发送 / Esc 同样关闭
        .on_mouse_down_out(move |_, _, cx| { s_out.update(cx, |m, cx| m.end_edit(&id_out, cx)); })
        .on_action(move |_: &crate::workbench::Escape, _, cx| { s_esc.update(cx, |m, cx| m.end_edit(&id_esc, cx)); })
        // 正文区(附件 + 文本框):悬浮时限制最大高度,超出在框内滚动;横向不外溢
        .child(
            v_flex().id(SharedString::from(format!("umsg-editor-body-{id}"))).w_full().min_w_0().gap_2().overflow_x_hidden()
                .when_some(max_body_h, |d, h| d.max_h(px(h)).overflow_y_scroll())
                .when(!images.is_empty(), |d| d.child(h_flex().flex_wrap().gap_1p5().children(images.iter().map(|n| thumb(n, cx)))))
                .child(Textarea::new(input).appearance(false).bordered(false).token(|ctx, window, cx| crate::composer_refs::token_view(ctx, window, cx))),
        )
        .when(running, |d| d.child(div().text_size(px(11.)).text_color(t.muted_foreground).child("回合进行中,将排到本轮结束后发送。")))
        .child(
            h_flex().gap_2().items_center().justify_end()
                .children(crate::edit_model::picker(session, id, cx).map(|c| div().flex_1().flex().child(c)))
                .child(Button::new(SharedString::from(format!("umsg-cancel-{id}"))).small().ghost().label("取消").on_click(move |_, _, cx| { s_cancel.update(cx, |m, cx| m.end_edit(&id_cancel, cx)); }))
                .child(send_button(false, has_text, move |window, cx| { s_send.update(cx, |m, cx| m.submit_edit(&id_send, window, cx)); }, cx)),
        )
        .into_any_element()
}

/// 编辑中的用户消息:消息 id 与文本框。
pub struct UserEdit {
    pub id: String,
    pub input: Entity<TextareaState>,
    /// 从吸顶卡发起:编辑卡悬浮在会话流顶部、向下展开,不推挤会话流
    pub pinned: bool,
    /// 模型列表是否展开
    pub model_open: bool,
    pub(crate) _sub: Subscription,
}

impl SessionModel {
    /// 开始编辑一条用户消息。可以同时编辑多条,互不影响;这条已在编辑中就只把焦点给它。
    ///
    /// 参数:`id` 消息 id;`text` 要放进文本框的正文(已去掉占位符);`pinned` 是否从吸顶卡发起(悬浮编辑)。
    pub fn begin_edit(&mut self, id: &str, text: &str, pinned: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(e) = self.editing.iter().find(|e| e.id == id) {
            let input = e.input.clone();
            input.update(cx, |i, cx| i.focus(window, cx));
            return;
        }
        self.scroller.update(cx, |s, _| s.pause_following());
        let input = cx.new(|cx| TextareaState::new(window, cx).auto_grow(1, 10).submit_on_enter(true));
        input.update(cx, |i, cx| {
            i.set_value(crate::user_mentions::content_from_text(text), window, cx);
            // 光标落在正文末尾(set_value 默认回到开头),接着往后写最顺手
            let end = i.value().len();
            i.set_selected_range(end..end, cx);
            i.focus(window, cx);
        });
        let edit_id = id.to_string();
        let sub = cx.subscribe_in(&input, window, move |this, _, ev: &InputEvent, window, cx| match ev {
            // 文本框随内容增高:重新测量这一行
            InputEvent::Change => { this.remeasure_id(&edit_id, cx); cx.notify(); }
            InputEvent::PressEnter { shift: false, .. } => this.submit_edit(&edit_id, window, cx),
            _ => {}
        });
        self.editing.push(UserEdit { id: id.to_string(), input, pinned, model_open: false, _sub: sub });
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
        if self.state.phase == loom_core::Phase::Running {
            self.queue_prompt(text, images, files, cx);
        } else {
            self.send_prompt_with(text, images, files, cx);
        }
    }

    /// 展开 / 收起编辑卡里的模型列表。
    pub fn toggle_edit_model(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(e) = self.editing.iter_mut().find(|e| e.id == id) {
            e.model_open = !e.model_open;
        }
        cx.notify();
    }

    /// 结束编辑(取消或发送后)。
    pub fn end_edit(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(ix) = self.editing.iter().position(|e| e.id == id) {
            let e = self.editing.remove(ix);
            self.remeasure_id(&e.id, cx);
        }
        cx.notify();
    }

    /// 把编辑后的文本作为新消息发送(原消息与回复保留);回合进行中排到本轮结束后。
    pub fn submit_edit(&mut self, id: &str, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(e) = self.editing.iter().find(|e| e.id == id) else { return };
        let text = e.input.read(cx).value().trim().to_string();
        let (images, files) = match self.state.items.iter().find(|i| matches!(i, loom_core::TimelineItem::User { id, .. } if id == &e.id)) {
            Some(loom_core::TimelineItem::User { images, files, .. }) => (images.clone(), files.clone()),
            _ => (Vec::new(), Vec::new()),
        };
        if text.is_empty() && images.is_empty() {
            return;
        }
        let running = self.state.phase == loom_core::Phase::Running;
        self.end_edit(id, cx);
        if running {
            self.queue_prompt(text, images, files, cx);
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

    #[::core::prelude::v1::test]
    fn 估算显示行数() {
        assert_eq!(estimated_lines(""), 0);
        assert_eq!(estimated_lines("短"), 1);
        assert_eq!(estimated_lines("a\n\nb"), 3);
        // 一行很长的 ASCII(如 curl 命令)折成多行
        assert_eq!(estimated_lines(&"x".repeat(COLS * 3 + 1)), 4);
        // 汉字按 2 列
        assert_eq!(estimated_lines(&"字".repeat(COLS)), 2);
        // 少数几行但每行很长 → 超过折叠阈值
        assert!(estimated_lines(&format!("{}\n{}", "x".repeat(200), "y".repeat(200))) > FOLD_LINES);
    }
}
