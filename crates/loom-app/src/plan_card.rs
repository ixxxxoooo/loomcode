//! 计划(ExitPlanMode)的界面:主会话里的紧凑卡片、Build 按钮组,以及标题 / 摘要的纯函数。
//!
//! 完整计划不再塞进时间线:卡片只给标题、状态、几行摘要和决定按钮,点击在右栏「计划」标签里
//! 打开全文与执行进度(对标 Cursor 的计划页、本项目的子 Agent 卡片)。
//!
//! @author ixxxxoooo

use crate::model::{SessionModel, UiRequest};
use crate::ui::{icon, pill};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::*;

/// 计划批准后的几种 Build 方式:(按钮文案, 之后的权限模式, 悬停说明)。
pub const BUILDS: [(&str, PermissionMode, &str); 3] = [
    ("Build · 逐项确认", PermissionMode::Default, "按计划实施,每次修改文件或运行命令前都问你"),
    ("Build · 自动模式", PermissionMode::Auto, "按计划实施,自动模式下由分类器判断哪些操作无需确认"),
    ("Build · 跳过权限确认", PermissionMode::BypassPermissions, "按计划实施,并跳过所有权限确认——只在完全信任这个计划时使用"),
];

/// 计划标题:正文里第一个非空行(去掉 `#` 标记)。
pub fn plan_title(markdown: &str) -> String {
    markdown.lines().map(|l| l.trim_start_matches('#').trim()).find(|l| !l.is_empty()).unwrap_or("计划").to_string()
}

/// 卡片 / 标签头部显示的标题:正文还没生成出来时给一句提示。
pub fn display_title(plan: &PlanApproval) -> String {
    if plan.markdown.trim().is_empty() { "正在生成计划…".into() } else { plan_title(&plan.markdown) }
}

/// 卡片上的摘要:跳过标题、分隔线、代码围栏,取前 `max` 行正文(去列表符号与强调标记)。
pub fn plan_excerpt(markdown: &str, max: usize) -> Vec<String> {
    let mut in_code = false;
    markdown
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.starts_with("```") {
                in_code = !in_code;
                return None;
            }
            if in_code || l.is_empty() || l.starts_with('#') || l.chars().all(|c| matches!(c, '-' | '*' | '_' | '=' | ' ')) {
                return None;
            }
            let l = l.trim_start_matches(|c: char| matches!(c, '-' | '*' | '+' | '>' | ' '));
            let l = l.split_once(". ").filter(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())).map_or(l, |(_, r)| r);
            let l = l.replace("**", "").replace('`', "");
            (!l.trim().is_empty()).then(|| l.trim().to_string())
        })
        .take(max)
        .collect()
}

/// 状态胶囊的文案与颜色。
pub fn status(plan: &PlanApproval, cx: &App) -> (&'static str, Hsla) {
    let t = cx.theme();
    match plan.approved {
        None if plan.streaming => ("生成中…", t.primary),
        None => ("待批准", t.warning),
        Some(true) => ("已批准", t.success),
        Some(false) => ("已拒绝 · 继续规划", t.danger),
    }
}

/// 决定按钮:三种 Build + 「继续规划」。逐项确认(最稳妥)是唯一的实心主按钮。
/// 主会话卡片与右栏「计划」标签头部共用。
pub fn actions(session: &Entity<SessionModel>, plan: &PlanApproval, scope: &str) -> AnyElement {
    h_flex().flex_wrap().gap_2()
        .children(BUILDS.iter().enumerate().map(|(i, (label, mode, hint))| {
            let (s, id, mode) = (session.clone(), plan.id.clone(), *mode);
            let b = Button::new(SharedString::from(format!("{scope}-{}-build{i}", plan.id))).small().label(*label).tooltip(*hint);
            let b = if i == 0 { b.primary() } else { b };
            b.on_click(move |_, _, cx| { cx.stop_propagation(); s.update(cx, |m, cx| m.resolve_plan(&id, true, Some(mode), cx)); })
        }))
        .child({
            let (s, id) = (session.clone(), plan.id.clone());
            Button::new(SharedString::from(format!("{scope}-{}-no", plan.id))).small().label("继续规划").tooltip("不批准,让 Claude 根据反馈继续完善计划")
                .on_click(move |_, _, cx| { cx.stop_propagation(); s.update(cx, |m, cx| m.resolve_plan(&id, false, None, cx)); })
        })
        .into_any_element()
}

/// 主会话里的计划卡:标题 + 状态 + 几行摘要,点击在右栏打开;待批准时带 Build 按钮。
pub fn render_card(session: &Entity<SessionModel>, plan: &PlanApproval, cx: &App) -> AnyElement {
    let t = cx.theme();
    // 生成完才能决定:生成中卡片只展示进展,没有 Build 按钮
    let pending = plan.approved.is_none() && !plan.streaming;
    let (text, fg) = status(plan, cx);
    let excerpt = plan_excerpt(&plan.markdown, 3);
    let (s, id) = (session.clone(), plan.id.clone());
    v_flex().w_full().gap_2().p_3().rounded(px(12.)).border_1().bg(t.background)
        .border_color(if pending { t.warning.opacity(0.6) } else { t.border })
        .child(
            h_flex().id(SharedString::from(format!("plan-card-{}", plan.id))).w_full().gap_2().items_center().cursor_pointer()
                .on_click(move |_, _, cx| { s.update(cx, |_, cx| cx.emit(UiRequest::OpenPlan(id.clone()))); })
                .child(icon(IconName::ListTodo, 14., if pending { t.warning } else { t.muted_foreground }))
                .child(div().flex_none().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child("计划"))
                .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).child(display_title(plan)))
                .child(pill(text, fg, t.muted))
                .child(icon(IconName::ChevronRight, 12., t.muted_foreground)),
        )
        .when(!excerpt.is_empty(), |d| d.child(
            v_flex().gap_0p5().pl_5().text_size(px(12.)).text_color(t.muted_foreground)
                .children(excerpt.into_iter().map(|l| div().w_full().truncate().child(l))),
        ))
        .when(pending, |d| d.child(
            v_flex().gap_1p5()
                .child(div().text_size(px(11.)).text_color(t.muted_foreground).child("点击卡片在右侧查看完整计划;确认没问题再选择 Build。"))
                .child(actions(session, plan, "card")),
        ))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 标题取第一个非空行() {
        assert_eq!(plan_title("\n# 修复会话故障\n\n正文"), "修复会话故障");
        assert_eq!(plan_title("   \n"), "计划");
        assert_eq!(plan_title("## 二级标题"), "二级标题");
    }

    #[::core::prelude::v1::test]
    fn 摘要跳过标题分隔线与代码() {
        let md = "# 标题\n\n## Context\n目前图片只做**存储**和引用。\n\n---\n- 设计文档 `a.md` 写明\n1. 第一步\n```go\nfunc x() {}\n```\n> 引用\n- 第五行";
        assert_eq!(plan_excerpt(md, 10), vec!["目前图片只做存储和引用。", "设计文档 a.md 写明", "第一步", "引用", "第五行"]);
        assert_eq!(plan_excerpt(md, 2).len(), 2);
        assert!(plan_excerpt("# 只有标题", 3).is_empty());
    }
}
