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
use loom_core::*;

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
        // 成功不加勾,只在进行中 / 失败时标示状态
        ToolStatus::Completed => div().into_any_element(),
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
    // 详细档下编辑与命令默认展开;手动点击反转默认状态
    let detailed = cx.try_global::<crate::UiPrefs>().is_some_and(|p| p.tool_density == loom_config::ToolDensity::Detailed) && crate::row_groups::expands_in_detailed(tool);
    let expanded = session.read(cx).is_expanded(&tool.id) != detailed;
    let has_body = !tool.content.is_empty();
    let id = tool.id.clone();
    let s = session.clone();
    let stats = diff_stats(tool);
    let failed = tool.status == ToolStatus::Failed;
    let p = crate::tool_present::present(tool);
    // 命令带说明时,行内显示说明(Agent 写的一句话),具体命令留给展开详情
    let described = tool.kind == ToolKind::Execute && p.detail.is_some();
    let group = SharedString::from(format!("trow-{}", tool.id));
    let open_path = (matches!(tool.kind, ToolKind::Read | ToolKind::Edit | ToolKind::Write)).then(|| tool.locations.first().cloned().or_else(|| p.is_path.then(|| p.subject.clone()))).flatten();
    let subject_color = if failed { t.danger } else { t.foreground };
    let subject = if described {
        div().min_w_0().truncate().text_size(px(13.)).text_color(subject_color).child(p.detail.clone().unwrap_or_default()).into_any_element()
    } else if p.is_path {
        let name = p.subject.rsplit('/').next().unwrap_or(&p.subject).to_string();
        h_flex().min_w_0().gap(px(5.)).items_center()
            .child(crate::file_icons::file_icon_img(&name, false, false, 14.))
            .child(div().min_w_0().truncate().text_size(px(13.)).font_weight(FontWeight::MEDIUM).text_color(subject_color).child(name))
            .into_any_element()
    } else if matches!(tool.kind, ToolKind::Execute | ToolKind::Search) {
        div().min_w_0().truncate().text_size(px(12.)).font_family(MONO).text_color(subject_color).child(p.subject.clone()).into_any_element()
    } else {
        div().min_w_0().truncate().text_size(px(13.)).font_weight(FontWeight::MEDIUM).text_color(subject_color).child(p.subject.clone()).into_any_element()
    };
    // 纯文字行(对齐 Cursor / OhMyAgent):贴合内容宽度,没有整行底色;箭头与「打开文件」悬停才出现;成功不打勾
    let header = h_flex()
        .id(SharedString::from(format!("tool-{}", tool.id)))
        .group(group.clone())
        .self_start()
        .max_w_full()
        .px_2()
        .py(px(5.))
        .gap(px(7.))
        .items_center()
        .rounded(px(6.))
        .when(has_body, |d| d.cursor_pointer().hover(|d| d.bg(t.list_hover)))
        .when(has_body, |d| {
            d.on_click(move |_, _, cx| {
                s.update(cx, |m, cx| m.toggle(&id, cx));
            })
        })
        .child(div().w(px(16.)).flex().justify_center().child(icon(tool_icon(tool.kind), 12., if failed { t.danger } else { t.muted_foreground })))
        .when(!p.verb.is_empty(), |d| d.child(div().flex_none().text_size(px(12.)).text_color(t.muted_foreground).child(p.verb.clone())))
        .child(subject)
        .when(!described, |d| d.when_some(p.detail.clone().filter(|x| !x.is_empty()), |d, x| {
            d.child(div().flex_none().text_size(px(11.)).text_color(t.muted_foreground.opacity(0.75)).child(x))
        }))
        .when_some(open_path, |d, path| {
            let sess = session.clone();
            d.child(
                div().id(SharedString::from(format!("tool-open-{}", tool.id))).flex_none().size(px(16.)).flex().items_center().justify_center().rounded(px(4.))
                    .invisible().group_hover(group.clone(), |d| d.visible())
                    .hover(|d| d.bg(t.border))
                    .on_click(move |_, _, cx| { cx.stop_propagation(); sess.update(cx, |_, cx| cx.emit(crate::model::UiRequest::OpenFile(path.clone()))); })
                    .child(icon(IconName::ExternalLink, 11., t.muted_foreground)),
            )
        })
        .when_some(stats, |d, (a, r)| {
            d.child(
                h_flex().gap_1().text_size(px(11.)).font_family(MONO)
                    .child(div().text_color(t.success).child(format!("+{a}")))
                    .child(div().text_color(t.danger).child(format!("−{r}"))),
            )
        })
        // 只在耗时较长(≥1 秒)时显示,太短的耗时只是噪音
        .when_some(tool.duration_ms.filter(|ms| tool.status.is_finished() && *ms >= 1000), |d, ms| {
            d.child(div().text_size(px(11.)).text_color(t.muted_foreground.opacity(0.75)).child(format!("{:.1} s", ms as f64 / 1000.)))
        })
        .child(status_indicator(tool.status, cx))
        .when(has_body, |d| {
            d.child(
                div().flex_none().when(!expanded, |d| d.invisible().group_hover(group.clone(), |d| d.visible()))
                    .child(icon(if expanded { IconName::ChevronDown } else { IconName::ChevronRight }, 11., t.muted_foreground)),
            )
        });

    v_flex()
        .w_full()
        .child(header)
        .when(expanded && has_body, |d| {
            d.child(v_flex().w_full().pl(px(30.)).pr_2().py_1().gap_1p5().children(tool.content.iter().map(|c| render_content(c, Some(&tool.id), cx))))
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
/// 参数:`run_id` 所属工具调用的 id;终端块用它显示「在终端面板查看」。
pub fn render_content(content: &ToolContent, run_id: Option<&str>, cx: &App) -> AnyElement {
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
                    h_flex().gap_2().mb_1().items_start()
                        // 长命令在卡片内折行,不再把右侧的状态与「在终端面板查看」挤出去
                        .child(div().flex_1().min_w_0().text_color(t.muted_foreground).child(format!("$ {}", command.lines().next().unwrap_or(""))))
                        .when_some(*exit_code, |d, code| {
                            let (fg, bg) = if code == 0 { (t.success, t.background) } else { (t.danger, t.background) };
                            d.child(div().flex_none().child(pill(format!("退出码 {code}"), fg, bg)))
                        })
                        .when_some(run_id.map(str::to_string), |d, id| {
                            // 只有 Agent 窗口处理这个动作(IDE 窗口里没有对应面板,等于无操作)
                            d.child(
                                div().id(SharedString::from(format!("view-run-{id}"))).flex_none().cursor_pointer().text_size(px(11.)).text_color(t.muted_foreground)
                                    .hover(|s| s.text_color(t.primary))
                                    .on_click(move |_, window, cx| window.dispatch_action(Box::new(crate::workbench::ViewAgentRun { id: id.clone() }), cx))
                                    .child("在终端面板查看"),
                            )
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
                        .child({
                            // 点击路径:在 IDE 编辑器里打开(Agent 窗口里无处理者,等于无操作)
                            let target = path.clone();
                            div().id(SharedString::from(format!("diff-path-{path}"))).cursor_pointer().text_color(t.muted_foreground)
                                .hover(|s| s.text_color(t.primary).underline())
                                .on_click(move |_, window, cx| window.dispatch_action(Box::new(crate::ide_window::OpenPath { path: target.clone() }), cx))
                                .child(path.clone())
                        })
                        .child(div().text_color(t.success).child(format!("+{added}")))
                        .child(div().text_color(t.danger).child(format!("−{removed}"))),
                )
                .child(diff::render(&lines, 120, cx))
                .into_any_element()
        }
        ToolContent::Image { label, path: Some(path) } if std::path::Path::new(path).is_file() => v_flex()
            .gap_1()
            .child(img(std::path::PathBuf::from(path)).max_w_full().max_h(px(360.)).rounded(px(8.)).border_1().border_color(t.border))
            .child(div().text_size(px(12.)).text_color(t.muted_foreground).child(label.clone()))
            .into_any_element(),
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
