//! 命令面板 / 中央搜索(⌘K):命令、会话、项目文件、演示场景统一模糊搜索。

use crate::ui::{MONO, icon};
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use crate::palette_search::{Segment, filter_items};
use loom_config::ThemeChoice;

/// 选中某项后要执行的动作。
#[derive(Debug, Clone, PartialEq)]
pub enum PaletteAction {
    NewSession,
    ToggleLeft,
    ToggleRight,
    CycleMode,
    ToggleIde,
    OpenSettings(Option<&'static str>),
    /// 打开「自定义」页的某个标签(plugins / mcp / skills / commands / agents / rules / hooks)
    OpenCustomize(&'static str),
    OpenPage(&'static str),
    SetTheme(ThemeChoice),
    /// 打开主题二级列表(面板内部处理:↑↓ 实时预览,回车应用,Esc 还原)
    PickTheme,
    /// (项目下标, 会话 id)
    OpenSession(usize, String),
    /// 项目下标:展开并打开最近会话(没有则新建)
    OpenProject(usize),
    OpenFile(String),
    /// 演示场景 id:从头按演示速度播放
    RunScenario(String),
    /// 重新播放当前会话对应的演示场景
    ReplayScenario,
    /// 设置演示播放的慢放倍数(1 = 正常速度,越大越慢)
    DemoSlow(u32),
    /// 演示 ↔ 真实一键切换(确认后重启)
    SwitchMode,
    /// IDE 窗口的内置命令(键为命令名,由 `ide_quick` 执行)
    Ide(&'static str),
}

/// 条目分组。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Command,
    Session,
    Project,
    File,
    /// 设置 / 自定义页的跳转:只在输入查询或「命令」分段里出现,欢迎列表里不展示,免得淹没常用命令
    Setting,
    /// 主题二级列表里的条目
    Theme,
    /// 最近打开的文件:只在无查询时显示,有查询时由「文件」分组承担
    Recent,
    Scenario,
}

impl Group {
    fn label(self) -> &'static str {
        match self {
            Self::Command => "命令",
            Self::Session => "会话",
            Self::Project => "项目",
            Self::File => "文件",
            Self::Setting => "设置",
            Self::Theme => "主题",
            Self::Recent => "最近文件",
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
    /// 主题列表里选中项变了(或关闭时还原):宿主临时套用该主题,不写入设置
    PreviewTheme(ThemeChoice),
    Close,
}

impl EventEmitter<PaletteEvent> for Palette {}

pub struct Palette {
    input: Entity<InputState>,
    items: Vec<PaletteItem>,
    shown: Vec<PaletteItem>,
    segment: Segment,
    sel: usize,
    /// 当前查询词(用来过滤掉没有实际变化的 Change 事件)
    query: String,
    /// 列表滚动句柄:键盘移动选中项时保证它可见
    scroll: ScrollHandle,
    /// 处于主题二级列表时,记着进入前生效的主题(关闭时还原预览)
    theme_mode: Option<ThemeChoice>,
    _sub: Subscription,
}

impl Palette {
    pub fn new(items: Vec<PaletteItem>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索命令、会话、项目、文件…"));
        let sub = cx.subscribe_in(&input, window, |this: &mut Self, _, ev: &InputEvent, window, cx| match ev {
            InputEvent::Change => {
                let q = this.input.read(cx).value().to_string();
                if q != this.query {
                    this.query = q;
                    this.refilter(cx);
                }
            }
            InputEvent::PressEnter { .. } => this.run_selected(window, cx),
            _ => {}
        });
        input.update(cx, |i, cx| i.focus(window, cx));
        let shown = filter_items(&items, "", 50, Segment::All);
        Self { input, items, shown, segment: Segment::All, sel: 0, query: String::new(), scroll: ScrollHandle::new(), theme_mode: None, _sub: sub }
    }

    /// 预填查询(调试与外部调用);`@theme` 直接进入主题列表。
    pub fn set_query(&mut self, q: &str, window: &mut Window, cx: &mut Context<Self>) {
        if q == "@theme" {
            self.enter_theme_mode(window, cx);
            return;
        }
        self.query = q.to_string();
        self.input.update(cx, |i, cx| i.set_value(q.to_string(), window, cx));
        self.refilter(cx);
    }

    /// 按当前查询与分段重新筛选,选中第一项并滚回顶部;主题列表里同时预览第一项。
    fn refilter(&mut self, cx: &mut Context<Self>) {
        self.shown = filter_items(&self.items, &self.query, 50, self.segment);
        self.sel = 0;
        self.scroll.set_offset(point(px(0.), px(0.)));
        self.preview_selected(cx);
        cx.notify();
    }

    /// 切换分段并重新筛选。
    fn set_segment(&mut self, segment: Segment, cx: &mut Context<Self>) {
        self.segment = segment;
        self.refilter(cx);
    }

    /// 关闭面板;主题列表里先把预览还原成进入前的主题。
    fn close(&mut self, cx: &mut Context<Self>) {
        if let Some(orig) = self.theme_mode {
            cx.emit(PaletteEvent::PreviewTheme(orig));
        }
        cx.emit(PaletteEvent::Close);
    }

    /// 主题列表里把当前选中项交给宿主预览(没有匹配项时还原)。
    fn preview_selected(&self, cx: &mut Context<Self>) {
        let Some(orig) = self.theme_mode else { return };
        let c = match self.shown.get(self.sel).map(|i| &i.action) {
            Some(PaletteAction::SetTheme(c)) => *c,
            _ => orig,
        };
        cx.emit(PaletteEvent::PreviewTheme(c));
    }

    /// 执行一个条目:「主题:切换…」由面板自己展开二级列表,其余交给宿主。
    fn activate(&mut self, action: PaletteAction, window: &mut Window, cx: &mut Context<Self>) {
        match action {
            PaletteAction::PickTheme => self.enter_theme_mode(window, cx),
            other => cx.emit(PaletteEvent::Run(other)),
        }
    }

    /// 切到主题二级列表:列出全部主题,选中当前主题,清空输入框。
    fn enter_theme_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let cur = crate::prefs(cx).theme;
        self.theme_mode = Some(cur);
        self.items = ThemeChoice::ALL
            .into_iter()
            .map(|c| PaletteItem {
                title: crate::theme::preview(c).0.into(),
                subtitle: if c == cur { "当前".into() } else { String::new() },
                group: Group::Theme,
                icon: match crate::theme::is_dark_theme(c) {
                    Some(true) => IconName::Moon,
                    Some(false) => IconName::Sun,
                    None => IconName::Monitor,
                },
                action: PaletteAction::SetTheme(c),
            })
            .collect();
        self.segment = Segment::All;
        // 先改 query 再清输入框:随后的 Change 事件发现查询没变就不会重筛(否则会把选中项重置并误预览第一个主题)
        self.query.clear();
        self.input.update(cx, |i, cx| {
            i.set_value("", window, cx);
            i.set_placeholder("选择主题,↑↓ 实时预览…", window, cx);
        });
        self.shown = filter_items(&self.items, "", 50, Segment::All);
        self.sel = self.shown.iter().position(|i| i.action == PaletteAction::SetTheme(cur)).unwrap_or(0);
        self.scroll.scroll_to_item(self.sel);
        cx.notify();
    }

    fn run_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(item) = self.shown.get(self.sel) {
            let action = item.action.clone();
            self.activate(action, window, cx);
        }
    }

    fn move_sel(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.shown.is_empty() {
            return;
        }
        let n = self.shown.len() as isize;
        self.sel = ((self.sel as isize + delta).rem_euclid(n)) as usize;
        self.scroll.scroll_to_item(self.sel);
        self.preview_selected(cx);
        cx.notify();
    }
}

impl Render for Palette {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let this = cx.entity();
        let theme_mode = self.theme_mode.is_some();
        // 无查询时按分组显示小标题(命令 / 会话 / 最近文件…),有查询时按得分排序,右侧标签说明来源
        let headers = !theme_mode && self.query.trim().is_empty();
        // 遮罩 + 居中偏上的面板
        div().absolute().inset_0().flex().justify_center().items_start().pt(px(88.))
            .bg(t.background.opacity(0.35))
            .child(div().absolute().inset_0().id("palette-backdrop").on_click(cx.listener(|this, _, _, cx| this.close(cx))))
            .child(
                v_flex().relative().occlude().w(px(620.)).max_h(px(520.)).rounded(px(14.)).border_1().border_color(t.border).bg(t.popover).shadow_lg().overflow_hidden()
                    .capture_action(cx.listener(|this, _: &MoveUp, _, cx| this.move_sel(-1, cx)))
                    .capture_action(cx.listener(|this, _: &MoveDown, _, cx| this.move_sel(1, cx)))
                    .capture_action(cx.listener(|this, _: &crate::workbench::Escape, _, cx| this.close(cx)))
                    .child(
                        h_flex().h(px(48.)).px_3().gap_2().items_center().border_b_1().border_color(t.border)
                            .child(icon(IconName::Search, 16., t.muted_foreground))
                            .child(div().flex_1().child(Input::new(&self.input).appearance(false).bordered(false))),
                    )
                    .when(!theme_mode, |d| {
                        d.child(
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
                    })
                    .child(
                        v_flex().id("palette-list").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.scroll).p_1()
                            .when(self.shown.is_empty(), |d| d.child(div().p_4().text_size(px(13.)).text_color(t.muted_foreground).child("没有匹配的结果")))
                            .children(self.shown.iter().enumerate().map(|(i, item)| {
                                let this = this.clone();
                                let action = item.action.clone();
                                let new_group = headers && (i == 0 || self.shown[i - 1].group != item.group);
                                // 每个条目(含它前面的分组标题)包成一个子元素,滚动句柄按下标定位
                                v_flex()
                                    .when(new_group, |d| d.child(div().px_2().pt(if i == 0 { px(4.) } else { px(8.) }).pb_1().text_size(px(11.)).text_color(t.muted_foreground).child(item.group.label())))
                                    .child(
                                        h_flex().id(SharedString::from(format!("pal-{i}"))).h(px(34.)).px_2().gap_2p5().items_center().rounded(px(8.)).cursor_pointer()
                                            .when(i == self.sel, |d| d.bg(t.list_active))
                                            // 鼠标移上去就把选中项移过来,避免「悬停高亮」与「键盘选中高亮」同时出现两条
                                            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                                                if this.sel != i {
                                                    this.sel = i;
                                                    cx.notify();
                                                }
                                            }))
                                            .on_click(move |_, window, cx| { let a = action.clone(); this.update(cx, |p, cx| p.activate(a, window, cx)); })
                                            .child(icon(item.icon, 15., t.muted_foreground))
                                            .child(div().text_size(px(13.5)).flex_none().max_w(px(300.)).truncate().child(item.title.clone()))
                                            .child(div().flex_1().min_w_0().truncate().text_size(px(12.)).font_family(if item.group == Group::File { MONO } else { ".SystemUIFont" }).text_color(t.muted_foreground).child(item.subtitle.clone()))
                                            .when(!headers && !theme_mode, |d| d.child(div().text_size(px(11.)).text_color(t.muted_foreground).child(item.group.label()))),
                                    )
                            })),
                    )
                    .child(
                        h_flex().h(px(26.)).px_3().gap_3().items_center().border_t_1().border_color(t.border).text_size(px(11.)).text_color(t.muted_foreground)
                            .child(if theme_mode { "↑↓ 实时预览" } else { "↑↓ 选择" })
                            .child(if theme_mode { "↵ 应用" } else { "↵ 执行" })
                            .child(if theme_mode { "Esc 还原并关闭" } else { "Esc 关闭" }),
                    ),
            )
    }
}
