//! 设置页公共控件:页头、分组卡片、设置行、分段选择器、筛选 chip、提示条。
//!
//! 版式约定(对标 OhMyAgent / Zed 的设置页):分组标题在卡片外,卡片是圆角 + 发丝边框 + 浮层底色,
//! 卡片里的行之间用细分隔线,行内边距固定 `px 14 / py 10`;所有设置页都只用这里的控件,不再手写边框盒。
//!
//! @author ixxxxoooo

use crate::theme::{R_LG, R_MD, R_SM};
use crate::ui::caption;
use crate::workbench::Workbench;
use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// 把若干元素收成 `Vec<AnyElement>`,给 [`group`] / [`card`] 用。
macro_rules! rows {
    ($($e:expr),* $(,)?) => { vec![$($e.into_any_element()),*] };
}
pub(crate) use rows;

/// 页头:大标题 + 一句话说明,右侧是操作区。
///
/// 参数:`title` 标题;`subtitle` 说明(可空);`actions` 右侧操作。返回:页头元素。
pub fn page_header(title: &str, subtitle: &str, actions: impl IntoElement, cx: &App) -> Div {
    h_flex().w_full().items_center().gap_3().pb_1()
        .child(v_flex().flex_1().min_w_0().gap_1()
            .child(div().text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).child(title.to_string()))
            .when(!subtitle.is_empty(), |d| d.child(div().text_size(px(12.5)).text_color(cx.theme().muted_foreground).child(subtitle.to_string()))))
        .child(h_flex().flex_none().gap_2().items_center().child(actions))
}

/// 一行设置:左标题与说明,右控件。放进 [`card`] / [`group`] 使用,行本身没有边框。
///
/// 参数:`title` 标题;`desc` 说明(可空);`control` 右侧控件。返回:行元素。
pub fn row(title: &str, desc: &str, control: impl IntoElement, cx: &App) -> Div {
    h_flex().w_full().px(px(14.)).py(px(10.)).gap_4().items_center()
        .child(v_flex().flex_1().min_w_0().gap_0p5()
            .child(div().text_size(px(13.)).child(title.to_string()))
            .when(!desc.is_empty(), |d| d.child(caption(desc.to_string(), cx))))
        .child(div().flex_none().child(control))
}

/// 通栏的一行:标题与说明在上,内容(输入框、列表)占满整行。
pub fn block_row(title: &str, desc: &str, body: impl IntoElement, cx: &App) -> Div {
    v_flex().w_full().px(px(14.)).py(px(10.)).gap_2()
        .child(v_flex().gap_0p5()
            .child(div().text_size(px(13.)).child(title.to_string()))
            .when(!desc.is_empty(), |d| d.child(caption(desc.to_string(), cx))))
        .child(body)
}

/// 卡片里的一个紧凑条目(规则、变量、进程):只给内边距,内容自己排。
pub fn item(cx: &App) -> Div {
    let _ = cx;
    h_flex().w_full().px(px(14.)).min_h(px(36.)).gap_2().items_center()
}

/// 圆角卡片:把一组行装起来,行之间是分隔线。
///
/// 参数:`rows` 行元素;`indent` 分隔线左缩进(带图标方块的列表用 56,其余 0)。返回:卡片元素。
pub fn card(rows: Vec<AnyElement>, indent: f32, cx: &App) -> Div {
    let t = cx.theme();
    v_flex().w_full().rounded(px(R_LG)).border_1().border_color(t.border).bg(t.popover).overflow_hidden()
        .children(rows.into_iter().enumerate().map(|(i, r)| {
            v_flex().w_full()
                .when(i > 0, |d| d.child(div().ml(px(indent)).h(px(1.)).bg(t.border.opacity(0.7))))
                .child(r)
        }))
}

/// 分组:卡片外的小标题 + 卡片。
pub fn group(title: &str, rows: Vec<AnyElement>, cx: &App) -> Div {
    group_desc(title, "", rows, cx)
}

/// 带说明的分组。
pub fn group_desc(title: &str, desc: &str, rows: Vec<AnyElement>, cx: &App) -> Div {
    let t = cx.theme();
    v_flex().w_full().pt_5().gap_1p5()
        .child(v_flex().gap_0p5().px_1()
            .child(div().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(t.muted_foreground).child(title.to_string()))
            .when(!desc.is_empty(), |d| d.child(div().text_size(px(11.)).text_color(t.muted_foreground.opacity(0.8)).child(desc.to_string()))))
        .child(card(rows, 0., cx))
}

/// 卡片外的脚注(生效时机、取舍说明)。
pub fn note(text: impl Into<SharedString>, cx: &App) -> Div {
    div().w_full().px_1().pt_2().text_size(px(11.)).text_color(cx.theme().muted_foreground).child(text.into())
}

/// 指标卡:标签 + 大号数值,用在用量页。
pub fn stat(label: &str, value: String, cx: &App) -> Div {
    let t = cx.theme();
    v_flex().flex_1().min_w(px(140.)).px_3().py(px(10.)).gap_1().rounded(px(R_LG)).border_1().border_color(t.border).bg(t.popover)
        .child(div().text_size(px(11.5)).text_color(t.muted_foreground).child(label.to_string()))
        .child(div().text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).child(value))
}

/// 分段选择器。
///
/// 参数:`id` 元素 id 前缀;`options` (值, 标签) 列表;`current` 当前值;`on` 选中回调。返回:选择器元素。
pub fn segmented<T: Copy + PartialEq + 'static>(
    id: &'static str,
    options: Vec<(T, &'static str)>,
    current: T,
    cx: &mut Context<Workbench>,
    on: impl Fn(&mut Workbench, T, &mut Context<Workbench>) + Clone + 'static,
) -> Div {
    let t = cx.theme().clone();
    h_flex().p(px(2.)).gap_0p5().rounded(px(R_MD)).bg(t.muted)
        .children(options.into_iter().enumerate().map(|(i, (v, label))| {
            let on = on.clone();
            let active = v == current;
            div().id(SharedString::from(format!("{id}-{i}"))).px_2p5().h(px(24.)).flex().items_center().rounded(px(R_SM)).cursor_pointer().text_size(px(12.))
                .when(active, |d| d.bg(t.background).shadow_sm().text_color(t.foreground))
                .when(!active, |d| d.text_color(t.muted_foreground).hover(|d| d.text_color(t.foreground)))
                .on_click(cx.listener(move |this, _, _, cx| on(this, v, cx)))
                .child(label)
        }))
}

/// 筛选 chip 行(自定义页标签、MCP 范围筛选):选中为淡主色底 + 主色细描边,24 高。
///
/// 参数:`id` 元素 id 前缀;`options` (值, 标签) 列表;`current` 当前值;`on` 选中回调。返回:chip 行。
pub fn chips<T: Copy + PartialEq + 'static>(
    id: &'static str,
    options: Vec<(T, String)>,
    current: T,
    cx: &mut Context<Workbench>,
    on: impl Fn(&mut Workbench, T, &mut Context<Workbench>) + Clone + 'static,
) -> Div {
    let t = cx.theme().clone();
    h_flex().gap_1p5().flex_wrap()
        .children(options.into_iter().enumerate().map(|(i, (v, label))| {
            let on = on.clone();
            let active = v == current;
            h_flex().id(SharedString::from(format!("{id}-{i}"))).h(px(26.)).px_2p5().items_center().rounded(px(R_SM + 1.)).border_1().cursor_pointer().text_size(px(12.5))
                .when(active, |d| d.bg(t.primary.opacity(0.12)).border_color(t.primary.opacity(0.4)).text_color(t.primary).font_weight(FontWeight::MEDIUM))
                .when(!active, |d| d.bg(t.muted.opacity(0.6)).border_color(t.border).text_color(t.muted_foreground).hover(|d| d.bg(t.list_hover).text_color(t.foreground)))
                .on_click(cx.listener(move |this, _, _, cx| on(this, v, cx)))
                .child(label)
        }))
}

/// 提示条的语气。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Danger,
    Success,
    Info,
}

/// 提示条:语气色淡底 + 细描边。
pub fn banner(tone: Tone, text: impl Into<SharedString>, cx: &App) -> Div {
    let t = cx.theme();
    let c = match tone {
        Tone::Danger => t.danger,
        Tone::Success => t.success,
        Tone::Info => t.info,
    };
    div().w_full().mt_3().px_3().py_2().rounded(px(R_MD)).border_1().border_color(c.opacity(0.3)).bg(c.opacity(0.08)).text_color(c).text_size(px(12.5)).child(text.into())
}

/// 写配置失败时的红色提示条。
pub fn error_banner(wb: &Workbench, cx: &App) -> Option<Div> {
    wb.forms().error.clone().map(|e| banner(Tone::Danger, e, cx))
}

/// 配置层选择条:「配置层 [用户|项目|本地]」+ 当前层文件路径;没有项目时只能选用户层。
///
/// 参数:`wb` 工作台;`target` 当前层要写入的文件说明。返回:选择条元素。
pub fn scope_header(wb: &Workbench, target: String, cx: &mut Context<Workbench>) -> Div {
    use loom_config::Scope;
    let has_project = wb.claude_cfg_root.is_some();
    let options: Vec<(Scope, &'static str)> = Scope::ALL.into_iter().filter(|s| has_project || !s.needs_project()).map(|s| (s, s.label())).collect();
    let scope = wb.effective_scope();
    v_flex().w_full().pt_4().gap_1()
        .child(h_flex().w_full().gap_3().items_center()
            .child(div().text_size(px(13.)).child("配置层"))
            .child(segmented("cfg-scope", options, scope, cx, |this, v, cx| { this.cfg_scope = v; cx.notify(); }))
            .when(!has_project, |d| d.child(caption("打开项目会话后可选项目层 / 本地层", cx))))
        .child(caption(target, cx))
}

/// 表单内的紧凑配置层选择:「写入到 [用户|项目|本地]」;没有项目时只能选用户层。
pub fn scope_chooser(wb: &Workbench, cx: &mut Context<Workbench>) -> Div {
    use loom_config::Scope;
    let has_project = wb.claude_cfg_root.is_some();
    let options: Vec<(Scope, &'static str)> = Scope::ALL.into_iter().filter(|s| has_project || !s.needs_project()).map(|s| (s, s.label())).collect();
    h_flex().gap_3().items_center()
        .child(div().text_size(px(12.5)).child("写入到"))
        .child(segmented("form-scope", options, wb.effective_scope(), cx, |this, v, cx| { this.cfg_scope = v; cx.notify(); }))
        .when(!has_project, |d| d.child(caption("打开项目会话后可选项目层 / 本地层", cx)))
}

/// 数值步进器:`[−] 值 单位 [+]`,在 `min..=max` 内按 `step` 增减。
///
/// 参数:`id` 元素 id 前缀;`value` 当前值;`unit` 单位文字;`on` 变化回调。返回:步进器元素。
pub fn stepper(
    id: &'static str,
    value: i64,
    (min, max, step): (i64, i64, i64),
    unit: &'static str,
    cx: &mut Context<Workbench>,
    on: impl Fn(&mut Workbench, i64, &mut Context<Workbench>) + Clone + 'static,
) -> Div {
    let t = cx.theme().clone();
    let (on_dec, on_inc) = (on.clone(), on);
    h_flex().gap_1().items_center()
        .child(Button::new(SharedString::from(format!("{id}-dec"))).xsmall().outline().icon(IconName::Minus).disabled(value <= min)
            .on_click(cx.listener(move |this, _, _, cx| on_dec(this, (value - step).max(min), cx))))
        .child(div().min_w(px(64.)).flex().justify_center().text_size(px(12.5)).text_color(t.foreground).child(format!("{value}{unit}")))
        .child(Button::new(SharedString::from(format!("{id}-inc"))).xsmall().outline().icon(IconName::Plus).disabled(value >= max)
            .on_click(cx.listener(move |this, _, _, cx| on_inc(this, (value + step).min(max), cx))))
}

/// 开关行:标题与说明在左,开关在右;`on` 收到新状态。
pub fn switch_row(
    id: &'static str,
    title: &str,
    desc: &str,
    checked: bool,
    cx: &mut Context<Workbench>,
    on: impl Fn(&mut Workbench, bool, &mut Context<Workbench>) + 'static,
) -> Div {
    let sw = Switch::new(id).checked(checked).on_click(cx.listener(move |this, c: &bool, _, cx| on(this, *c, cx)));
    row(title, desc, sw, cx)
}
