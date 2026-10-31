//! 命令面板 / 中央搜索(⌘K):命令、会话、项目文件、演示场景统一模糊搜索。

use crate::ui::{MONO, icon};
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use crate::palette_search::{Segment, filter_items};
use ucode_config::ThemeChoice;

/// 选中某项后要执行的动作。
#[derive(Debug, Clone, PartialEq)]
pub enum PaletteAction {
    NewSession,
    ToggleLeft,
    ToggleRight,
    CycleMode,
    ToggleIde,
    OpenSettings(Option<&'static str>),
    OpenPage(&'static str),
    SetTheme(ThemeChoice),
    /// (项目下标, 会话 id)
    OpenSession(usize, String),
    OpenFile(String),
    /// 演示场景 id
    RunScenario(String),
}

/// 条目分组。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Command,
    Session,
    File,
    Scenario,
}

impl Group {
    fn label(self) -> &'static str {
        match self {
            Self::Command => "命令",
            Self::Session => "会话",
            Self::File => "文件",
            Self::Scenario => "演示场景",
        }
    }
}

/// 列表里的一项。
#[derive(Debug, Clone)]
pub struct PaletteItem {
    pub title: String,
    pub subtitle: String,
    pub group: Group,
    pub icon: IconName,
    pub action: PaletteAction,
}

/// 面板向工作台发出的事件。
pub enum PaletteEvent {
    Run(PaletteAction),
    Close,
}

impl EventEmitter<PaletteEvent> for Palette {}

pub struct Palette {
    input: Entity<InputState>,
    items: Vec<PaletteItem>,
    shown: Vec<PaletteItem>,
    segment: Segment,
    sel: usize,
    _sub: Subscription,
}

impl Palette {
    pub fn new(items: Vec<PaletteItem>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索命令、会话、文件…"));
        let sub = cx.subscribe_in(&input, window, |this: &mut Self, _, ev: &InputEvent, _, cx| match ev {
            InputEvent::Change => this.refilter(cx),
            InputEvent::PressEnter { .. } => this.run_selected(cx),
            _ => {}
        });
        input.update(cx, |i, cx| i.focus(window, cx));
        let shown = filter_items(&items, "", 50, Segment::All);
        Self { input, items, shown, segment: Segment::All, sel: 0, _sub: sub }
    }

    /// 预填查询(调试与外部调用)。
    pub fn set_query(&mut self, q: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |i, cx| i.set_value(q.to_string(), window, cx));
        self.refilter(cx);
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        let q = self.input.read(cx).value().to_string();
        self.shown = filter_items(&self.items, &q, 50, self.segment);
        self.sel = 0;
        cx.notify();
    }

    /// 切换分段并重新筛选。
    fn set_segment(&mut self, segment: Segment, cx: &mut Context<Self>) {
        self.segment = segment;
        self.refilter(cx);
    }

    fn run_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(item) = self.shown.get(self.sel) {
            cx.emit(PaletteEvent::Run(item.action.clone()));
        }
    }

    fn move_sel(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.shown.is_empty() {
            return;
        }
        let n = self.shown.len() as isize;
        self.sel = ((self.sel as isize + delta).rem_euclid(n)) as usize;
        cx.notify();
    }
}

impl Render for Palette {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let this = cx.entity();
        // 遮罩 + 居中偏上的面板
        div().absolute().inset_0().flex().justify_center().items_start().pt(px(88.))
            .bg(t.background.opacity(0.35))
            .child(div().absolute().inset_0().id("palette-backdrop").on_click(cx.listener(|_, _, _, cx| cx.emit(PaletteEvent::Close))))
            .child(
                v_flex().relative().w(px(620.)).max_h(px(480.)).rounded(px(14.)).border_1().border_color(t.border).bg(t.popover).shadow_lg().overflow_hidden()
                    .capture_action(cx.listener(|this, _: &MoveUp, _, cx| this.move_sel(-1, cx)))
                    .capture_action(cx.listener(|this, _: &MoveDown, _, cx| this.move_sel(1, cx)))
                    .capture_action(cx.listener(|_, _: &crate::workbench::Escape, _, cx| cx.emit(PaletteEvent::Close)))
                    .child(
                        h_flex().h(px(48.)).px_3().gap_2().items_center().border_b_1().border_color(t.border)
                            .child(icon(IconName::Search, 16., t.muted_foreground))
                            .child(div().flex_1().child(Input::new(&self.input).appearance(false).bordered(false))),
                    )
                    .child(
                        h_flex().px_3().py_1p5().gap_1().border_b_1().border_color(t.border).children(Segment::ALL.into_iter().map(|seg| {
                            let on = seg == self.segment;
                            div().id(SharedString::from(format!("pal-seg-{}", seg.label()))).px_2().h(px(22.)).flex().items_center().rounded(px(6.)).cursor_pointer()
                                .text_size(px(12.))
                                .when(on, |d| d.bg(t.list_active).font_weight(FontWeight::MEDIUM))
                                .when(!on, |d| d.text_color(t.muted_foreground).hover(|d| d.bg(t.list_hover)))
                                .on_click(cx.listener(move |this, _, _, cx| this.set_segment(seg, cx)))
                                .child(seg.label())
                        })),
                    )
                    .child(
                        v_flex().id("palette-list").flex_1().min_h_0().overflow_y_scroll().p_1()
                            .when(self.shown.is_empty(), |d| d.child(div().p_4().text_size(px(13.)).text_color(t.muted_foreground).child("没有匹配的结果")))
                            .children(self.shown.iter().enumerate().map(|(i, item)| {
                                let this = this.clone();
                                let action = item.action.clone();
                                h_flex().id(SharedString::from(format!("pal-{i}"))).h(px(34.)).px_2().gap_2p5().items_center().rounded(px(8.)).cursor_pointer()
                                    .when(i == self.sel, |d| d.bg(t.list_active))
                                    .hover(|d| d.bg(t.list_hover))
                                    .on_click(move |_, _, cx| { let a = action.clone(); this.update(cx, |_, cx| cx.emit(PaletteEvent::Run(a))); })
                                    .child(icon(item.icon, 15., t.muted_foreground))
                                    .child(div().text_size(px(13.5)).flex_none().max_w(px(300.)).truncate().child(item.title.clone()))
                                    .child(div().flex_1().min_w_0().truncate().text_size(px(12.)).font_family(if item.group == Group::File { MONO } else { ".SystemUIFont" }).text_color(t.muted_foreground).child(item.subtitle.clone()))
                                    .child(div().text_size(px(11.)).text_color(t.muted_foreground).child(item.group.label()))
                            })),
                    ),
            )
    }
}
