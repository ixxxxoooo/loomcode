//! IDE 对话栏的历史会话选择器(对标 JetBrains / Cursor 的「搜索智能体」弹层):
//! 顶部搜索框,下方按「过去 7 天 / 更早」分组的会话列表,右侧是相对时间,当前已打开的会话带对勾。
//!
//! @author ixxxxoooo

use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// 选择器发给宿主的事件。
pub enum HistoryEvent {
    /// 打开(或切到)某个会话
    Open(String),
    Close,
}

impl EventEmitter<HistoryEvent> for HistoryPicker {}

/// 一条历史会话。
#[derive(Clone)]
pub struct HistoryItem {
    pub id: String,
    pub title: String,
    /// 相对时间文案(如「2 天前」)
    pub updated: String,
    /// 是否已在对话栏打开
    pub open: bool,
}

/// 时间分组。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bucket {
    Recent,
    Earlier,
}

/// 按相对时间文案分组:刚刚 / 分钟 / 小时 / 昨天 / 不超过 7 天 → 过去 7 天,其余 → 更早。
///
/// 参数:`updated` 相对时间文案。
pub fn bucket(updated: &str) -> Bucket {
    let s = updated.trim();
    if s == "刚刚" || s == "昨天" || s == "今天" || s.contains("分钟") || s.contains("小时") {
        return Bucket::Recent;
    }
    let days: Option<u32> = s.strip_suffix("天前").and_then(|n| n.trim().parse().ok());
    if days.is_some_and(|d| d <= 7) { Bucket::Recent } else { Bucket::Earlier }
}

/// 按查询过滤并分组(保持原顺序):返回 (分组, 条目) 的扁平列表。
///
/// 参数:`items` 全部会话;`query` 搜索词(不分大小写,空则全部)。
pub fn filter(items: &[HistoryItem], query: &str) -> Vec<(Bucket, HistoryItem)> {
    let q = query.trim().to_lowercase();
    let hit = |i: &&HistoryItem| q.is_empty() || i.title.to_lowercase().contains(&q);
    let pick = |b: Bucket| items.iter().filter(hit).filter(move |i| bucket(&i.updated) == b).cloned().map(move |i| (b, i)).collect::<Vec<_>>();
    let mut out = pick(Bucket::Recent);
    out.extend(pick(Bucket::Earlier));
    out
}

pub struct HistoryPicker {
    input: Entity<InputState>,
    items: Vec<HistoryItem>,
    rows: Vec<(Bucket, HistoryItem)>,
    sel: usize,
    _sub: Subscription,
}

impl HistoryPicker {
    /// 创建并聚焦搜索框。
    ///
    /// 参数:`items` 历史会话(按最近在前)。
    pub fn new(items: Vec<HistoryItem>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索会话…"));
        let sub = cx.subscribe_in(&input, window, |this: &mut Self, _, ev: &InputEvent, _, cx| match ev {
            InputEvent::Change => this.refilter(cx),
            InputEvent::PressEnter { .. } => this.accept(cx),
            _ => {}
        });
        input.update(cx, |i, cx| i.focus(window, cx));
        let rows = filter(&items, "");
        Self { input, items, rows, sel: 0, _sub: sub }
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        let q = self.input.read(cx).value().to_string();
        self.rows = filter(&self.items, &q);
        self.sel = 0;
        cx.notify();
    }

    fn accept(&mut self, cx: &mut Context<Self>) {
        if let Some((_, it)) = self.rows.get(self.sel) {
            cx.emit(HistoryEvent::Open(it.id.clone()));
        }
    }

    fn move_sel(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.rows.is_empty() {
            return;
        }
        self.sel = (self.sel as isize + delta).rem_euclid(self.rows.len() as isize) as usize;
        cx.notify();
    }
}

impl Render for HistoryPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let this = cx.entity();
        let mut list = v_flex().id("history-list").flex_1().min_h_0().overflow_y_scroll().px_1p5().pb_1p5();
        if self.rows.is_empty() {
            list = list.child(div().h(px(48.)).flex().items_center().justify_center().text_size(px(12.5)).text_color(t.muted_foreground).child("没有匹配的会话"));
        }
        let mut prev = None;
        for (i, (b, it)) in self.rows.iter().enumerate() {
            if prev != Some(*b) {
                list = list.child(div().h(px(28.)).px_2().mt(px(if prev.is_some() { 6. } else { 2. })).flex().items_center().text_size(px(11.5)).font_weight(FontWeight::MEDIUM).text_color(t.muted_foreground)
                    .child(if *b == Bucket::Recent { "过去 7 天" } else { "更早" }));
                prev = Some(*b);
            }
            let (this, id) = (this.clone(), it.id.clone());
            list = list.child(
                h_flex().id(SharedString::from(format!("hist-{i}"))).h(px(30.)).px_2().gap_2().items_center().rounded(px(8.)).cursor_pointer()
                    .when(i == self.sel, |d| d.bg(t.list_active))
                    .hover(|d| d.bg(t.list_hover))
                    .on_click(move |_, _, cx| { let id = id.clone(); this.update(cx, |_, cx| cx.emit(HistoryEvent::Open(id))); })
                    .child(icon(IconName::MessageSquare, 14., t.muted_foreground))
                    .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).child(it.title.clone()))
                    .when(it.open, |d| d.child(icon(IconName::Check, 12., t.primary)))
                    .child(div().flex_none().text_size(px(12.)).text_color(t.muted_foreground).child(it.updated.clone())),
            );
        }
        v_flex().w(px(340.)).max_h(px(460.)).rounded(px(12.)).border_1().border_color(t.border).bg(t.popover).shadow_lg().overflow_hidden().occlude()
            .on_mouse_down_out(cx.listener(|_, _, _, cx| cx.emit(HistoryEvent::Close)))
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| this.move_sel(-1, cx)))
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| this.move_sel(1, cx)))
            .capture_action(cx.listener(|_, _: &gpui_kit::component::input::Escape, _, cx| cx.emit(HistoryEvent::Close)))
            .child(
                div().p_2().child(
                    h_flex().h(px(34.)).px_2().items_center().rounded(px(8.)).border_1().border_color(t.primary.opacity(0.6)).bg(t.background)
                        .child(div().flex_1().child(Input::new(&self.input).appearance(false).bordered(false))),
                ),
            )
            .child(list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn it(title: &str, updated: &str) -> HistoryItem {
        HistoryItem { id: title.into(), title: title.into(), updated: updated.into(), open: false }
    }

    #[::core::prelude::v1::test]
    fn 按相对时间分组() {
        assert_eq!(bucket("刚刚"), Bucket::Recent);
        assert_eq!(bucket("5 分钟前"), Bucket::Recent);
        assert_eq!(bucket("昨天"), Bucket::Recent);
        assert_eq!(bucket("3 天前"), Bucket::Recent);
        assert_eq!(bucket("8 天前"), Bucket::Earlier);
        assert_eq!(bucket("1 周前"), Bucket::Earlier);
        assert_eq!(bucket("乱写"), Bucket::Earlier);
    }

    #[::core::prelude::v1::test]
    fn 过滤保持分组顺序且不分大小写() {
        let items = vec![it("旧的 Rust", "2 周前"), it("新的 rust", "1 天前"), it("别的", "1 天前")];
        let r = filter(&items, "RUST");
        assert_eq!(r.iter().map(|(_, i)| i.title.as_str()).collect::<Vec<_>>(), ["新的 rust", "旧的 Rust"]);
        assert_eq!(r[0].0, Bucket::Recent);
        assert_eq!(filter(&items, "").len(), 3);
    }
}
