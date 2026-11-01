//! 斜杠命令 / 子 Agent / Skills 管理页(三者共用):按层列出、新建、编辑、移到废纸篓。
//!
//! @author ixxxxoooo

use super::CustomizeTab;
use super::claude_pages::{doc_editor, scope_pill};
use super::widgets::{error_banner, page_title, section_title, segmented};
use crate::ui::{MONO, caption, icon};
use crate::workbench::Workbench;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_config::Scope;
use loom_config::items::{Item, ItemKind};

/// 页面说明。
fn intro(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Command => "~/.claude/commands 与 <项目>/.claude/commands 下的 Markdown,在输入框里用 /名称 调用,子目录名会变成 目录:名称;同名时项目层优先",
        ItemKind::Agent => "~/.claude/agents 与 <项目>/.claude/agents 下的 Markdown,frontmatter 里写 name、description、tools、model。内置的 Explore、Plan、general-purpose 不在这里",
        ItemKind::Skill => "~/.claude/skills/<名称>/SKILL.md 与项目 .claude/skills;Claude 按 description 自动选用,也可以 /名称 手动调用(user-invocable: false 时不行)",
    }
}

/// 新建表单。
fn form(wb: &Workbench, kind: ItemKind, tab: CustomizeTab, cx: &mut Context<Workbench>) -> Div {
    let t = cx.theme().clone();
    let f = &wb.forms().item;
    let has_project = wb.claude_cfg_root.is_some();
    let options: Vec<(Scope, &'static str)> = [Scope::User, Scope::Project].into_iter().filter(|s| has_project || *s == Scope::User).map(|s| (s, s.label())).collect();
    let scope = if has_project { f.scope } else { Scope::User };
    v_flex().w_full().mt_3().p_3().gap_2().rounded(px(10.)).border_1().border_color(t.primary.opacity(0.5))
        .child(h_flex().gap_3().items_center()
            .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(format!("新建{}", kind.label())))
            .child(segmented("item-scope", options, scope, cx, |this, v, cx| { this.forms_mut().item.scope = v; cx.notify(); }))
            .when(!has_project, |d| d.child(caption("打开项目会话后可建在项目层", cx))))
        .child(Input::new(&f.name).small())
        .child(Input::new(&f.description).small())
        .child(h_flex().w_full().gap_2().justify_end()
            .child(Button::new("item-cancel").small().ghost().label("取消").on_click(cx.listener(|this, _, _, cx| this.toggle_item_form(cx))))
            .child(Button::new("item-create").small().primary().label("创建并编辑")
                .on_click(cx.listener(move |this, _, window, cx| this.create_item_from_form(kind, tab, window, cx)))))
}

/// 一行条目:点击在下方编辑。
fn item_row(i: usize, item: Item, tab: CustomizeTab, current: bool, cx: &mut Context<Workbench>) -> Stateful<Div> {
    let t = cx.theme().clone();
    let (path, del) = (item.path.clone(), item.clone());
    let name = if item.kind == ItemKind::Agent { item.name.clone() } else { format!("/{}", item.name) };
    h_flex().id(SharedString::from(format!("item-{i}"))).w_full().min_h(px(40.)).px_2().py_1().gap_2().items_center().rounded(px(6.)).cursor_pointer()
        .when(current, |d| d.bg(t.list_active)).hover(|d| d.bg(t.list_hover))
        .on_click(cx.listener(move |this, _, window, cx| this.open_doc(path.clone(), tab, window, cx)))
        .child(v_flex().flex_1().min_w_0()
            .child(h_flex().gap_2().items_center()
                .child(div().font_family(MONO).text_size(px(12.5)).font_weight(FontWeight::MEDIUM).child(name))
                .child(scope_pill(item.scope, cx))
                .when(!item.user_invocable, |d| d.child(caption("仅自动调用", cx))))
            .child(div().truncate().text_size(px(12.)).text_color(t.muted_foreground).child(item.description.clone())))
        .child(Button::new(SharedString::from(format!("item-del-{i}"))).xsmall().ghost().icon(IconName::Trash)
            .on_click(cx.listener(move |this, _, _, cx| { cx.stop_propagation(); this.trash_item(&del, cx) })))
}

/// 渲染管理页。
///
/// 参数:`kind` 种类;`tab` 对应的自定义标签;`filter` 搜索词(小写,空为不过滤)。
pub fn render(wb: &Workbench, kind: ItemKind, tab: CustomizeTab, filter: &str, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let list: Vec<Item> = wb.forms().items.get(&kind).cloned().unwrap_or_default().into_iter().filter(|i| filter.is_empty() || format!("{} {}", i.name, i.description).to_lowercase().contains(filter)).collect();
    let current = wb.forms().doc.as_ref().filter(|d| d.tab == tab).map(|d| d.path.clone());
    let open = wb.forms().item.open;
    let mut body = v_flex().w_full();
    for scope in [Scope::User, Scope::Project] {
        let group: Vec<(usize, Item)> = list.iter().cloned().enumerate().filter(|(_, it)| it.scope == scope).collect();
        if group.is_empty() {
            continue;
        }
        body = body.child(section_title(&format!("{}层({})", scope.label(), group.len()), cx));
        for (i, it) in group {
            let on = current.as_ref() == Some(&it.path);
            body = body.child(item_row(i, it, tab, on, cx));
        }
    }
    v_flex().w_full()
        .child(h_flex().w_full().items_center()
            .child(div().flex_1().child(page_title(kind.label())))
            .child(Button::new("item-new").small().primary().icon(IconName::Plus).label("新建").on_click(cx.listener(|this, _, _, cx| this.toggle_item_form(cx)))))
        .child(caption(intro(kind), cx))
        .children(error_banner(wb, cx))
        .when(open, |d| d.child(form(wb, kind, tab, cx)))
        .when(list.is_empty(), |d| d.child(div().pt_4().child(caption(format!("还没有{}", kind.label()), cx))))
        .child(body)
        .when(current.is_none() && !list.is_empty(), |d| d.child(h_flex().pt_3().gap_1().items_center()
            .child(icon(IconName::Info, 12., t.muted_foreground)).child(caption("点击条目在下方编辑", cx))))
        .children(doc_editor(wb, tab, true, cx))
        .into_any_element()
}
