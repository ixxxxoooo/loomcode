//! 待办的展示(对标 Cursor / Claude Code):
//! - 时间线里每次 TodoWrite 只留一行差异摘要(「✓ 完成 X · ▶ 开始 Y」),点击展开当次全量;
//! - 输入框上方固定一条「待办条」,显示最新一份待办的进度与当前项,点击向上悬浮展开全量清单;
//! - 「处理中」行优先显示进行中项的 `activeForm`(见 [`current_active`])。
//!
//! @author ixxxxoooo

use crate::model::SessionModel;
use crate::ui::{caption, icon};
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::*;

/// 待办条展开状态的键(存在会话的展开集合里)。
const DOCK_KEY: &str = "todo-dock";

/// 一项待办的变化。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delta {
    /// 新完成
    Done(String),
    /// 新开始
    Started(String),
    /// 新加入
    Added(String),
}

/// 比较两份待办,列出变化(按内容对应;顺序:完成、开始、新增)。
///
/// 参数:`prev` 上一份;`cur` 这一份。返回:变化列表,没有上一份时为空。
pub fn delta(prev: &[TodoItem], cur: &[TodoItem]) -> Vec<Delta> {
    let old = |c: &str| prev.iter().find(|p| p.content == c).map(|p| p.status);
    let mut done = Vec::new();
    let mut started = Vec::new();
    let mut added = Vec::new();
    for it in cur {
        match (old(&it.content), it.status) {
            (None, _) => added.push(Delta::Added(it.content.clone())),
            (Some(s), TodoStatus::Completed) if s != TodoStatus::Completed => done.push(Delta::Done(it.content.clone())),
            (Some(s), TodoStatus::InProgress) if s != TodoStatus::InProgress => started.push(Delta::Started(it.content.clone())),
            _ => {}
        }
    }
    done.into_iter().chain(started).chain(added).collect()
}

/// 进行中项的显示文案:优先 `activeForm`,否则内容本身。
fn active_text(it: &TodoItem) -> String {
    it.active_form.clone().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| it.content.clone())
}

/// 最新一份待办(只看顶层时间线)。
fn latest(items: &[TimelineItem]) -> Option<&[TodoItem]> {
    items.iter().rev().find_map(|i| if let TimelineItem::Todos { items, .. } = i { Some(items.as_slice()) } else { None })
}

/// 当前进行中的待办文案(给「处理中」行用);没有返回 `None`。
pub fn current_active(items: &[TimelineItem]) -> Option<String> {
    latest(items)?.iter().find(|i| i.status == TodoStatus::InProgress).map(active_text)
}

/// 状态图标与颜色。
fn status_icon(status: TodoStatus, cx: &App) -> (IconName, Hsla) {
    let t = cx.theme();
    match status {
        TodoStatus::Completed => (IconName::CircleCheck, t.success),
        TodoStatus::InProgress => (IconName::CircleDot, t.primary),
        TodoStatus::Pending => (IconName::Circle, t.muted_foreground),
    }
}

/// 全量清单(时间线展开与待办条面板共用)。
fn list(key: &str, items: &[TodoItem], cx: &App) -> Div {
    let t = cx.theme();
    v_flex().gap_1().children(items.iter().enumerate().map(|(i, it)| {
        let (ic, color) = status_icon(it.status, cx);
        let done = it.status == TodoStatus::Completed;
        h_flex().id(SharedString::from(format!("{key}-todo-{i}"))).gap_2().items_start()
            .child(div().mt(px(2.)).child(icon(ic, 13., color)))
            .child(div().flex_1().min_w_0().text_size(px(13.))
                .text_color(if done { t.muted_foreground } else { t.foreground })
                .when(done, |d| d.line_through())
                .when(it.status == TodoStatus::InProgress, |d| d.font_weight(FontWeight::MEDIUM))
                .child(it.content.clone()))
    }))
}

/// 时间线里的待办行:一行差异摘要,点击展开当次全量。
///
/// 参数:`session` 会话;`id` 条目 id;`items` 这一份待办。
pub fn render_line(session: &Entity<SessionModel>, id: &str, items: &[TodoItem], cx: &App) -> AnyElement {
    let t = cx.theme();
    let m = session.read(cx);
    let prev = m.state.items.iter().take_while(|i| i.id() != id).filter_map(|i| if let TimelineItem::Todos { items, .. } = i { Some(items.as_slice()) } else { None }).last();
    let expanded = m.is_expanded(id);
    let done = items.iter().filter(|i| i.status == TodoStatus::Completed).count();
    let changes = prev.map(|p| delta(p, items));
    let (s, key) = (session.clone(), id.to_string());
    let mut head = crate::flow_style::step_row().id(SharedString::from(format!("todo-line-{id}"))).cursor_pointer()
        .hover(|d| d.bg(t.list_hover))
        .on_click(move |_, _, cx| { s.update(cx, |m, cx| m.toggle(&key, cx)); })
        .child(crate::flow_style::icon_col(icon(IconName::ListTodo, 13., t.muted_foreground)))
        .child(div().flex_none().text_size(px(13.)).text_color(t.muted_foreground).child(if prev.is_none() { format!("创建了 {} 项待办", items.len()) } else { "更新了待办".into() }))
        .child(div().flex_none().text_size(px(11.5)).text_color(t.muted_foreground.opacity(0.75)).child(format!("{done}/{}", items.len())));
    for d in changes.unwrap_or_default().into_iter().take(3) {
        let (ic, color, text) = match d {
            Delta::Done(x) => (IconName::Check, t.success, x),
            Delta::Started(x) => (IconName::CircleDot, t.primary, x),
            Delta::Added(x) => (IconName::Plus, t.muted_foreground, x),
        };
        head = head.child(h_flex().min_w_0().gap_1().items_center().child(icon(ic, 11., color)).child(div().min_w_0().truncate().text_size(px(12.)).text_color(t.muted_foreground).child(text)));
    }
    head = head.child(m.motion.chevron(id, expanded, 11., t.muted_foreground));
    v_flex().w_full().child(head)
        .when(expanded, |d| d.child(div().pl(px(crate::flow_style::TEXT_INSET)).pr_2().py_1().child(list(id, items, cx))))
        .into_any_element()
}

/// 输入框上方的待办条:最新一份待办还没全部完成时显示,折叠为一行(当前项 + 进度),点击向上悬浮展开全量。
///
/// 参数:`session` 会话。返回:待办条;没有待办或已全部完成(且不在运行)时 `None`。
pub fn dock(session: &Entity<SessionModel>, cx: &App) -> Option<AnyElement> {
    let m = session.read(cx);
    let items = latest(&m.state.items)?;
    let done = items.iter().filter(|i| i.status == TodoStatus::Completed).count();
    if items.is_empty() || (done == items.len() && m.state.phase != Phase::Running) {
        return None;
    }
    let t = cx.theme();
    let open = m.is_expanded(DOCK_KEY);
    let current = items.iter().find(|i| i.status == TodoStatus::InProgress).map(active_text)
        .or_else(|| items.iter().find(|i| i.status == TodoStatus::Pending).map(|i| format!("下一项:{}", i.content)))
        .unwrap_or_else(|| "全部完成".into());
    let s = session.clone();
    let bar = h_flex().id("todo-dock").w_full().h(px(28.)).px(px(11.)).gap_2().items_center().rounded(px(8.)).cursor_pointer()
        .border_1().border_color(if open { t.primary.opacity(0.5) } else { t.border }).bg(if open { t.list_active } else { t.background }).hover(|d| d.bg(t.list_hover))
        .on_click(move |_, _, cx| { s.update(cx, |m, cx| m.toggle(DOCK_KEY, cx)); })
        .child(icon(IconName::ListTodo, 12., t.muted_foreground))
        .child(div().flex_none().text_size(px(12.)).text_color(t.muted_foreground).child(format!("待办 {done}/{}", items.len())))
        .child(div().flex_1().min_w_0().truncate().text_size(px(12.)).text_color(t.foreground).child(current))
        .child(icon(if open { IconName::ChevronDown } else { IconName::ChevronUp }, 11., t.muted_foreground));
    let mut host = div().relative().w_full().max_w(px(crate::timeline::content_max_w(cx))).child(bar);
    if open {
        let s_out = session.clone();
        // 向上悬浮,不挤动布局;放进 deferred 保证命中测试在最上层(同任务坞面板)
        let panel = v_flex().id("todo-dock-panel").absolute().bottom(px(32.)).left_0().w_full().occlude()
            .rounded(px(10.)).border_1().border_color(t.border).bg(t.popover).shadow_lg().overflow_hidden()
            .on_mouse_down_out(move |_, _, cx| { s_out.update(cx, |m, cx| if m.is_expanded(DOCK_KEY) { m.toggle(DOCK_KEY, cx) }); })
            .child(h_flex().h(px(30.)).px_3().items_center().child(caption(format!("待办 · 已完成 {done}/{}", items.len()), cx)))
            .child(div().id("todo-dock-list").w_full().max_h(px(240.)).overflow_y_scroll().px_3().pb_2().child(list("todo-dock", items, cx)));
        host = host.child(deferred(panel).with_priority(10));
    }
    Some(div().w_full().flex().justify_center().px_4().pt_1().child(host).into_any_element())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn it(c: &str, s: TodoStatus) -> TodoItem {
        TodoItem { content: c.into(), status: s, active_form: None }
    }

    #[::core::prelude::v1::test]
    fn 待办差异按完成开始新增排列() {
        let prev = [it("a", TodoStatus::InProgress), it("b", TodoStatus::Pending)];
        let cur = [it("a", TodoStatus::Completed), it("b", TodoStatus::InProgress), it("c", TodoStatus::Pending)];
        assert_eq!(delta(&prev, &cur), vec![Delta::Done("a".into()), Delta::Started("b".into()), Delta::Added("c".into())]);
        assert!(delta(&cur, &cur).is_empty());
    }

    #[::core::prelude::v1::test]
    fn 进行中文案优先_active_form() {
        let mut a = it("写测试", TodoStatus::InProgress);
        let items = vec![TimelineItem::Todos { id: "t".into(), items: vec![a.clone()] }];
        assert_eq!(current_active(&items).as_deref(), Some("写测试"));
        a.active_form = Some("正在写测试".into());
        let items = vec![TimelineItem::Todos { id: "t".into(), items: vec![a] }];
        assert_eq!(current_active(&items).as_deref(), Some("正在写测试"));
    }
}
