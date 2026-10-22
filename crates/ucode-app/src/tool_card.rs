//! 工具调用行与展开内容。

use crate::ansi;
use crate::diff;
use crate::model::SessionModel;
use crate::ui::{MONO, icon, mono_box, pill};
use gpui_kit::assets::IconName;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ucode_core::*;

/// 工具大类对应的图标。
pub fn tool_icon(kind: ToolKind) -> IconName {
    match kind {
        ToolKind::Read => IconName::FileText,
        ToolKind::Edit => IconName::FilePen,
        ToolKind::Write => IconName::FilePlus,
        ToolKind::Delete => IconName::Trash,
        ToolKind::Move => IconName::FolderOutput,
        ToolKind::Search => IconName::Search,
        ToolKind::Execute => IconName::Terminal,
        ToolKind::Think => IconName::Brain,
        ToolKind::Fetch => IconName::Globe,
        ToolKind::Browser => IconName::Globe,
        ToolKind::Mcp => IconName::Plug,
        ToolKind::Subagent => IconName::Bot,
        ToolKind::Todo => IconName::ListTodo,
        ToolKind::SwitchMode => IconName::Map,
        ToolKind::Other => IconName::Wrench,
    }
}

/// 状态指示器。
fn status_indicator(status: ToolStatus, cx: &App) -> AnyElement {
    let t = cx.theme();
    match status {
        ToolStatus::Pending => icon(IconName::Clock, 12., t.muted_foreground).into_any_element(),
        ToolStatus::Running => Spinner::new().xsmall().into_any_element(),
        ToolStatus::Completed => icon(IconName::Check, 12., t.success).into_any_element(),
        ToolStatus::Failed => icon(IconName::X, 12., t.danger).into_any_element(),
    }
}

/// 统计一个工具里所有 Diff 的增删行数。
fn diff_stats(tool: &ToolCall) -> Option<(usize, usize)> {
    let mut found = false;
    let (mut a, mut r) = (0, 0);
    for c in &tool.content {
        if let ToolContent::Diff { old, new, .. } = c {
            found = true;
            let (_, x, y) = diff::compute(old, new);
            a += x;
            r += y;
        }
    }
    found.then_some((a, r))
}

/// 渲染一个工具调用(紧凑行 + 可展开内容)。
pub fn render_tool(session: &Entity<SessionModel>, tool: &ToolCall, cx: &App) -> AnyElement {
    let t = cx.theme();
    let expanded = session.read(cx).is_expanded(&tool.id);
    let has_body = !tool.content.is_empty();
    let id = tool.id.clone();
    let s = session.clone();
    let stats = diff_stats(tool);
    let title_color = if tool.status == ToolStatus::Failed { t.danger } else { t.foreground };

    let header = h_flex()
        .id(SharedString::from(format!("tool-{}", tool.id)))
        .w_full()
        .h(px(26.))
        .px_2()
        .gap_2()
        .items_center()
        .rounded(px(6.))
        .when(has_body, |d| d.cursor_pointer().hover(|d| d.bg(t.list_hover)))
        .when(has_body, |d| {
            d.on_click(move |_, _, cx| {
                s.update(cx, |m, cx| m.toggle(&id, cx));
            })
        })
        .child(icon(tool_icon(tool.kind), 14., t.muted_foreground))
        .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).text_color(title_color).child(tool.title.clone()))
        .when_some(stats, |d, (a, r)| {
            d.child(
                h_flex().gap_1().text_size(px(11.)).font_family(MONO)
                    .child(div().text_color(t.success).child(format!("+{a}")))
                    .child(div().text_color(t.danger).child(format!("−{r}"))),
            )
        })
        .when_some(tool.duration_ms.filter(|_| tool.status.is_finished()), |d, ms| {
            d.child(div().text_size(px(11.)).text_color(t.muted_foreground).child(format!("{ms} ms")))
        })
        .child(status_indicator(tool.status, cx))
        .when(has_body, |d| {
            d.child(icon(if expanded { IconName::ChevronDown } else { IconName::ChevronRight }, 12., t.muted_foreground))
        });

    v_flex()
        .w_full()
        .child(header)
        .when(expanded && has_body, |d| {
            d.child(v_flex().w_full().pl(px(30.)).pr_2().py_1().gap_1p5().children(tool.content.iter().map(|c| render_content(c, cx))))
        })
        .into_any_element()
}

/// 限制输出行数,避免一次渲染过长文本。
fn clip_lines(text: &str, max: usize) -> (String, usize) {
    let total = text.lines().count();
    if total <= max {
        return (text.to_string(), 0);
    }
    (text.lines().take(max).collect::<Vec<_>>().join("\n"), total - max)
}

/// 渲染一个内容块。
pub fn render_content(content: &ToolContent, cx: &App) -> AnyElement {
    let t = cx.theme();
    match content {
        ToolContent::Text(text) => {
            let (shown, more) = clip_lines(text, 60);
            mono_box(cx)
                .child(StyledText::new(shown))
                .when(more > 0, |d| d.child(div().mt_1().text_color(t.muted_foreground).child(format!("… 还有 {more} 行"))))
                .into_any_element()
        }
        ToolContent::Terminal { command, output, exit_code } => {
            let (shown, more) = clip_lines(output, 80);
            let parsed = ansi::parse(&shown);
            mono_box(cx)
                .child(
                    h_flex().gap_2().mb_1().items_center()
                        .child(div().text_color(t.muted_foreground).child(format!("$ {}", command.lines().next().unwrap_or(""))))
                        .when_some(*exit_code, |d, code| {
                            let (fg, bg) = if code == 0 { (t.success, t.background) } else { (t.danger, t.background) };
                            d.child(pill(format!("退出码 {code}"), fg, bg))
                        }),
                )
                .child(StyledText::new(parsed.text).with_highlights(parsed.runs))
                .when(more > 0, |d| d.child(div().mt_1().text_color(t.muted_foreground).child(format!("… 还有 {more} 行"))))
                .into_any_element()
        }
        ToolContent::Diff { path, old, new } => {
            let (lines, added, removed) = diff::compute(old, new);
            v_flex()
                .w_full()
                .gap_1()
                .child(
                    h_flex().gap_2().text_size(px(12.)).font_family(MONO)
                        .child(div().text_color(t.muted_foreground).child(path.clone()))
                        .child(div().text_color(t.success).child(format!("+{added}")))
                        .child(div().text_color(t.danger).child(format!("−{removed}"))),
                )
                .child(diff::render(&lines, 120, cx))
                .into_any_element()
        }
        ToolContent::Image { label, .. } => h_flex()
            .gap_2()
            .items_center()
            .p_2()
            .rounded(px(8.))
            .border_1()
            .border_color(t.border)
            .bg(t.muted)
            .child(
                div().size(px(56.)).rounded(px(6.)).bg(t.background).flex().items_center().justify_center()
                    .child(icon(IconName::Image, 22., t.muted_foreground)),
            )
            .child(div().text_size(px(12.)).text_color(t.muted_foreground).child(label.clone()))
            .into_any_element(),
    }
}
