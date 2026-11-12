//! 时间线上的各类卡片:画布、改动汇总、提示(权限 / 提问 / 计划见 `dock_cards`、`ask_cards`,子 Agent 见 `subagent_card`)。

use crate::model::{SessionModel, UiRequest};
use crate::ui::{MONO, card, caption, icon};
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::*;

/// Canvas 卡片。
pub fn render_canvas(session: &Entity<SessionModel>, id: &str, title: &str, cx: &App) -> AnyElement {
    let t = cx.theme();
    let (s, cid) = (session.clone(), id.to_string());
    card(cx).id(SharedString::from(format!("canvas-{id}"))).p_3().cursor_pointer().hover(|d| d.bg(t.list_hover))
        .on_click(move |_, _, cx| { s.update(cx, |_, cx| cx.emit(UiRequest::OpenCanvas(cid.clone()))); })
        .child(h_flex().gap_2().items_center()
            .child(icon(IconName::LayoutDashboard, 16., t.primary))
            .child(v_flex().child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(title.to_string())).child(caption("画布 · 点击打开", cx))))
        .into_any_element()
}

/// 本轮改动汇总(Cursor 风格:「N 个文件已更改」+ 审查,每行一个文件,点击直接查看)。
///
/// 参数:`session` 会话;`files` 改动文件;返回:卡片元素。
pub fn render_changes(session: &Entity<SessionModel>, files: &[ChangedFile], cx: &App) -> AnyElement {
    let t = cx.theme();
    let open = |s: &Entity<SessionModel>| { let s = s.clone(); move |_: &ClickEvent, _: &mut Window, cx: &mut App| { s.update(cx, |_, cx| cx.emit(UiRequest::OpenChanges)); } };
    card(cx).p_3().gap_1().mt_2()
        .child(h_flex().id("changes-head").items_center().cursor_pointer().on_click(open(session))
            .child(div().flex_1().text_size(px(13.)).text_color(t.muted_foreground).child(format!("{} 个文件已更改", files.len())))
            .child(div().text_size(px(13.)).text_color(t.muted_foreground).hover(|d| d.text_color(t.foreground)).child("审查")))
        .children(files.iter().enumerate().map(|(i, f)| {
            let name = f.path.rsplit('/').next().unwrap_or(&f.path).to_string();
            h_flex().id(SharedString::from(format!("changes-file-{i}"))).h(px(28.)).px_1().gap_2().items_center().rounded(px(6.)).cursor_pointer()
                .hover(|d| d.bg(t.list_hover)).on_click({ let (s, p) = (session.clone(), f.path.clone()); move |_, _, cx| { s.update(cx, |_, cx| cx.emit(UiRequest::OpenChangeFile(p.clone()))); } })
                .child(crate::file_icons::file_icon_img(&name, false, false, 14.))
                .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).child(name))
                .when(f.added > 0, |d| d.child(div().text_size(px(12.)).font_family(MONO).text_color(t.success).child(format!("+{}", f.added))))
                .when(f.removed > 0, |d| d.child(div().text_size(px(12.)).font_family(MONO).text_color(t.danger).child(format!("−{}", f.removed))))
        }))
        .into_any_element()
}

/// 提示行(对标 Cursor 的错误卡):正文用中性色便于阅读,级别色只用在图标与左侧色条;
/// 最近一个回合里的错误带「重试」(原样重发本回合的用户消息)与「复制」。
///
/// 参数:`session` 会话;`id` 条目 id;`level` 级别;`text` 文案。
pub fn render_notice(session: &Entity<SessionModel>, id: &str, level: NoticeLevel, text: &str, cx: &App) -> AnyElement {
    let t = cx.theme();
    // 「已中断」:用户自己的操作,只是一行灰字(对标 Claude Code 的 Interrupted 标记)
    if text == INTERRUPTED_TEXT {
        return crate::flow_style::plain_row().text_size(px(12.)).text_color(t.muted_foreground)
            .child(crate::flow_style::icon_col(icon(IconName::CirclePause, 12., t.muted_foreground)))
            .child(format!("{text} · 可以直接输入新的指示"))
            .into_any_element();
    }
    let (ic, color) = match level {
        NoticeLevel::Info => (IconName::Info, t.info),
        NoticeLevel::Warning => (IconName::TriangleAlert, t.warning),
        NoticeLevel::Error => (IconName::CircleX, t.danger),
    };
    // 重试只给最近一个回合、且当前空闲时的错误:找本条之前最近的用户消息,之后不能再有新的用户消息
    let retry_target = (level == NoticeLevel::Error).then(|| {
        let m = session.read(cx);
        let pos = m.state.items.iter().position(|i| i.id() == id)?;
        let later_user = m.state.items[pos..].iter().any(|i| matches!(i, TimelineItem::User { .. }));
        let idle = m.state.phase != Phase::Running;
        let user = m.state.items[..pos].iter().rev().find_map(|i| if let TimelineItem::User { id, .. } = i { Some(id.clone()) } else { None });
        user.filter(|_| idle && !later_user)
    }).flatten();
    let copied = session.read(cx).copied.as_deref() == Some(&format!("n-{id}"));
    let action = |key: &str, ic: IconName, label: &'static str| {
        h_flex().id(SharedString::from(format!("notice-{key}-{id}"))).h(px(22.)).px_1p5().gap_1().items_center().rounded(px(5.)).cursor_pointer()
            .text_size(px(12.)).text_color(t.muted_foreground).hover(|d| d.bg(t.list_hover).text_color(t.foreground))
            .child(icon(ic, 12., t.muted_foreground)).child(label)
    };
    // 分级:普通提示(切换模式、已压缩上下文……)只是过程里的一句旁白,画成和步骤行同一栅格的灰字;
    // 警告 / 错误才是需要注意的卡片,用级别色的淡底与细描边(不再所有级别都是同样的灰底色条)
    if level == NoticeLevel::Info {
        return h_flex().w_full().min_h(px(crate::flow_style::ROW_H)).py(px(3.)).gap(px(crate::flow_style::ICON_GAP)).items_start()
            .child(crate::flow_style::icon_col(icon(ic, 12., t.muted_foreground)).h(px(20.)))
            .child(div().flex_1().min_w_0().text_size(px(12.5)).line_height(px(20.)).text_color(t.muted_foreground).child(gpui_base::SelectableText::new(SharedString::from(format!("notice-sel-{id}")), text.to_string())))
            .into_any_element();
    }
    h_flex().w_full().gap_2().px(px(10.)).py(px(7.)).rounded(px(8.)).items_start()
        .bg(color.opacity(0.07)).border_1().border_color(color.opacity(0.22))
        .child(div().h(px(20.)).flex().items_center().child(icon(ic, 14., color)))
        // min_w_0:长英文报错在卡片内折行,不再溢出右侧
        .child(div().flex_1().min_w_0().text_size(px(13.)).line_height(px(20.)).text_color(t.foreground).child(gpui_base::SelectableText::new(SharedString::from(format!("notice-sel-{id}")), text.to_string())))
        .when(level == NoticeLevel::Error, |d| d.child(h_flex().flex_none().gap_0p5()
            .when_some(retry_target, |d, uid| {
                let s = session.clone();
                d.child(action("retry", IconName::RotateCcw, "重试").on_click(move |_, _, cx| { s.update(cx, |m, cx| m.retry_user(&uid, cx)); }))
            })
            .child({
                let (s, nid, txt) = (session.clone(), id.to_string(), text.to_string());
                action("copy", if copied { IconName::Check } else { IconName::Copy }, if copied { "已复制" } else { "复制" }).on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(txt.clone()));
                    s.update(cx, |m, cx| m.mark_copied(&format!("n-{nid}"), cx));
                })
            })))
        .into_any_element()
}
