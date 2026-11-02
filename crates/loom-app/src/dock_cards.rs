//! 输入框上方的「待你处理」卡片(对标 OhMyAgent):权限请求、提问向导、计划待批准。
//! 这些卡片不进时间线;处理完后时间线上只留一行结果(见 `ask_cards`)。
//!
//! @author ixxxxoooo

use crate::model::SessionModel;
use crate::tool_card::tool_icon;
use crate::tool_content::render_content;
use crate::ui::{MONO, caption, icon, mono_box};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::*;

/// 待处理的请求(从时间线里挑出还没有结果的)。
pub enum Pending<'a> {
    Permission(&'a PermissionRequest),
    Question(&'a QuestionRequest),
    Plan(&'a PlanApproval),
}

/// 找出所有等待用户处理的请求:权限全部列出,提问只呈现最早的一个(其余计入「还有 N 个」),计划取最后一个。
///
/// 参数:`items` 时间线条目。返回:按时间顺序的待处理请求。
pub fn pending_of(items: &[TimelineItem]) -> Vec<Pending<'_>> {
    let mut out = Vec::new();
    let mut asked = false;
    for it in items {
        match it {
            TimelineItem::Permission(p) if p.resolved.is_none() => out.push(Pending::Permission(p)),
            TimelineItem::Question(q) if q.answers.is_none() && !asked => {
                asked = true;
                out.push(Pending::Question(q));
            }
            TimelineItem::PlanApproval(p) if p.approved.is_none() => out.push(Pending::Plan(p)),
            _ => {}
        }
    }
    out
}

/// 某个条目是否是还在等待处理的权限 / 提问(它们只在输入框上方显示,时间线里隐藏)。
pub fn hidden_in_timeline(item: &TimelineItem) -> bool {
    matches!(item, TimelineItem::Permission(p) if p.resolved.is_none()) || matches!(item, TimelineItem::Question(q) if q.answers.is_none())
}

/// 输入框上方的卡片区;没有待处理请求时返回 `None`。
///
/// 参数:`session` 会话模型。返回:居中、与输入框同宽的卡片栈。
pub fn render(session: &Entity<SessionModel>, cx: &App) -> Option<AnyElement> {
    let m = session.read(cx);
    let list = pending_of(&m.state.items);
    if list.is_empty() {
        return None;
    }
    let queued = m.state.items.iter().filter(|i| matches!(i, TimelineItem::Question(q) if q.answers.is_none())).count().saturating_sub(1);
    let cards: Vec<AnyElement> = list
        .iter()
        .map(|p| match p {
            Pending::Permission(r) => permission(session, r, cx),
            Pending::Question(q) => crate::wizard_card::render(session, q, queued, cx),
            Pending::Plan(p) => plan_bar(session, p, cx),
        })
        .collect();
    Some(
        div().w_full().flex().justify_center().px_4().pt_1().pb_1()
            .child(v_flex().w_full().max_w(px(crate::timeline::content_max_w(cx))).gap_2().children(cards))
            .into_any_element(),
    )
}

/// 权限卡:标题 + 命令 / Diff 预览 + 选项按钮(单次允许是唯一实心主按钮)。
fn permission(session: &Entity<SessionModel>, req: &PermissionRequest, cx: &App) -> AnyElement {
    let t = cx.theme();
    let cmd = req.tool.input.get("command").and_then(|v| v.as_str()).map(str::to_string).filter(|c| *c != req.tool.title);
    let preview: Vec<AnyElement> = req.tool.content.iter().map(|c| render_content(c, None, "text", None, cx)).collect();
    let danger = crate::perm_keys::is_dangerous(&format!("{} {}", req.tool.title, cmd.as_deref().unwrap_or("")));
    let focus = session.read(cx).perm_focus.clone().filter(|(id, _)| *id == req.id).map(|(_, f)| f);
    let key_session = session.clone();
    v_flex().w_full().p_3().gap_2().rounded(px(12.)).border_1().border_color(if danger { t.danger.opacity(0.7) } else { t.border }).bg(t.background).shadow_sm()
        .key_context("PermissionCard")
        .when_some(focus, |d, f| d.track_focus(&f))
        // 键盘:⏎ 允许一次、数字键选对应选项、Esc 拒绝
        .on_key_down(move |ev, _, cx| {
            let m = ev.keystroke.modifiers;
            if crate::perm_keys::on_key(&key_session, &ev.keystroke.key, !m.control && !m.platform && !m.alt, cx) {
                cx.stop_propagation();
            }
        })
        .child(
            h_flex().gap_2().items_center()
                .child(icon(if danger { IconName::TriangleAlert } else { tool_icon(req.tool.kind) }, 14., if danger { t.danger } else { t.warning }))
                .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(if danger { "需要你的许可 · 高风险操作" } else { "需要你的许可" })),
        )
        .child(div().text_size(px(12.)).font_family(MONO).max_h(px(72.)).overflow_hidden().child(req.tool.title.clone()))
        .when_some(cmd, |d, c| d.child(mono_box(cx).max_h(px(120.)).overflow_hidden().child(crate::code_hl::shell_text(&c, "$ ", cx))))
        .when(!preview.is_empty(), |d| d.child(v_flex().id(SharedString::from(format!("perm-prev-{}", req.id))).w_full().max_h(px(260.)).overflow_y_scroll().gap_2().children(preview)))
        .child(h_flex().flex_wrap().gap_2().children(req.options.iter().enumerate().map(|(i, opt)| {
            let (s, rid, oid) = (session.clone(), req.id.clone(), opt.id.clone());
            // 按钮后缀快捷键提示:允许一次 ⏎、拒绝 ⎋,其余用数字键
            let hint = match opt.kind { PermissionOptionKind::AllowOnce => "⏎".to_string(), PermissionOptionKind::RejectOnce => "⎋".to_string(), _ => (i + 1).to_string() };
            let b = Button::new(SharedString::from(format!("{}-{}", req.id, opt.id))).small().label(format!("{} {hint}", opt.label));
            let b = if opt.kind == PermissionOptionKind::AllowOnce { b.primary() } else { b };
            b.on_click(move |_, _, cx| {
                s.update(cx, |m, cx| m.resolve_permission(&rid, &oid, cx));
            })
        })))
        .into_any_element()
}

/// 计划批准后的几种 Build 方式:(按钮文案, 之后的权限模式, 悬停说明)。
const BUILDS: [(&str, PermissionMode, &str); 3] = [
    ("Build · 逐项确认", PermissionMode::Default, "按计划实施,每次修改文件或运行命令前都问你"),
    ("Build · 自动模式", PermissionMode::Auto, "按计划实施,自动模式下由分类器判断哪些操作无需确认"),
    ("Build · 跳过权限确认", PermissionMode::BypassPermissions, "按计划实施,并跳过所有权限确认——只在完全信任这个计划时使用"),
];

/// 计划待批准条:计划正文在时间线的计划卡里,这里只保留决定。
fn plan_bar(session: &Entity<SessionModel>, plan: &PlanApproval, cx: &App) -> AnyElement {
    let t = cx.theme();
    let name = plan.markdown.lines().map(|l| l.trim_start_matches('#').trim()).find(|l| !l.is_empty()).unwrap_or("计划").to_string();
    v_flex().w_full().p_3().gap_2().rounded(px(12.)).border_1().border_color(t.border).bg(t.background).shadow_sm()
        .child(
            h_flex().gap_2().items_center()
                .child(icon(IconName::ListTodo, 14., t.warning))
                .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child("计划待批准"))
                .child(div().flex_1().min_w_0().truncate().text_size(px(12.)).text_color(t.muted_foreground).child(name)),
        )
        .child(caption("上面的计划卡里可以查看完整计划;确认没问题再选择 Build。", cx))
        .child(
            h_flex().flex_wrap().gap_2()
                .children(BUILDS.iter().enumerate().map(|(i, (label, mode, hint))| {
                    let (s, id, mode) = (session.clone(), plan.id.clone(), *mode);
                    // 逐项确认(最稳妥的 Build)是唯一的实心主按钮
                    let b = Button::new(SharedString::from(format!("{}-build{i}", plan.id))).small().label(*label).tooltip(*hint);
                    let b = if i == 0 { b.primary() } else { b };
                    b.on_click(move |_, _, cx| { s.update(cx, |m, cx| m.resolve_plan(&id, true, Some(mode), cx)); })
                }))
                .child({
                    let (s, id) = (session.clone(), plan.id.clone());
                    Button::new(SharedString::from(format!("{}-no", plan.id))).small().label("继续规划").tooltip("不批准,让 Claude 根据反馈继续完善计划")
                        .on_click(move |_, _, cx| { s.update(cx, |m, cx| m.resolve_plan(&id, false, None, cx)); })
                }),
        )
        .into_any_element()
}

/// 字母徽标(A、B、C…),选中时反色。
pub fn letter_badge(index: usize, selected: bool, cx: &App) -> Div {
    let t = cx.theme();
    let ch = char::from_u32(65 + index as u32).filter(|_| index < 26).unwrap_or('·');
    div().size(px(18.)).flex().items_center().justify_center().rounded(px(5.)).text_size(px(10.5)).font_weight(FontWeight::MEDIUM)
        .bg(if selected { t.primary } else { t.muted })
        .text_color(if selected { t.primary_foreground } else { t.muted_foreground })
        .child(ch.to_string())
}
