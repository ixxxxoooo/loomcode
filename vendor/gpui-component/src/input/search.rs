use ropey::Rope;
use std::ops::Range;

use gpui::{
    App, AppContext as _, Context, Empty, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render, Styled, Subscription,
    WeakEntity, Window, actions, div, prelude::FluentBuilder as _,
};

use crate::{
    ActiveTheme, Disableable, ElementExt, IconName, Selectable, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{
        Enter, Escape, IndentInline, Input, InputBaseState, InputEvent, InputState, OutdentInline,
        Replace,
    },
    v_flex,
};

const CONTEXT: &'static str = "SearchPanel";

actions!(input, [Tab]);

#[cfg(test)]
use gpui_base::input::SearchMatcher;

#[derive(Clone, Copy)]
#[cfg(test)]
enum MoveDirection {
    Up,
    Down,
}

#[cfg(test)]
fn next_scroll_direction(
    previous_match_ix: usize,
    current_match_ix: usize,
) -> Option<MoveDirection> {
    if current_match_ix <= previous_match_ix {
        None
    } else {
        Some(MoveDirection::Down)
    }
}

#[cfg(test)]
fn prev_scroll_direction(
    previous_match_ix: usize,
    current_match_ix: usize,
) -> Option<MoveDirection> {
    if current_match_ix >= previous_match_ix {
        None
    } else {
        Some(MoveDirection::Up)
    }
}

pub(super) struct SearchPanel<M: crate::input::overlay::OverlayMode> {
    editor: WeakEntity<InputBaseState<M>>,
    search_input: Entity<InputState>,
    replace_input: Entity<InputState>,
    session: gpui_base::input::SearchSession,
    input_width: Pixels,

    _subscriptions: Vec<Subscription>,
}

impl<M: crate::input::overlay::OverlayMode> SearchPanel<M> {
    pub(super) fn replace_mode(&self) -> bool {
        self.session.replace_mode
    }

    pub(super) fn sync_session(&mut self, session: &gpui_base::input::SearchSession) {
        self.session = session.clone();
    }

    /// The query the panel's search input currently holds.
    pub(super) fn query(&self, cx: &App) -> gpui::SharedString {
        self.search_input.read(cx).value()
    }

    /// The panel's search input, so a test can assert what re-invoking search
    /// does to its value and selection.
    #[cfg(test)]
    pub(super) fn search_input(&self) -> &Entity<InputState> {
        &self.search_input
    }

    pub(crate) fn new(
        editor: Entity<InputBaseState<M>>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let search_input = cx.new(|cx| InputState::new(window, cx));
        let replace_input = cx.new(|cx| InputState::new(window, cx));

        cx.new(|cx| {
            let _subscriptions =
                vec![
                    cx.subscribe(&search_input, |this: &mut Self, _, ev: &InputEvent, cx| {
                        // Handle search input changes
                        match ev {
                            InputEvent::Change => {
                                this.update_search_query(None, cx);
                            }
                            _ => {}
                        }
                    }),
                ];

            Self {
                editor: editor.downgrade(),
                search_input,
                replace_input,
                session: gpui_base::input::SearchSession::default(),
                input_width: Pixels::ZERO,
                _subscriptions,
            }
        })
    }

    pub(super) fn show_with_focus(
        &mut self,
        selected_text: &Rope,
        replace_mode: bool,
        visible_range_offset: Option<Range<usize>>,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.session.open = true;
        self.session.replace_mode = replace_mode;
        if focus {
            self.search_input
                .update(cx, |input, cx| input.focus(window, cx));
        }

        self.search_input.update(cx, |this, cx| {
            if selected_text.len() > 0 {
                this.set_value(selected_text.to_string(), window, cx);
            }
            this.select_all(window, cx);
        });

        // The `set_value` does not emit `InputEvent::Change`, so update the query
        // here to match the value of the search input.
        self.update_search_query(visible_range_offset, cx);
    }

    /// Update the matcher by the value of the search input.
    ///
    /// The `visible_range_offset` is to select the nearest match of the visible range,
    /// it is passed in, because the editor may be borrowed by the caller.
    fn update_search_query(
        &mut self,
        visible_range_offset: Option<Range<usize>>,
        cx: &mut Context<Self>,
    ) {
        let query = self.search_input.read(cx).value();
        let editor = self.editor.clone();
        let _ = editor.update(cx, |state, cx| {
            state.set_search_query(query.clone(), self.session.case_insensitive, cx);
        });
        if let Ok(session) = editor.read_with(cx, |state, _| state.search_session().clone()) {
            self.session = session;
        }
        if let Some(visible_range_offset) = visible_range_offset {
            self.session
                .matcher
                .update_cursor_by_offset(visible_range_offset.start);
        }
        cx.notify();
    }

    fn replaceable(&self, cx: &App) -> bool {
        self.editor
            .read_with(cx, |editor, _| editor.is_replaceable())
            .unwrap_or(false)
    }

    pub(super) fn hide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.hide_with_focus(true, window, cx);
    }

    pub(super) fn hide_with_focus(
        &mut self,
        focus_editor: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.session.open = false;
        let _ = self.editor.update(cx, |state, cx| {
            state.close_search(cx);
            if focus_editor {
                state.focus(window, cx);
            }
        });
        cx.notify();
    }

    fn on_action_enter(&mut self, action: &Enter, window: &mut Window, cx: &mut Context<Self>) {
        // 焦点在「替换」框里按 Enter = 替换当前匹配项(VS Code 一致)
        if self.session.replace_mode && self.replace_input.read(cx).focus_handle(cx).is_focused(window) {
            self.replace_next(window, cx);
            return;
        }
        if action.shift {
            self.prev(window, cx);
        } else {
            self.next(window, cx);
        }
    }

    fn on_action_escape(&mut self, _: &Escape, window: &mut Window, cx: &mut Context<Self>) {
        self.hide(window, cx);
    }

    fn on_action_tab(&mut self, _: &IndentInline, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle_focus(window, cx);
    }

    fn on_action_tab_prev(
        &mut self,
        _: &OutdentInline,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cycle_focus(window, cx);
    }

    /// Cycle focus between the search and the replace input, to keep the Tab key
    /// staying in the panel.
    ///
    /// There are only 2 inputs, so the forward and the backward are the same.
    fn cycle_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.session.replace_mode || !self.replaceable(cx) {
            return;
        }

        let search_focus_handle = self.search_input.read(cx).focus_handle(cx);
        let focus_handle = if search_focus_handle.is_focused(window) {
            self.replace_input.read(cx).focus_handle(cx)
        } else {
            search_focus_handle
        };
        focus_handle.focus(window, cx);
    }

    fn on_action_replace(&mut self, _: &Replace, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_replace_mode(window, cx);
    }

    /// Toggle the replace field, and move focus to the field that is going to be used.
    fn toggle_replace_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.replaceable(cx) {
            return;
        }

        self.session.replace_mode = !self.session.replace_mode;
        let replace_mode = self.session.replace_mode;
        let _ = self.editor.update(cx, |state, cx| {
            state.set_search_replace_mode(replace_mode, cx);
        });
        let focus_handle = if self.session.replace_mode {
            self.replace_input.read(cx).focus_handle(cx)
        } else {
            self.search_input.read(cx).focus_handle(cx)
        };
        focus_handle.focus(window, cx);
        cx.notify();
    }

    fn prev(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self.editor.update(cx, |state, cx| {
            _ = state.previous_search_match(cx);
        });
    }

    fn next(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self.editor.update(cx, |state, cx| {
            _ = state.next_search_match(cx);
        });
    }

    fn replace_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.replaceable(cx) {
            self.session.replace_mode = false;
            cx.notify();
            return;
        }

        let replacement = self.replace_input.read(cx).value();
        let _ = self.editor.update(cx, |state, cx| {
            _ = state.replace_current_search_match(&replacement, window, cx);
        });
    }

    fn replace_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.replaceable(cx) {
            self.session.replace_mode = false;
            cx.notify();
            return;
        }

        let replacement = self.replace_input.read(cx).value();
        let _ = self.editor.update(cx, |state, cx| {
            _ = state.replace_all_search_matches(&replacement, window, cx);
        });
    }
}

impl<M: crate::input::overlay::OverlayMode> Focusable for SearchPanel<M> {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.search_input.read(cx).focus_handle(cx)
    }
}

impl<M: crate::input::overlay::OverlayMode> Render for SearchPanel<M> {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.session.open {
            return Empty.into_any_element();
        }

        let has_matches = !self.session.matcher.is_empty();
        let allow_replace = self.replaceable(cx);
        if !allow_replace {
            self.session.replace_mode = false;
        }

        let theme = cx.theme();
        let (border, muted, popover) = (theme.border, theme.muted_foreground, theme.tokens.popover);
        let replace_on = self.session.replace_mode && allow_replace;
        let total = self.session.matcher.len();
        let has_query = !self.session.query.is_empty();
        // 对标 VS Code 查找小窗:无结果时 "无结果",否则 "第 N 项,共 M 项"
        let counter = if has_matches {
            format!("第 {} 项,共 {} 项", self.session.matcher.current_match_index() + 1, total)
        } else {
            "无结果".to_string()
        };
        let compact_input = |input: &Entity<InputState>| {
            Input::new(input).focus_bordered(false).xsmall().w_full().shadow_none()
        };

        // 浮在编辑器右上角的紧凑小窗(宿主把它绝对定位在编辑区上方),不再占满整行、不推挤正文
        h_flex()
            .id("search-panel")
            .occlude()
            .track_focus(&self.focus_handle(cx))
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::on_action_enter))
            .on_action(cx.listener(Self::on_action_escape))
            .on_action(cx.listener(Self::on_action_tab))
            .on_action(cx.listener(Self::on_action_tab_prev))
            .on_action(cx.listener(Self::on_action_replace))
            .font_family(cx.theme().font_family.clone())
            .items_start()
            .w(gpui::px(340.))
            .max_w_full()
            .gap_0p5()
            .py_1()
            .pr_1()
            .pl_0p5()
            .bg(popover)
            .border_1()
            .border_color(border)
            .rounded(gpui::px(6.))
            .shadow_md()
            // 左侧整列的展开 / 收起替换(VS Code 的 ▸ / ▾),不能替换时留一点空白
            .child(
                div().w(gpui::px(18.)).h_full().flex().items_start().justify_center().pt(gpui::px(1.)).child(
                    Button::new("replace-mode")
                        .xsmall()
                        .ghost()
                        .compact()
                        .icon(if replace_on { IconName::ChevronDown } else { IconName::ChevronRight })
                        .tooltip("切换替换")
                        .when(!allow_replace, |b| b.invisible())
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.toggle_replace_mode(window, cx);
                        })),
                ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        h_flex()
                            .w_full()
                            .gap_0p5()
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        compact_input(&self.search_input).suffix(
                                            Button::new("case-insensitive")
                                                .selected(!self.session.case_insensitive)
                                                .toggled(!self.session.case_insensitive)
                                                .xsmall()
                                                .compact()
                                                .text()
                                                .icon(IconName::CaseSensitive)
                                                .tooltip("区分大小写")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.session.case_insensitive =
                                                        !this.session.case_insensitive;
                                                    this.update_search_query(None, cx);
                                                    cx.notify();
                                                })),
                                        ),
                                    )
                                    .on_prepaint({
                                        let view = cx.entity();
                                        move |bounds, _, cx| {
                                            view.update(cx, |r, _| r.input_width = bounds.size.width)
                                        }
                                    }),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .min_w(gpui::px(64.))
                                    .px_1()
                                    .text_size(gpui::px(11.))
                                    .text_color(if has_query && !has_matches { theme.danger } else { muted })
                                    .whitespace_nowrap()
                                    .child(counter),
                            )
                            .child(
                                Button::new("prev")
                                    .xsmall()
                                    .ghost()
                                    .compact()
                                    .icon(IconName::ArrowUp)
                                    .tooltip("上一个匹配项 (⇧Enter)")
                                    .disabled(!has_matches)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.prev(window, cx);
                                    })),
                            )
                            .child(
                                Button::new("next")
                                    .xsmall()
                                    .ghost()
                                    .compact()
                                    .icon(IconName::ArrowDown)
                                    .tooltip("下一个匹配项 (Enter)")
                                    .disabled(!has_matches)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.next(window, cx);
                                    })),
                            )
                            .child(
                                Button::new("close")
                                    .xsmall()
                                    .ghost()
                                    .compact()
                                    .icon(IconName::Close)
                                    .tooltip("关闭 (Esc)")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.on_action_escape(&Escape, window, cx);
                                    })),
                            ),
                    )
                    .when(replace_on, |this| {
                        this.child(
                            h_flex()
                                .w_full()
                                .gap_0p5()
                                .items_center()
                                .child(div().flex_1().min_w_0().child(compact_input(&self.replace_input)))
                                .child(
                                    Button::new("replace-one")
                                        .xsmall()
                                        .ghost()
                                        .compact()
                                        .label("替换")
                                        .tooltip("替换 (Enter)")
                                        .disabled(!has_matches)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.replace_next(window, cx);
                                        })),
                                )
                                .child(
                                    Button::new("replace-all")
                                        .xsmall()
                                        .ghost()
                                        .compact()
                                        .label("全部替换")
                                        .tooltip("全部替换")
                                        .disabled(!has_matches)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.replace_all(window, cx);
                                        })),
                                ),
                        )
                    }),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ropey::Rope;

    #[test]
    fn test_search() {
        let mut matcher = SearchMatcher::new();
        matcher.update(&Rope::from("Hello 世界 this is a Is test string."));
        matcher.update_query("Is", true);

        assert_eq!(matcher.len(), 3);
        let mut matches = matcher.clone();
        assert_eq!(matches.current_match_index(), 0);
        assert_eq!(matches.next(), Some(18..20));
        assert_eq!(matches.next(), Some(23..25));
        assert_eq!(matches.current_match_index(), 2);
        assert_eq!(matches.next(), Some(15..17));
        assert_eq!(matches.current_match_index(), 0);
        assert_eq!(matches.next_back(), Some(23..25));
        assert_eq!(matches.current_match_index(), 2);
        assert_eq!(matches.next_back(), Some(18..20));
        assert_eq!(matches.current_match_index(), 1);
        assert_eq!(matches.next_back(), Some(15..17));
        assert_eq!(matches.current_match_index(), 0);
        assert_eq!(matches.next_back(), Some(23..25));

        matcher.update_query("IS", false);
        assert_eq!(matcher.len(), 0);
        assert_eq!(matcher.next(), None);
        assert_eq!(matcher.next_back(), None);
    }

    #[test]
    fn test_search_label() {
        let mut matcher = SearchMatcher::new();
        matcher.update(&Rope::from("Hello 世界 this is a Is test string."));
        matcher.update_query("Is", true);
        assert_eq!(matcher.label(), "1/3");
        matcher.next();
        assert_eq!(matcher.label(), "2/3");
        matcher.next();
        assert_eq!(matcher.label(), "3/3");
        matcher.next();
        assert_eq!(matcher.label(), "1/3");

        matcher.update_query("IS", false);
        assert_eq!(matcher.label(), "0/0");
    }

    #[test]
    fn test_select_range_start() {
        let mut matcher = SearchMatcher::new();
        matcher.update(&Rope::from(".....aaaaa.....aaaaa.....aaaaa"));
        matcher.update_query("aaaaa", false);
        matcher.update_cursor_by_offset(0);
        assert_eq!(matcher.current_match_index(), 0);

        matcher.update_cursor_by_offset(5);
        assert_eq!(matcher.current_match_index(), 0);

        matcher.update_cursor_by_offset(12);
        assert_eq!(matcher.current_match_index(), 1);

        matcher.update_cursor_by_offset(16);
        assert_eq!(matcher.current_match_index(), 1);

        matcher.update_cursor_by_offset(30);
        assert_eq!(matcher.current_match_index(), 2);

        matcher.update_cursor_by_offset(31);
        assert_eq!(matcher.current_match_index(), 2);
    }

    #[test]
    fn test_next_scroll_direction_returns_down_without_wrap() {
        assert!(matches!(
            next_scroll_direction(0, 1),
            Some(MoveDirection::Down)
        ));
    }

    #[test]
    fn test_next_scroll_direction_returns_none_on_wrap() {
        assert!(next_scroll_direction(2, 0).is_none());
    }

    #[test]
    fn test_next_scroll_direction_returns_none_for_single_match() {
        assert!(next_scroll_direction(0, 0).is_none());
    }

    #[test]
    fn test_prev_scroll_direction_returns_up_without_wrap() {
        assert!(matches!(
            prev_scroll_direction(2, 1),
            Some(MoveDirection::Up)
        ));
    }

    #[test]
    fn test_prev_scroll_direction_returns_none_on_wrap() {
        assert!(prev_scroll_direction(0, 2).is_none());
    }

    #[test]
    fn test_prev_scroll_direction_returns_none_for_single_match() {
        assert!(prev_scroll_direction(0, 0).is_none());
    }
}
