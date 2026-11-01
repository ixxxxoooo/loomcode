//! 与 Claude Code 配置相关的公共部件(来源层标签、Markdown 编辑器)与记忆页。
//!
//! @author ixxxxoooo

use super::CustomizeTab;
use super::ext_ui::{Row, ext_card, ext_header, ext_label, tag};
use super::widgets::error_banner;
use crate::ui::{MONO, icon, pill};
use crate::workbench::Workbench;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Editor;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_config::Scope;

/// 来源层小标签。
pub fn scope_pill(scope: Scope, cx: &App) -> impl IntoElement {
    let t = cx.theme();
    pill(scope.label(), t.muted_foreground, t.muted)
}

/// 共用的 Markdown 编辑器:路径 + 保存 / 关闭 + 编辑区。只显示属于 `tab` 的文件。
///
/// 参数:`wb` 工作台;`tab` 当前自定义标签;`closable` 是否显示关闭按钮。返回:编辑器元素,没有打开文件时为 `None`。
pub fn doc_editor(wb: &Workbench, tab: CustomizeTab, closable: bool, cx: &mut Context<Workbench>) -> Option<Div> {
    let t = cx.theme().clone();
    let doc = wb.forms().doc.as_ref().filter(|d| d.tab == tab)?;
    let saved = doc.saved;
    let exists = doc.path.exists();
    Some(v_flex().w_full().pt_4().gap_2()
        .child(h_flex().gap_2().items_center()
            .child(icon(IconName::FileText, 13., t.muted_foreground))
            .child(div().flex_1().min_w_0().truncate().font_family(MONO).text_size(px(12.)).text_color(t.muted_foreground).child(doc.path.display().to_string()))
            .when(!exists, |d| d.child(pill("保存后创建", t.warning, t.muted)))
            .when(closable, |d| d.child(Button::new("doc-close").small().ghost().icon(IconName::ArrowLeft).label("返回").on_click(cx.listener(|this, _, _, cx| this.close_doc(cx)))))
            .child(Button::new("doc-save").small().when(!saved, |b| b.primary()).when(saved, |b| b.ghost()).label(if saved { "已保存" } else { "保存" })
                .on_click(cx.listener(|this, _, _, cx| this.save_doc(cx)))))
        .child(div().w_full().h(px(440.)).rounded(px(8.)).border_1().border_color(t.border).overflow_hidden()
            .child(Editor::new(&doc.editor).bordered(false).appearance(false).size_full())))
}

/// 规则页(记忆文件):页头 + 按层分组的文件列表(按 Claude 的加载顺序),点选后在下方编辑。
pub fn memory(wb: &mut Workbench, window: &mut Window, cx: &mut Context<Workbench>) -> AnyElement {
    let tab = CustomizeTab::Rules;
    let files = wb.forms().memory_files.clone();
    // 第一次进入:默认打开项目 CLAUDE.md(没有项目时打开用户级)
    if wb.forms().doc.as_ref().is_none_or(|d| d.tab != tab) {
        if let Some(f) = files.iter().find(|f| f.group == "项目").or_else(|| files.first()) {
            wb.open_doc(f.path.clone(), tab, window, cx);
        }
    }
    let t = cx.theme().clone();
    let current = wb.forms().doc.as_ref().map(|d| d.path.clone());
    let mut groups: Vec<(&'static str, Vec<AnyElement>)> = Vec::new();
    for (i, f) in files.into_iter().enumerate() {
        let path = f.path.clone();
        let mut row = Row::new(format!("mem-{i}"), IconName::FileText, f.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
        row.tint = Some(if f.exists { t.primary } else { t.muted_foreground });
        row.subtitle = Some(format!("{} · {}", f.label, super::fields::tilde(&f.path)));
        row.lines = 1;
        if !f.exists {
            row.badges.push(tag("未创建", None, cx).into_any_element());
        }
        if current.as_ref() == Some(&f.path) {
            row.badges.push(tag("编辑中", Some(t.primary), cx).into_any_element());
        }
        row.on_click = Some(std::rc::Rc::new(move |this, window, cx| this.open_doc(path.clone(), tab, window, cx)));
        let el = row.render(cx);
        match groups.last_mut() {
            Some((g, rows)) if *g == f.group => rows.push(el),
            _ => groups.push((f.group, vec![el])),
        }
    }
    let mut list = v_flex().w_full().gap_2();
    for (g, rows) in groups {
        list = list.child(ext_label(g, Some(rows.len()), cx)).child(ext_card(rows, cx));
    }
    v_flex().w_full().gap_3()
        .child(ext_header(IconName::Brain, "规则", "Claude 启动时按从上到下的顺序加载这些记忆文件,后加载的优先。规则文件放在 .claude/rules/*.md;AGENTS.md 不会自动加载,需在 CLAUDE.md 里写 @AGENTS.md 导入。", div(), cx))
        .children(error_banner(wb, cx))
        .child(list)
        .children(doc_editor(wb, tab, false, cx))
        .into_any_element()
}
