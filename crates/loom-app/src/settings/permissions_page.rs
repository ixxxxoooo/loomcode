//! 权限页:当前配置层的默认模式 / 禁止绕过 / 沙盒 / 额外目录,以及合并后的 allow / ask / deny 规则。
//!
//! @author ixxxxoooo

use super::claude_pages::scope_pill;
use super::widgets::{block_row, error_banner, group, item, note, page_header, row, rows, scope_header, segmented};
use crate::ui::{MONO, caption, icon};
use crate::workbench::Workbench;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::Button;
use gpui_kit::component::input::Input;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::*;
use loom_config::Rule;
use loom_core::PermissionMode;

/// 一组规则(allow / deny / ask),一个分组卡片。
fn rule_group(kind: &'static str, title: &str, rules: &[Rule], color: Hsla, cx: &mut Context<Workbench>) -> Div {
    let t = cx.theme().clone();
    let list: Vec<AnyElement> = if rules.is_empty() {
        rows![item(cx).h(px(40.)).child(caption("暂无规则", cx))]
    } else {
        rules.iter().enumerate().map(|(i, r)| {
            let rule = r.clone();
            item(cx)
                .child(div().size(px(6.)).flex_none().rounded_full().bg(color))
                .child(div().flex_1().min_w_0().truncate().font_family(MONO).text_size(px(12.5)).child(r.text.clone()))
                .child(scope_pill(r.scope, cx))
                .child(div().id(SharedString::from(format!("rule-del-{kind}-{i}"))).p_1().rounded(px(4.)).cursor_pointer().hover(|d| d.bg(t.list_hover))
                    .on_click(cx.listener(move |this, _, _, cx| this.remove_rule(kind, &rule, cx)))
                    .child(icon(IconName::X, 12., t.muted_foreground)))
                .into_any_element()
        }).collect()
    };
    group(&format!("{title}({})", rules.len()), list, cx)
}

/// 当前层的模式、沙盒、额外目录。
fn layer_section(wb: &Workbench, cx: &mut Context<Workbench>) -> Div {
    let t = cx.theme().clone();
    let scope = wb.effective_scope();
    let layer = wb.claude_cfg.layer(scope);
    // 「未设置」用空串表示:删除该键,继承其它层
    let mut modes: Vec<(&'static str, &'static str)> = vec![("", "未设置")];
    modes.extend(PermissionMode::ALL.iter().filter(|m| **m != PermissionMode::Auto).map(|m| (m.cli_value(), m.label())));
    let cur = PermissionMode::ALL.iter().map(|m| m.cli_value()).find(|v| Some(*v) == layer.default_mode.as_deref()).unwrap_or("");
    let dirs = layer.additional_dirs.clone();
    let dir_list = v_flex().w_full().gap_1()
        .children(dirs.into_iter().enumerate().map(|(i, d)| {
            let dir = d.clone();
            h_flex().w_full().h(px(28.)).px_2().gap_2().items_center().rounded(px(6.)).bg(t.muted)
                .child(icon(IconName::Folder, 12., t.muted_foreground))
                .child(div().flex_1().min_w_0().truncate().font_family(MONO).text_size(px(12.)).child(d))
                .child(div().id(SharedString::from(format!("dir-del-{i}"))).p_1().rounded(px(4.)).cursor_pointer().hover(|d| d.bg(t.list_hover))
                    .on_click(cx.listener(move |this, _, _, cx| this.remove_additional_dir(&dir, cx)))
                    .child(icon(IconName::X, 12., t.muted_foreground)))
        }))
        .child(h_flex().w_full().gap_2()
            .child(div().flex_1().child(Input::new(&wb.forms().dir).small()))
            .child(Button::new("dir-add").small().outline().label("添加")
                .on_click(cx.listener(|this, _, window, cx| this.add_additional_dir(window, cx)))));
    group(&format!("{}层设置", scope.label()), rows![
        row("默认权限模式", "写入 permissions.defaultMode;LoomCode 新会话的默认模式在「对话」页", segmented("perm-mode", modes, cur, cx,
            |this, v, cx| this.set_default_mode((!v.is_empty()).then_some(v), cx)), cx),
        row("禁止绕过权限模式", "permissions.disableBypassPermissionsMode = \"disable\"", Switch::new("sw-nobypass").checked(layer.disable_bypass)
            .on_click(cx.listener(|this, c: &bool, _, cx| this.set_disable_bypass(*c, cx))), cx),
        row("Bash 沙盒", "sandbox.enabled:在受限环境里执行 Bash 命令", Switch::new("sw-sandbox").checked(layer.sandbox.unwrap_or(false))
            .on_click(cx.listener(|this, c: &bool, _, cx| this.set_sandbox(*c, cx))), cx),
        block_row("额外可访问目录", "permissions.additionalDirectories:工作目录之外允许 Claude 读写的目录", dir_list, cx),
    ], cx)
}

/// 权限页。
pub fn permissions(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let p = wb.claude_cfg.permissions.clone();
    let t = cx.theme().clone();
    let target = match wb.scope_settings_file(wb.effective_scope()) {
        Some(path) => format!("写入 {}", path.display()),
        None => "先打开一个项目会话".to_string(),
    };
    v_flex().w_full()
        .child(page_header("权限", "规则合并显示三层并标出来源,删除会写回规则所在的文件", div(), cx))
        .child(scope_header(wb, target, cx))
        .children(error_banner(wb, cx))
        .child(layer_section(wb, cx))
        .child(group("添加规则", rows![
            block_row("规则", "示例:Bash(git status)、Bash(npm run:*)、Read(src/**);新增规则写入所选配置层", h_flex().w_full().gap_2().items_center()
                .child(div().flex_1().child(Input::new(&wb.forms().rule).small()))
                .children([("allow", "允许"), ("ask", "询问"), ("deny", "拒绝")].into_iter().map(|(kind, label)| {
                    Button::new(SharedString::from(format!("add-{kind}"))).small().outline().label(label)
                        .on_click(cx.listener(move |this, _, window, cx| this.add_rule(kind, window, cx)))
                })), cx),
        ], cx))
        .child(rule_group("allow", "始终允许", &p.allow, t.success, cx))
        .child(rule_group("ask", "每次询问", &p.ask, t.warning, cx))
        .child(rule_group("deny", "始终拒绝", &p.deny, t.danger, cx))
        .child(note("规则按「拒绝 → 询问 → 允许」的顺序匹配", cx))
        .into_any_element()
}
