//! 与 Claude Code 配置相关的设置页:权限 / MCP / 钩子 / 记忆。
//!
//! @author ixxxxoooo

use super::widgets::{page_title, section_title, segmented};
use crate::ui::{MONO, caption, icon, pill};
use crate::workbench::Workbench;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Editor, Input};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_config::{Rule, Scope};

/// 来源层小标签。
fn scope_pill(scope: Scope, cx: &App) -> impl IntoElement {
    let t = cx.theme();
    pill(scope.label(), t.muted_foreground, t.muted)
}

/// 一组规则(allow / deny / ask)。
fn rule_group(kind: &'static str, title: &str, rules: &[Rule], color: Hsla, cx: &mut Context<Workbench>) -> Div {
    let t = cx.theme().clone();
    v_flex().w_full().gap_1()
        .child(section_title(&format!("{title}({})", rules.len()), cx))
        .when(rules.is_empty(), |d| d.child(caption("暂无规则", cx)))
        .children(rules.iter().enumerate().map(|(i, r)| {
            let rule = r.clone();
            h_flex().w_full().h(px(30.)).px_2().gap_2().items_center().rounded(px(6.)).border_1().border_color(t.border)
                .child(div().size(px(6.)).rounded_full().bg(color))
                .child(div().flex_1().min_w_0().truncate().font_family(MONO).text_size(px(12.5)).child(r.text.clone()))
                .child(scope_pill(r.scope, cx))
                .child(div().id(SharedString::from(format!("rule-del-{kind}-{i}"))).p_1().rounded(px(4.)).cursor_pointer().hover(|d| d.bg(t.list_hover))
                    .on_click(cx.listener(move |this, _, _, cx| this.remove_rule(kind, &rule, cx)))
                    .child(icon(IconName::X, 12., t.muted_foreground)))
        }))
}

/// 权限页:选择写入层后增删 allow / ask / deny 规则,每条规则标出来源层。
pub fn permissions(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let p = wb.claude_cfg.permissions.clone();
    let scope = wb.effective_rule_scope();
    let has_project = wb.demo || wb.claude_cfg_root.is_some();
    let options: Vec<(Scope, &'static str)> = Scope::ALL.into_iter().filter(|s| has_project || !s.needs_project()).map(|s| (s, s.label())).collect();
    let target = if wb.demo {
        "演示数据,仅在内存中修改".to_string()
    } else {
        match wb.rule_scope_file(scope) {
            Some(path) => format!("新规则写入 {}", path.display()),
            None => "先打开一个项目会话".to_string(),
        }
    };
    v_flex().w_full()
        .child(page_title("权限规则"))
        .child(caption("合并显示用户层 ~/.claude/settings.json、项目层 .claude/settings.json 与本地层 .claude/settings.local.json;删除会写回规则所在的文件", cx))
        .child(h_flex().w_full().pt_4().gap_3().items_center()
            .child(div().text_size(px(13.)).child("写入到"))
            .child(segmented("rule-scope", options, scope, cx, |this, v, cx| { this.rule_scope = v; cx.notify(); }))
            .when(!has_project, |d| d.child(caption("打开项目后可写入项目层 / 本地层", cx))))
        .child(caption(target, cx))
        .child(
            h_flex().w_full().gap_2().pt_2().items_center()
                .child(div().flex_1().child(Input::new(&wb.rule_input)))
                .children([("allow", "允许"), ("ask", "询问"), ("deny", "拒绝")].into_iter().map(|(kind, label)| {
                    Button::new(SharedString::from(format!("add-{kind}"))).small().outline().label(label)
                        .on_click(cx.listener(move |this, _, window, cx| this.add_rule(kind, window, cx)))
                })),
        )
        .child(caption("规则示例:Bash(git status)、Bash(npm run:*)、Read(src/**)", cx))
        .child(rule_group("allow", "始终允许", &p.allow, t.success, cx))
        .child(rule_group("ask", "每次询问", &p.ask, t.warning, cx))
        .child(rule_group("deny", "始终拒绝", &p.deny, t.danger, cx))
        .into_any_element()
}

/// MCP 页(只读):同名服务器只显示优先级最高的一条(本地 > 项目 > 用户)。
pub fn mcp(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let servers = wb.claude_cfg.mcp_servers.clone();
    v_flex().w_full()
        .child(page_title("MCP 服务器"))
        .child(caption("用户层 ~/.claude.json、项目层 .mcp.json、本地层 ~/.claude.json 的当前项目条目;同名时 本地 > 项目 > 用户", cx))
        .child(v_flex().w_full().pt_4().gap_2()
            .when(servers.is_empty(), |d| d.child(caption("没有配置 MCP 服务器", cx)))
            .children(servers.into_iter().map(|s| {
                h_flex().w_full().p_3().gap_3().items_center().rounded(px(10.)).border_1().border_color(t.border)
                    .child(icon(IconName::Plug, 16., if s.enabled { t.primary } else { t.muted_foreground }))
                    .child(v_flex().flex_1().min_w_0().gap_0p5()
                        .child(h_flex().gap_2().items_center()
                            .child(div().text_size(px(13.5)).font_weight(FontWeight::MEDIUM).child(s.name.clone()))
                            .child(pill(s.transport.clone(), t.muted_foreground, t.muted))
                            .child(scope_pill(s.scope, cx)))
                        .child(div().truncate().font_family(MONO).text_size(px(12.)).text_color(t.muted_foreground).child(s.target.clone())))
                    .child(pill(if s.enabled { "已启用" } else { "已禁用" }, if s.enabled { t.success } else { t.muted_foreground }, t.muted))
            })))
        .into_any_element()
}

/// 钩子页(只读)。
pub fn hooks(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let hooks = wb.claude_cfg.hooks.clone();
    v_flex().w_full()
        .child(page_title("钩子"))
        .child(caption("在工具调用前后自动运行的命令(只读展示)", cx))
        .child(v_flex().w_full().pt_4().gap_2()
            .when(hooks.is_empty(), |d| d.child(caption("没有配置钩子", cx)))
            .children(hooks.into_iter().map(|h| {
                v_flex().w_full().p_3().gap_1().rounded(px(10.)).border_1().border_color(t.border)
                    .child(h_flex().gap_2().items_center()
                        .child(pill(h.event.clone(), t.primary, t.muted))
                        .when(!h.matcher.is_empty(), |d| d.child(div().font_family(MONO).text_size(px(12.)).text_color(t.muted_foreground).child(format!("匹配 {}", h.matcher))))
                        .child(div().flex_1())
                        .child(scope_pill(h.scope, cx)))
                    .child(div().font_family(MONO).text_size(px(12.5)).child(h.command.clone()))
            })))
        .into_any_element()
}

/// 记忆页:编辑当前项目根目录的 CLAUDE.md。
pub fn memory(wb: &mut Workbench, window: &mut Window, cx: &mut Context<Workbench>) -> AnyElement {
    wb.ensure_memory(window, cx);
    let t = cx.theme().clone();
    let Some(doc) = wb.memory.as_ref() else {
        return v_flex().child(page_title("记忆")).child(caption("先打开一个项目会话", cx)).into_any_element();
    };
    let path = doc.path.display().to_string();
    let saved = doc.saved;
    v_flex().w_full().gap_2()
        .child(page_title("记忆(CLAUDE.md)"))
        .child(h_flex().gap_2().items_center()
            .child(div().flex_1().min_w_0().truncate().font_family(MONO).text_size(px(12.)).text_color(t.muted_foreground).child(path))
            .child(Button::new("memory-save").small().when(!saved, |b| b.primary()).when(saved, |b| b.ghost()).label(if saved { "已保存" } else { "保存" })
                .on_click(cx.listener(|this, _, _, cx| this.save_memory(cx)))))
        .child(div().w_full().h(px(460.)).rounded(px(8.)).border_1().border_color(t.border).overflow_hidden()
            .child(Editor::new(&doc.editor).bordered(false).appearance(false).size_full()))
        .into_any_element()
}
