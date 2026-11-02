//! 终端内搜索(⌘F):搜索栏、匹配跳转与高亮。匹配在回看历史与当前画面里找,输出变化后自动刷新。
//!
//! @author ixxxxoooo

use crate::terminal_view::TerminalView;
use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme, Selectable, Sizable, h_flex};
use gpui_kit::*;
use loom_term::{SearchMatch, Snapshot};

/// 搜索状态;`TerminalView.find` 为 `Some` 表示搜索栏打开。
pub struct FindState {
    input: Entity<InputState>,
    matches: Vec<SearchMatch>,
    /// 当前匹配的下标
    current: usize,
    case_sensitive: bool,
    /// 上次搜索时的终端输出序号与时间(输出变了且距上次超过 0.3 秒才重搜)
    seq: u64,
    at: std::time::Instant,
    timer_pending: bool,
    _sub: Subscription,
}

/// 输出变化后重新搜索的最短间隔。
const REFIND_GAP: std::time::Duration = std::time::Duration::from_millis(300);

impl TerminalView {
    /// ⌘F:打开搜索栏并聚焦输入框;已打开就关掉并把焦点还给终端。
    pub(crate) fn toggle_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
            return;
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("在终端中查找"));
        let sub = cx.subscribe_in(&input, window, |this: &mut Self, _, ev: &InputEvent, _, cx| match ev {
            InputEvent::Change => this.refind(true, cx),
            InputEvent::PressEnter { .. } => this.step_find(1, cx),
            _ => {}
        });
        input.update(cx, |i, cx| i.focus(window, cx));
        self.find = Some(FindState { input, matches: Vec::new(), current: 0, case_sensitive: false, seq: 0, at: std::time::Instant::now(), timer_pending: false, _sub: sub });
        cx.notify();
    }

    /// 关闭搜索栏。
    pub(crate) fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    /// 重新搜索。`jump` 为真时跳到最靠近底部的一处(新查询);否则保持当前下标(输出刷新)。
    fn refind(&mut self, jump: bool, cx: &mut Context<Self>) {
        let Some(f) = self.find.as_mut() else { return };
        let q = f.input.read(cx).value().to_string();
        f.matches = self.term.search(&q, f.case_sensitive);
        f.seq = self.term.output_seq();
        f.at = std::time::Instant::now();
        f.current = if jump { f.matches.len().saturating_sub(1) } else { f.current.min(f.matches.len().saturating_sub(1)) };
        if jump {
            if let Some(m) = f.matches.get(f.current) {
                self.term.reveal_line(m.line);
            }
        }
        cx.notify();
    }

    /// 跳到下一处(`delta` 为 1)或上一处(-1),首尾循环。
    pub(crate) fn step_find(&mut self, delta: i32, cx: &mut Context<Self>) {
        let Some(f) = self.find.as_mut() else { return };
        let n = f.matches.len();
        if n == 0 {
            return;
        }
        f.current = (f.current as i32 + delta).rem_euclid(n as i32) as usize;
        self.term.reveal_line(f.matches[f.current].line);
        cx.notify();
    }

    /// 渲染前调用:终端有新输出且距上次搜索够久就重搜;不够久则排一个定时重绘。
    pub(crate) fn refresh_find(&mut self, cx: &mut Context<Self>) {
        let Some(f) = self.find.as_mut() else { return };
        if f.seq == self.term.output_seq() {
            return;
        }
        if f.at.elapsed() >= REFIND_GAP {
            self.refind(false, cx);
        } else if !f.timer_pending {
            f.timer_pending = true;
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(REFIND_GAP).await;
                let _ = this.update(cx, |this, cx| {
                    if let Some(f) = this.find.as_mut() {
                        f.timer_pending = false;
                    }
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// 当前画面里可见的匹配:(画面行, 起始列, 格数, 是否当前项)。
    pub(crate) fn visible_matches(&self, snap: &Snapshot) -> Vec<(usize, usize, usize, bool)> {
        let Some(f) = &self.find else { return Vec::new() };
        f.matches
            .iter()
            .enumerate()
            .filter_map(|(i, m)| {
                let row = m.line + snap.display_offset as i32;
                (row >= 0 && (row as usize) < snap.rows).then_some((row as usize, m.col, m.cells, i == f.current))
            })
            .collect()
    }

    /// 搜索栏(浮在终端右上角);没打开返回 `None`。
    pub(crate) fn find_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let f = self.find.as_ref()?;
        let t = cx.theme().clone();
        let has_query = !f.input.read(cx).value().is_empty();
        let count = if f.matches.is_empty() { if has_query { "无结果".to_string() } else { String::new() } } else { format!("{}/{}", f.current + 1, f.matches.len()) };
        let case = f.case_sensitive;
        Some(
            h_flex().absolute().top(px(6.)).right(px(14.)).w(px(330.)).h(px(32.)).px_2().gap_1().items_center().rounded(px(8.))
                .border_1().border_color(t.border).bg(t.popover).shadow_sm()
                .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                    if ev.keystroke.key == "escape" {
                        this.close_find(window, cx);
                        cx.stop_propagation();
                    }
                }))
                .child(icon(IconName::Search, 13., t.muted_foreground))
                .child(div().flex_1().min_w_0().child(Input::new(&f.input).appearance(false).bordered(false)))
                .child(div().min_w(px(44.)).text_size(px(11.)).text_color(t.muted_foreground).child(count))
                .child(Button::new("tf-case").xsmall().ghost().label("Aa").tooltip("区分大小写").selected(case)
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(f) = this.find.as_mut() {
                            f.case_sensitive = !f.case_sensitive;
                        }
                        this.refind(true, cx);
                    })))
                .child(Button::new("tf-prev").xsmall().ghost().icon(IconName::ChevronUp).tooltip("上一个").on_click(cx.listener(|this, _, _, cx| this.step_find(-1, cx))))
                .child(Button::new("tf-next").xsmall().ghost().icon(IconName::ChevronDown).tooltip("下一个(回车)").on_click(cx.listener(|this, _, _, cx| this.step_find(1, cx))))
                .child(Button::new("tf-close").xsmall().ghost().icon(IconName::Close).tooltip("关闭(Esc)").on_click(cx.listener(|this, _, window, cx| this.close_find(window, cx))))
                .into_any_element(),
        )
    }
}
