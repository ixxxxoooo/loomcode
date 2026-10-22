//! 时间线上的各类卡片:待办、子 Agent、权限、提问向导、计划、画布、改动汇总、提示。

use crate::diff;
use crate::model::{SessionModel, UiRequest};
use crate::timeline;
use crate::tool_card::{render_content, tool_icon};
use crate::ui::{MONO, card, caption, icon, pill};
use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ucode_core::*;

/// 待办清单卡。
pub fn render_todos(id: &str, items: &[TodoItem], cx: &App) -> AnyElement {
    let t = cx.theme();
    let done = items.iter().filter(|i| i.status == TodoStatus::Completed).count();
    card(cx)
        .p_3()
        .gap_1p5()
        .child(
            h_flex().gap_2().items_center()
                .child(icon(IconName::ListTodo, 14., t.muted_foreground))
                .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child("待办"))
                .child(caption(format!("{done}/{}", items.len()), cx)),
        )
        .children(items.iter().enumerate().map(|(i, it)| {
            let (ic, color) = match it.status {
                TodoStatus::Completed => (IconName::CircleCheck, t.success),
                TodoStatus::InProgress => (IconName::CircleDot, t.primary),
                TodoStatus::Pending => (IconName::Circle, t.muted_foreground),
            };
            h_flex()
                .id(SharedString::from(format!("{id}-todo-{i}")))
                .gap_2()
                .items_center()
                .child(icon(ic, 14., color))
                .child(
                    div().text_size(px(13.))
                        .text_color(if it.status == TodoStatus::Completed { t.muted_foreground } else { t.foreground })
                        .when(it.status == TodoStatus::Completed, |d| d.line_through())
                        .child(it.content.clone()),
                )
        }))
        .into_any_element()
}

fn subagent_status_pill(status: SubagentStatus, cx: &App) -> AnyElement {
    let t = cx.theme();
    let (text, fg) = match status {
        SubagentStatus::Starting => ("启动中", t.muted_foreground),
        SubagentStatus::Running => ("运行中", t.primary),
        SubagentStatus::Background => ("后台运行", t.info),
        SubagentStatus::Completed => ("已完成", t.success),
        SubagentStatus::Failed => ("已失败", t.danger),
    };
    let spinning = matches!(status, SubagentStatus::Starting | SubagentStatus::Running);
    h_flex()
        .gap_1p5()
        .items_center()
        .when(spinning, |d| d.child(Spinner::new().xsmall()))
        .child(pill(text, fg, t.muted))
        .into_any_element()
}

/// 子 Agent 卡片(内部嵌套时间线)。
pub fn render_subagent(session: &Entity<SessionModel>, card_data: &SubagentCard, depth: usize, window: &mut Window, cx: &mut App) -> AnyElement {
    let t = cx.theme().clone();
    let expanded = session.read(cx).is_expanded(&card_data.id);
    let id = card_data.id.clone();
    let s = session.clone();
    let last = card_data.children.last().map(|c| match c {
        TimelineItem::Tool(t) => t.title.clone(),
        TimelineItem::Assistant { text, .. } => text.lines().next().unwrap_or("").to_string(),
        _ => String::new(),
    });
    let header = h_flex()
        .id(SharedString::from(format!("sub-{}", card_data.id)))
        .w_full()
        .px_3()
        .py_2()
        .gap_2()
        .items_center()
        .cursor_pointer()
        .hover(|d| d.bg(t.list_hover))
        .on_click(move |_, _, cx| {
            s.update(cx, |m, cx| m.toggle(&id, cx));
        })
        .child(icon(IconName::Bot, 15., t.primary))
        .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(card_data.agent_type.clone()))
        .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).text_color(t.muted_foreground).child(card_data.description.clone()))
        .child(subagent_status_pill(card_data.status, cx))
        .child(icon(if expanded { IconName::ChevronDown } else { IconName::ChevronRight }, 12., t.muted_foreground));
    let mut body = v_flex().w_full().px_3().pb_2().gap_1();
    if expanded {
        for child in &card_data.children {
            body = body.child(timeline::render_item(session, child, depth + 1, window, cx));
        }
        if card_data.children.is_empty() {
            body = body.child(caption("暂无活动", cx));
        }
    } else if let Some(last) = last.filter(|l| !l.is_empty()) {
        body = body.child(div().text_size(px(12.)).text_color(t.muted_foreground).truncate().child(format!("最新:{last}")));
    }
    card(cx).overflow_hidden().child(header).child(body).into_any_element()
}

/// 权限请求卡。
pub fn render_permission(session: &Entity<SessionModel>, req: &PermissionRequest, cx: &App) -> AnyElement {
    let t = cx.theme();
    let preview: Vec<AnyElement> = req.tool.content.iter().map(|c| render_content(c, cx)).collect();
    let cmd = req.tool.input.get("command").and_then(|v| v.as_str()).map(str::to_string);
    let waiting = req.resolved.is_none();
    card(cx)
        .border_color(if waiting { t.warning } else { t.border })
        .p_3()
        .gap_2()
        .child(
            h_flex().gap_2().items_center()
                .child(icon(IconName::ShieldQuestionMark, 15., t.warning))
                .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(if waiting { "需要你的许可" } else { "权限已处理" }))
                .child(icon(tool_icon(req.tool.kind), 13., t.muted_foreground))
                .child(div().flex_1().truncate().text_size(px(13.)).text_color(t.muted_foreground).child(req.tool.title.clone())),
        )
        .when_some(cmd, |d, c| d.child(crate::ui::mono_box(cx).child(format!("$ {c}"))))
        .children(preview)
        .child(if waiting {
            let id = req.id.clone();
            h_flex().gap_2().children(req.options.iter().map(move |opt| {
                let s = session.clone();
                let (rid, oid) = (id.clone(), opt.id.clone());
                let mut b = Button::new(SharedString::from(format!("{}-{}", req.id, opt.id))).small().label(opt.label.clone());
                b = match opt.kind {
                    PermissionOptionKind::AllowOnce => b.primary(),
                    PermissionOptionKind::AllowAlways => b,
                    _ => b.ghost(),
                };
                b.on_click(move |_, _, cx| {
                    s.update(cx, |m, cx| m.resolve_permission(&rid, &oid, cx));
                })
            }))
            .into_any_element()
        } else {
            let chosen = req.options.iter().find(|o| Some(&o.id) == req.resolved.as_ref()).map(|o| o.label.clone()).unwrap_or_default();
            let allowed = req.options.iter().find(|o| Some(&o.id) == req.resolved.as_ref()).map(|o| matches!(o.kind, PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways)).unwrap_or(false);
            pill(chosen, if allowed { t.success } else { t.danger }, t.muted).into_any_element()
        })
        .into_any_element()
}

/// 提问向导卡。
pub fn render_question(session: &Entity<SessionModel>, req: &QuestionRequest, cx: &App) -> AnyElement {
    let t = cx.theme();
    let n = req.questions.len();
    if let Some(answers) = &req.answers {
        return card(cx).p_3().gap_1p5()
            .child(h_flex().gap_2().items_center()
                .child(icon(IconName::CircleQuestionMark, 15., t.primary))
                .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child("已回答")))
            .children(req.questions.iter().zip(answers).map(|(q, a)| {
                h_flex().gap_2().text_size(px(13.)).child(div().text_color(t.muted_foreground).child(format!("{}:", q.header)))
                    .child(div().child(if a.is_empty() { "(未选择)".to_string() } else { a.join("、") }))
            }))
            .into_any_element();
    }
    let step = session.read(cx).wizard_step.get(&req.id).copied().unwrap_or(0).min(n.saturating_sub(1));
    let q = &req.questions[step];
    let draft = session.read(cx).wizard_draft.get(&req.id).cloned().unwrap_or_default();
    let picked: Vec<String> = draft.get(step).cloned().unwrap_or_default();
    let last = step + 1 == n;
    card(cx).border_color(t.primary).p_3().gap_2()
        .child(
            h_flex().gap_2().items_center()
                .child(icon(IconName::CircleQuestionMark, 15., t.primary))
                .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child("需要你做个选择"))
                .child(div().flex_1())
                .when(n > 1, |d| d.child(caption(format!("{}/{n}", step + 1), cx))),
        )
        .child(h_flex().child(pill(q.header.clone(), t.primary, t.muted)))
        .child(div().text_size(px(14.)).child(q.question.clone()))
        .child(v_flex().gap_1().children(q.options.iter().enumerate().map(|(oi, opt)| {
            let on = picked.contains(&opt.label);
            let s = session.clone();
            let (rid, label, multi) = (req.id.clone(), opt.label.clone(), q.multi_select);
            h_flex()
                .id(SharedString::from(format!("{}-q{step}-o{oi}", req.id)))
                .w_full().gap_2().px_2().py_1p5().rounded(px(8.)).border_1().cursor_pointer()
                .border_color(if on { t.primary } else { t.border })
                .when(on, |d| d.bg(t.accent))
                .hover(|d| d.bg(t.list_hover))
                .on_click(move |_, _, cx| {
                    s.update(cx, |m, cx| m.wizard_pick(&rid, step, n, &label, multi, cx));
                })
                .child(icon(
                    match (q.multi_select, on) {
                        (true, true) => IconName::SquareCheck,
                        (true, false) => IconName::Square,
                        (false, true) => IconName::CircleDot,
                        (false, false) => IconName::Circle,
                    },
                    14.,
                    if on { t.primary } else { t.muted_foreground },
                ))
                .child(v_flex().child(div().text_size(px(13.)).child(opt.label.clone())).child(caption(opt.description.clone(), cx)))
        })))
        .child(
            h_flex().gap_2().justify_end()
                .when(step > 0, |d| {
                    let (s, id) = (session.clone(), req.id.clone());
                    d.child(Button::new(SharedString::from(format!("{}-prev", req.id))).small().ghost().label("上一题")
                        .on_click(move |_, _, cx| { s.update(cx, |m, cx| m.wizard_go(&id, step - 1, cx)); }))
                })
                .child({
                    let (s, id) = (session.clone(), req.id.clone());
                    Button::new(SharedString::from(format!("{}-next", req.id))).small().primary()
                        .label(if last { "提交" } else { "下一题" })
                        .disabled(picked.is_empty())
                        .on_click(move |_, _, cx| {
                            s.update(cx, |m, cx| if last { m.wizard_submit(&id, n, cx) } else { m.wizard_go(&id, step + 1, cx) });
                        })
                }),
        )
        .into_any_element()
}

/// 计划批准卡。
pub fn render_plan(session: &Entity<SessionModel>, plan: &PlanApproval, cx: &App) -> AnyElement {
    let t = cx.theme();
    card(cx).border_color(if plan.approved.is_none() { t.primary } else { t.border }).p_3().gap_2()
        .child(
            h_flex().gap_2().items_center()
                .child(icon(IconName::Map, 15., t.primary))
                .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child("计划"))
                .child(div().flex_1())
                .when_some(plan.approved, |d, ok| d.child(pill(if ok { "已批准" } else { "已拒绝" }, if ok { t.success } else { t.danger }, t.muted))),
        )
        .child(TextView::markdown(SharedString::from(format!("plan-{}", plan.id)), plan.markdown.clone()).selectable(true))
        .when(plan.approved.is_none(), |d| {
            let (s1, id1) = (session.clone(), plan.id.clone());
            let (s2, id2) = (session.clone(), plan.id.clone());
            d.child(
                h_flex().gap_2()
                    .child(Button::new(SharedString::from(format!("{}-ok", plan.id))).small().primary().label("批准并执行")
                        .on_click(move |_, _, cx| { s1.update(cx, |m, cx| m.resolve_plan(&id1, true, cx)); }))
                    .child(Button::new(SharedString::from(format!("{}-no", plan.id))).small().ghost().label("继续规划")
                        .on_click(move |_, _, cx| { s2.update(cx, |m, cx| m.resolve_plan(&id2, false, cx)); })),
            )
        })
        .into_any_element()
}

/// Canvas 卡片。
pub fn render_canvas(session: &Entity<SessionModel>, id: &str, title: &str, cx: &App) -> AnyElement {
    let t = cx.theme();
    let (s, cid) = (session.clone(), id.to_string());
    card(cx).id(SharedString::from(format!("canvas-{id}"))).p_3().cursor_pointer().hover(|d| d.bg(t.list_hover))
        .on_click(move |_, _, cx| { s.update(cx, |_, cx| cx.emit(UiRequest::OpenCanvas(cid.clone()))); })
        .child(h_flex().gap_2().items_center()
            .child(icon(IconName::LayoutDashboard, 16., t.primary))
            .child(v_flex().child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(title.to_string())).child(caption("画布 · 点击打开", cx))))
        .into_any_element()
}

/// 本轮改动汇总。
pub fn render_changes(session: &Entity<SessionModel>, files: &[ChangedFile], cx: &App) -> AnyElement {
    let t = cx.theme();
    let (a, r): (u32, u32) = files.iter().fold((0, 0), |(a, r), f| (a + f.added, r + f.removed));
    let s = session.clone();
    card(cx).p_3().gap_1p5()
        .child(h_flex().gap_2().items_center()
            .child(icon(IconName::FileDiff, 15., t.muted_foreground))
            .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(format!("本轮改动 · {} 个文件", files.len())))
            .child(div().text_size(px(12.)).font_family(MONO).text_color(t.success).child(format!("+{a}")))
            .child(div().text_size(px(12.)).font_family(MONO).text_color(t.danger).child(format!("−{r}")))
            .child(div().flex_1())
            .child(Button::new("changes-open").small().ghost().label("查看改动")
                .on_click(move |_, _, cx| { s.update(cx, |_, cx| cx.emit(UiRequest::OpenChanges)); })))
        .children(files.iter().map(|f| {
            h_flex().gap_2().text_size(px(12.)).font_family(MONO)
                .child(div().flex_1().truncate().child(f.path.clone()))
                .child(div().text_color(t.success).child(format!("+{}", f.added)))
                .child(div().text_color(t.danger).child(format!("−{}", f.removed)))
        }))
        .into_any_element()
}

/// 提示行。
pub fn render_notice(level: NoticeLevel, text: &str, cx: &App) -> AnyElement {
    let t = cx.theme();
    let (ic, color) = match level {
        NoticeLevel::Info => (IconName::Info, t.info),
        NoticeLevel::Warning => (IconName::TriangleAlert, t.warning),
        NoticeLevel::Error => (IconName::CircleX, t.danger),
    };
    h_flex().w_full().gap_2().px_3().py_1p5().rounded(px(8.)).bg(t.muted).items_start()
        .child(div().mt(px(2.)).child(icon(ic, 14., color)))
        .child(div().flex_1().text_size(px(13.)).text_color(color).child(text.to_string()))
        .into_any_element()
}

#[allow(dead_code)]
fn _unused(_: &diff::DiffLine) {}
