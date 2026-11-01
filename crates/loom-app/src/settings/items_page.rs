//! 技能 / 命令 / 子 Agent 管理页(三者共用):页头、按层分组的卡片列表、新建、点条目进入编辑、行菜单。
//!
//! @author ixxxxoooo

use super::CustomizeTab;
use super::claude_pages::doc_editor;
use super::confirm::{Confirm, confirm_banner};
use super::ext_ui::{Row, ext_card, ext_empty, ext_header, ext_label, icon_button, tag};
use super::item_actions::{chat_snippet, reveal};
use super::widgets::{error_banner, segmented};
use crate::ui::caption;
use crate::workbench::Workbench;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_config::Scope;
use loom_config::items::{Item, ItemKind};

/// 页面说明。
fn intro(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Command => "把常用提示词保存成斜杠命令,在输入框用 /命令名 触发,保存在 ~/.claude/commands 或项目的 .claude/commands。",
        ItemKind::Agent => "给 Claude 配置可委派任务的专用子 Agent,定义保存在 ~/.claude/agents 或项目的 .claude/agents。",
        ItemKind::Skill => "用 SKILL.md 扩展 Claude 的能力,保存在 ~/.claude/skills 或项目的 .claude/skills。",
    }
}

fn kind_icon(kind: ItemKind) -> IconName {
    match kind {
        ItemKind::Command => IconName::SquareTerminal,
        ItemKind::Agent => IconName::Bot,
        ItemKind::Skill => IconName::Sparkles,
    }
}

fn noun(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Command => "命令",
        ItemKind::Agent => "子 Agent",
        ItemKind::Skill => "技能",
    }
}

/// 新建表单。
fn form(wb: &Workbench, kind: ItemKind, tab: CustomizeTab, cx: &mut Context<Workbench>) -> Div {
    let t = cx.theme().clone();
    let f = &wb.forms().item;
    let has_project = wb.claude_cfg_root.is_some();
    let options: Vec<(Scope, &'static str)> = [Scope::User, Scope::Project].into_iter().filter(|s| has_project || *s == Scope::User).map(|s| (s, s.label())).collect();
    let scope = if has_project { f.scope } else { Scope::User };
    v_flex().w_full().p_3().gap_2().rounded(px(10.)).border_1().border_color(t.primary.opacity(0.5)).bg(t.popover)
        .child(h_flex().gap_3().items_center()
            .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(format!("新建{}", noun(kind))))
            .child(segmented("item-scope", options, scope, cx, |this, v, cx| { this.forms_mut().item.scope = v; cx.notify(); }))
            .when(!has_project, |d| d.child(caption("打开项目会话后可建在项目层", cx))))
        .child(Input::new(&f.name).small())
        .child(Input::new(&f.description).small())
        .child(h_flex().w_full().gap_2().justify_end()
            .child(Button::new("item-cancel").small().ghost().label("取消").on_click(cx.listener(|this, _, _, cx| this.toggle_item_form(cx))))
            .child(Button::new("item-create").small().primary().label("创建并编辑")
                .on_click(cx.listener(move |this, _, window, cx| this.create_item_from_form(kind, tab, window, cx)))))
}

/// 行右侧的「更多」菜单:编辑 / 在 Finder 中显示 / 删除。
fn item_menu(wb: WeakEntity<Workbench>, item: Item, tab: CustomizeTab) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu, _, _| {
        let (w1, p1) = (wb.clone(), item.path.clone());
        let edit = PopupMenuItem::new("编辑…").icon(IconName::Pencil).on_click(move |_, window, cx| {
            let (p, w) = (p1.clone(), w1.clone());
            let _ = w.update(cx, |this, cx| this.open_doc(p, tab, window, cx));
        });
        let p2 = item.path.clone();
        let show = PopupMenuItem::new("在 Finder 中显示").icon(IconName::FolderOpen).on_click(move |_, _, _| reveal(&p2));
        let (w3, it) = (wb.clone(), item.clone());
        let del = PopupMenuItem::new("删除…").icon(IconName::Trash).on_click(move |_, _, cx| {
            let it = it.clone();
            let _ = w3.update(cx, |this, cx| this.ask(Confirm::TrashItem(it), cx));
        });
        menu.item(edit).item(show).separator().item(del)
    }
}

/// 一行条目。
fn item_row(i: usize, item: Item, tab: CustomizeTab, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let path = item.path.clone();
    let mut row = Row::new(format!("item-{i}"), kind_icon(item.kind), if item.kind == ItemKind::Agent { item.name.clone() } else { format!("/{}", item.name) });
    row.subtitle = Some(item.description.clone());
    if item.kind == ItemKind::Command {
        row.badges.push(tag("命令", None, cx).into_any_element());
    }
    if !item.user_invocable {
        row.badges.push(tag("仅自动调用", Some(t.warning), cx).into_any_element());
    }
    row.on_click = Some(std::rc::Rc::new(move |this, window, cx| this.open_doc(path.clone(), tab, window, cx)));
    let snippet = chat_snippet(&item);
    let weak = cx.entity().downgrade();
    let usable = item.kind == ItemKind::Agent || item.user_invocable;
    row.trailing = Some(h_flex().gap_1().items_center()
        .when(usable, |d| d.child(icon_button(SharedString::from(format!("item-use-{i}")), IconName::MessageSquare, "回到对话并插入")
            .on_click(cx.listener(move |this, _, window, cx| { cx.stop_propagation(); this.use_in_chat(snippet.clone(), window, cx) }))))
        .child(Button::new(SharedString::from(format!("item-more-{i}"))).small().ghost().icon(IconName::Ellipsis).dropdown_menu(item_menu(weak, item, tab)))
        .into_any_element());
    row.render(cx)
}

/// Claude Code 内置的子 Agent(不需要配置)。
fn builtin_agents(cx: &mut Context<Workbench>) -> Div {
    let list = [("Explore", "快速、只读地检索代码库并回答问题"), ("Plan", "规划模式下调研代码并设计实现方案"), ("general-purpose", "通用 Agent:复杂检索与多步骤任务")];
    let rows: Vec<AnyElement> = list
        .into_iter()
        .map(|(name, desc)| {
            let mut r = Row::new(format!("builtin-{name}"), IconName::Bot, name);
            r.subtitle = Some(desc.to_string());
            r.badges.push(tag("内置", None, cx).into_any_element());
            r.render(cx)
        })
        .collect();
    v_flex().w_full().gap_2().child(ext_label("内置", Some(rows.len()), cx)).child(ext_card(rows, cx))
}

/// 渲染管理页。
///
/// 参数:`kind` 种类;`tab` 对应的自定义标签;`filter` 搜索词(小写,空为不过滤)。
pub fn render(wb: &Workbench, kind: ItemKind, tab: CustomizeTab, filter: &str, cx: &mut Context<Workbench>) -> AnyElement {
    // 正在编辑:整页显示编辑器,带返回按钮
    if let Some(editor) = doc_editor(wb, tab, true, cx) {
        return v_flex().w_full().gap_3()
            .child(ext_header(kind_icon(kind), &format!("编辑{}", noun(kind)), "修改后点「保存」,新会话与输入框补全立即生效", div(), cx))
            .children(error_banner(wb, cx))
            .child(editor)
            .into_any_element();
    }
    let all = wb.forms().items.get(&kind).cloned().unwrap_or_default();
    let total = all.len();
    let list: Vec<Item> = all.into_iter().filter(|i| filter.is_empty() || format!("{} {}", i.name, i.description).to_lowercase().contains(filter)).collect();
    let open = wb.forms().item.open;
    let actions = h_flex().gap_2().items_center()
        .child(icon_button("item-refresh", IconName::RefreshCw, "重新读取").on_click(cx.listener(|this, _, _, cx| { this.refresh_items(); cx.notify(); })))
        .child(Button::new("item-new").small().primary().icon(IconName::Plus).label(format!("新建{}", noun(kind))).on_click(cx.listener(|this, _, _, cx| this.toggle_item_form(cx))));
    let mut body = v_flex().w_full().gap_2();
    for scope in [Scope::User, Scope::Project] {
        let group: Vec<(usize, Item)> = list.iter().cloned().enumerate().filter(|(_, it)| it.scope == scope).collect();
        if group.is_empty() {
            continue;
        }
        body = body.child(ext_label(scope.label(), Some(group.len()), cx));
        let rows = group.into_iter().map(|(i, it)| item_row(i, it, tab, cx)).collect();
        body = body.child(ext_card(rows, cx));
    }
    v_flex().w_full().gap_3()
        .child(ext_header(kind_icon(kind), noun(kind), intro(kind), actions, cx))
        .children(error_banner(wb, cx))
        .children(confirm_banner(wb, cx))
        .when(open, |d| d.child(form(wb, kind, tab, cx)))
        .when(kind == ItemKind::Agent, |d| d.child(builtin_agents(cx)))
        .when(list.is_empty(), |d| d.child(ext_empty(
            kind_icon(kind),
            &if total == 0 { format!("还没有{}", noun(kind)) } else { format!("没有匹配的{}", noun(kind)) },
            &format!("点击右上角「新建{}」创建,或安装带有{}的插件。", noun(kind), noun(kind)),
            cx,
        )))
        .child(body)
        .into_any_element()
}
