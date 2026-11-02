//! 输入框顶部的「排队消息」卡(对标 Cursor):每条一行,可编辑 / 引导 / 停止并发送 / 删除;
//! 上一轮被中断后队列暂停,卡头显示「已暂停」与「继续 / 清空」。
//!
//! @author ixxxxoooo

use crate::composer::Composer;
use crate::model::SessionModel;
use crate::prompt_queue::QueuedPrompt;
use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// 最多直接显示几行,其余折成「还有 N 条」。
const MAX_ROWS: usize = 4;

/// 一条消息在队列里的单行摘要:取第一行,图片 / 文件引用另注。
fn summary(p: &QueuedPrompt) -> String {
    let line = crate::user_message::display_text(&p.text).lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
    let extra = if p.images.is_empty() { String::new() } else { format!(" · {} 张图片", p.images.len()) };
    if line.is_empty() { format!("(仅附件){extra}") } else { format!("{line}{extra}") }
}

/// 小文字按钮。
fn text_btn(id: SharedString, label: &'static str, color: Hsla, hover: Hsla, on_click: impl Fn(&mut Window, &mut App) + 'static) -> Stateful<Div> {
    div().id(id).flex_none().h(px(20.)).px_1p5().flex().items_center().rounded(px(5.)).cursor_pointer()
        .text_size(px(11.)).text_color(color)
        .hover(move |d| d.bg(hover))
        .on_click(move |_, window, cx| { cx.stop_propagation(); on_click(window, cx); })
        .child(label)
}

/// 渲染队列卡;队列为空时返回 `None`。
///
/// 参数:`session` 会话;`composer` 所属输入框(编辑要写回输入框);`running` 回合是否进行中。
pub fn render(session: &Entity<SessionModel>, composer: &Entity<Composer>, running: bool, cx: &App) -> Option<AnyElement> {
    let t = cx.theme();
    let q = &session.read(cx).queued;
    if q.is_empty() {
        return None;
    }
    let paused = q.paused();
    let items: Vec<QueuedPrompt> = q.items().to_vec();
    let hidden = items.len().saturating_sub(MAX_ROWS);
    let hover = t.list_hover;

    let header = paused.then(|| {
        let (s1, s2) = (session.clone(), composer.clone());
        h_flex().h(px(26.)).px_3().gap_2().items_center()
            .child(icon(IconName::CirclePause, 12., t.warning))
            .child(div().flex_1().min_w_0().truncate().text_size(px(11.)).text_color(t.warning).child("队列已暂停:上一轮被中断,不会自动发送"))
            .child(text_btn("queue-resume".into(), "继续", t.primary, hover, move |_, cx| { s1.update(cx, |m, cx| m.resume_queue(cx)); }))
            .child(text_btn("queue-clear".into(), "撤回", t.muted_foreground, hover, move |window, cx| {
                s2.update(cx, |c, cx| c.withdraw_queued(window, cx));
            }))
    });

    let rows = items.into_iter().take(MAX_ROWS).map(|p| {
        let id = p.id;
        let (s_edit, c_edit) = (session.clone(), composer.clone());
        let (s_steer, s_stop, s_send, s_del) = (session.clone(), session.clone(), session.clone(), session.clone());
        h_flex().h(px(28.)).px_3().gap_1().items_center()
            .child(icon(IconName::CornerDownRight, 12., t.muted_foreground))
            .child(
                div().id(SharedString::from(format!("queue-text-{id}"))).flex_1().min_w_0().truncate().cursor_pointer()
                    .text_size(px(12.)).text_color(t.foreground).child(summary(&p))
                    // 点文字:退回输入框继续编辑
                    .on_click(move |_, window, cx| {
                        if let Some(p) = s_edit.update(cx, |m, cx| m.remove_queued(id, cx)) {
                            c_edit.update(cx, |c, cx| c.restore_queued(p, window, cx));
                        }
                    }),
            )
            .when(running, |d| {
                d.child(text_btn(SharedString::from(format!("queue-steer-{id}")), "引导", t.primary, hover, move |_, cx| { s_steer.update(cx, |m, cx| m.steer_queued(id, cx)); }))
                    .child(text_btn(SharedString::from(format!("queue-stop-{id}")), "停止并发送", t.muted_foreground, hover, move |_, cx| { s_stop.update(cx, |m, cx| m.stop_and_send(id, cx)); }))
            })
            .when(!running, |d| d.child(text_btn(SharedString::from(format!("queue-send-{id}")), "发送", t.primary, hover, move |_, cx| { s_send.update(cx, |m, cx| m.steer_queued(id, cx)); })))
            .child(
                div().id(SharedString::from(format!("queue-del-{id}"))).flex_none().size(px(20.)).flex().items_center().justify_center().rounded(px(5.)).cursor_pointer()
                    .hover(move |d| d.bg(hover))
                    .on_click(move |_, _, cx| { s_del.update(cx, |m, cx| { m.remove_queued(id, cx); }); })
                    .child(icon(IconName::X, 11., t.muted_foreground)),
            )
    });

    Some(
        v_flex().w_full().pt_1().pb_1().border_b_1().border_color(t.border.opacity(0.6))
            .when_some(header, |d, h| d.child(h))
            .children(rows)
            .when(hidden > 0, |d| d.child(div().px_3().h(px(22.)).flex().items_center().text_size(px(11.)).text_color(t.muted_foreground).child(format!("还有 {hidden} 条排队"))))
            .into_any_element(),
    )
}
