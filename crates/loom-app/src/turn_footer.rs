//! 助手回复下面的操作条(一直显示):复制、分支、模型 · 用时 · 结束时间。
//!
//! @author ixxxxoooo

use crate::model::SessionModel;
use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// 操作条高度(固定占位,旧回合悬停才可见,避免悬停时行高跳动)。
pub const FOOTER_H: f32 = 26.;

/// 回合元信息:模型 · 用时 · token / 缓存 / 费用 · 结束时间(没有的项跳过)。
///
/// 参数:`session` 会话;`ix` 本回合最后一条助手回复的条目下标。
/// 返回:用「 · 」连接的文本(可能为空),以及悬停时显示的消耗明细(没有统计为 `None`)。
pub fn meta_text(session: &Entity<SessionModel>, ix: usize, cx: &App) -> (String, Option<String>) {
    let m = session.read(cx);
    let user = m.state.items[..ix].iter().rev().find_map(|i| if let loom_core::TimelineItem::User { id, .. } = i { Some(id.clone()) } else { None });
    let mut parts: Vec<String> = Vec::new();
    let mut tip = None;
    let name = m.state.models.iter().find(|x| x.id == m.state.model).map_or(m.state.model.clone(), |x| x.name.clone());
    if !name.is_empty() {
        parts.push(name);
    }
    if let Some(u) = &user {
        if let Some(secs) = m.state.durations.get(u) {
            parts.push(format!("用时 {}", crate::appearance::elapsed_text(*secs)));
        }
        if let Some(stats) = m.state.turn_stats.get(u) {
            parts.push(stats.short_text());
            tip = Some(stats.detail_text());
        }
        if let Some(t) = m.turn_finished.get(u) {
            parts.push(crate::real::time_ago(*t));
        }
    }
    (parts.join(" · "), tip)
}

/// 渲染操作条(一直显示):复制、分支,后面跟回合元信息。
///
/// 参数:`session` 会话;`id` 助手条目 id;`text` 要复制的原文;`meta` 元信息文本与悬停明细。
pub fn render(session: &Entity<SessionModel>, id: &str, text: &str, meta: (String, Option<String>), latest: bool, cx: &App) -> AnyElement {
    let t = cx.theme();
    let copied = session.read(cx).copied.as_deref() == Some(id);
    let s = session.clone();
    let (cid, ctext) = (id.to_string(), text.to_string());
    // 最近一个回合常驻;旧回合悬停在回复上才显示(占位高度不变,不跳动),刚复制过的保持可见
    let group = SharedString::from(format!("turn-{id}"));
    let (meta, tip) = meta;
    h_flex().h(px(FOOTER_H)).items_center().gap_1().mt_0p5()
        .when(!latest && !copied, |d| d.invisible().group_hover(group, |d| d.visible()))
        .child(
            h_flex().id(SharedString::from(format!("copy-{id}"))).h(px(22.)).px_1p5().gap_1().items_center().rounded(px(5.)).cursor_pointer()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .text_size(px(12.)).text_color(t.muted_foreground)
                .hover(|d| d.bg(t.list_hover).text_color(t.foreground))
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(ctext.clone()));
                    s.update(cx, |m, cx| m.mark_copied(&cid, cx));
                })
                .child(icon(if copied { IconName::Check } else { IconName::Copy }, 12., if copied { t.success } else { t.muted_foreground }))
                .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(if copied { "已复制" } else { "复制" }).build(window, cx)),
        )
        .child(
            h_flex().id(SharedString::from(format!("fork-{id}"))).h(px(22.)).px_1p5().items_center().rounded(px(5.)).cursor_pointer()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .hover(|d| d.bg(t.list_hover))
                .on_click(|_, window, cx| window.dispatch_action(Box::new(crate::workbench::ForkActive), cx))
                .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new("从这里创建分支会话").build(window, cx))
                .child(icon(IconName::GitFork, 12., t.muted_foreground)),
        )
        .when(!meta.is_empty(), |d| {
            d.child(
                div().id(SharedString::from(format!("meta-{id}"))).ml_1().min_w_0().text_size(px(12.)).text_color(t.muted_foreground).truncate().child(meta.clone())
                    .when_some(tip, |d, tip| d.tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx))),
            )
        })
        .into_any_element()
}
