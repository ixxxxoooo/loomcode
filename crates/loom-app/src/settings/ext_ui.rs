//! 扩展类页面(插件 / MCP / 技能 / 命令 / 子 Agent / 规则 / Hooks)的统一版式:
//! 页头(图标方块 + 标题 + 说明 + 右侧操作)、分组小标题(带计数)、圆角卡片列表、条目行、空状态卡片、图标按钮。
//! 版式参考 OhMyAgent 的 `ExtensionScaffold`。
//!
//! @author ixxxxoooo

use crate::ui::{MONO, icon, pill};
use crate::workbench::Workbench;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::rc::Rc;

/// 带色调底的图标方块。
///
/// 参数:`name` 图标;`size` 方块边长;`tint` 色调(底色取 12% 透明度)。
pub fn tile(name: IconName, size: f32, tint: Hsla) -> Div {
    div().size(px(size)).flex_none().flex().items_center().justify_center().rounded(px(8.)).bg(tint.opacity(0.12)).child(icon(name, size * 0.46, tint))
}

/// 页头:图标方块、标题、一句话说明,右侧是操作区。
pub fn ext_header(name: IconName, title: &str, subtitle: &str, actions: impl IntoElement, cx: &App) -> Div {
    let t = cx.theme();
    h_flex().w_full().items_center().gap_3()
        .child(tile(name, 38., t.primary))
        .child(v_flex().flex_1().min_w_0().gap_0p5()
            .child(div().text_size(px(18.)).font_weight(FontWeight::SEMIBOLD).child(title.to_string()))
            .child(div().text_size(px(12.5)).text_color(t.muted_foreground).line_clamp(2).child(subtitle.to_string())))
        .child(h_flex().flex_none().gap_2().items_center().child(actions))
}

/// 分组小标题,可带条目数。
pub fn ext_label(title: impl Into<String>, count: Option<usize>, cx: &App) -> Div {
    let t = cx.theme();
    h_flex().gap_1p5().items_baseline().pt_2().pl_0p5()
        .child(div().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(t.muted_foreground).child(title.into()))
        .when_some(count, |d, n| d.child(div().text_size(px(11.)).text_color(t.muted_foreground.opacity(0.7)).child(n.to_string())))
}

/// 把一组行装进圆角卡片,行与行之间是缩进的细分隔线。
pub fn ext_card(rows: Vec<AnyElement>, cx: &App) -> Div {
    let t = cx.theme();
    v_flex().w_full().rounded(px(10.)).border_1().border_color(t.border).bg(t.popover).overflow_hidden()
        .children(rows.into_iter().enumerate().map(|(i, r)| {
            v_flex().w_full()
                .when(i > 0, |d| d.child(div().ml(px(56.)).h(px(1.)).bg(t.border.opacity(0.7))))
                .child(r)
        }))
}

/// 空状态卡片。
pub fn ext_empty(name: IconName, title: &str, message: &str, cx: &App) -> Div {
    let t = cx.theme();
    v_flex().w_full().items_center().gap_2().py_8().rounded(px(10.)).border_1().border_color(t.border).bg(t.popover)
        .child(icon(name, 26., t.muted_foreground.opacity(0.7)))
        .child(div().text_size(px(14.)).font_weight(FontWeight::MEDIUM).child(title.to_string()))
        .child(div().text_size(px(12.5)).text_color(t.muted_foreground).child(message.to_string()))
}

/// 小标签。`tint` 为 `None` 用中性色。
pub fn tag(text: impl Into<SharedString>, tint: Option<Hsla>, cx: &App) -> Div {
    let t = cx.theme();
    match tint {
        Some(c) => pill(text, c, c.opacity(0.12)),
        None => pill(text, t.muted_foreground, t.muted),
    }
}

/// 行点击回调。
pub type RowClick = Rc<dyn Fn(&mut Workbench, &mut Window, &mut Context<Workbench>)>;

/// 条目行的内容。
pub struct Row {
    pub id: SharedString,
    pub icon: IconName,
    pub tint: Option<Hsla>,
    pub title: String,
    pub badges: Vec<AnyElement>,
    pub subtitle: Option<String>,
    /// 说明用等宽字体(命令、地址)
    pub mono: bool,
    /// 说明最多显示几行
    pub lines: usize,
    pub dimmed: bool,
    pub trailing: Option<AnyElement>,
    /// 展开后显示在行下方的详情
    pub detail: Option<AnyElement>,
    pub on_click: Option<RowClick>,
}

impl Row {
    /// 新建一行,其余字段取默认值。
    pub fn new(id: impl Into<SharedString>, icon: IconName, title: impl Into<String>) -> Self {
        Self { id: id.into(), icon, tint: None, title: title.into(), badges: Vec::new(), subtitle: None, mono: false, lines: 2, dimmed: false, trailing: None, detail: None, on_click: None }
    }

    /// 渲染成元素:图标方块 + 名称与标签 + 说明 + 右侧控件;可点击的行悬停高亮。
    pub fn render(self, cx: &mut Context<Workbench>) -> AnyElement {
        let t = cx.theme().clone();
        let tint = self.tint.unwrap_or(t.primary);
        let clickable = self.on_click.is_some();
        let on_click = self.on_click.clone();
        v_flex().w_full()
            .child(
                h_flex().id(self.id.clone()).w_full().px_3p5().py_2p5().gap_3().items_center()
                    .when(self.dimmed, |d| d.opacity(0.55))
                    .when(clickable, |d| d.cursor_pointer().hover(|d| d.bg(t.list_hover)))
                    .when_some(on_click, |d, f| d.on_click(cx.listener(move |this, _, window, cx| f(this, window, cx))))
                    .child(tile(self.icon, 30., tint))
                    .child(v_flex().flex_1().min_w_0().gap_0p5()
                        .child(h_flex().gap_1p5().items_center().flex_wrap()
                            .child(div().text_size(px(13.5)).font_weight(FontWeight::MEDIUM).child(self.title))
                            .children(self.badges))
                        .when_some(self.subtitle.filter(|s| !s.is_empty()), |d, s| d.child(
                            div().text_size(px(12.)).text_color(t.muted_foreground).line_clamp(self.lines).when(self.mono, |d| d.font_family(MONO)).child(s))))
                    .when_some(self.trailing, |d, tr| d.child(div().flex_none().child(tr))),
            )
            .when_some(self.detail, |d, detail| d.child(div().pl(px(56.)).pr_3p5().pb_3().child(detail)))
            .into_any_element()
    }
}

/// 行内图标按钮(幽灵样式)。
pub fn icon_button(id: impl Into<ElementId>, name: IconName, tooltip: &'static str) -> Button {
    Button::new(id).small().ghost().icon(name).tooltip(tooltip)
}
