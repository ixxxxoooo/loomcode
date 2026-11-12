//! 工具调用行与展开内容。

use crate::diff;
use crate::model::SessionModel;
use crate::ui::{MONO, icon, pill};
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
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

/// 编辑 Diff 收起时的预览高度。
const DIFF_PREVIEW_H: f32 = 130.;

/// 文字内容:执行中的工具用流光文字(随外观偏好),其余是普通文字。
///
/// 参数:`text` 文字;`id` 流光元素 id(同屏唯一);`running` 是否执行中。
fn live_text(text: String, id: String, running: bool, cx: &App) -> AnyElement {
    if running { crate::shimmer_text::shimmer_label(text, id, cx) } else { div().child(text).into_any_element() }
}

/// 状态指示器。
fn status_indicator(id: &str, status: ToolStatus, cx: &App) -> AnyElement {
    let t = cx.theme();
    match status {
        ToolStatus::Pending => icon(IconName::Clock, 12., t.muted_foreground).into_any_element(),
        ToolStatus::Running => crate::appearance::row_spinner(&format!("tool-sp-{id}"), cx),
        // 成功不加勾,只在进行中 / 失败时标示状态
        ToolStatus::Completed => div().into_any_element(),
        ToolStatus::Failed => icon(IconName::X, 12., t.danger).into_any_element(),
        // 被拒绝:用户自己的决定,不是出错;被中断:用户取消了回合
        ToolStatus::Denied => icon(IconName::Ban, 12., t.warning).into_any_element(),
        ToolStatus::Interrupted => icon(IconName::CirclePause, 12., t.muted_foreground).into_any_element(),
    }
}

/// 后台启动的工具(`run_in_background` 的命令、Monitor)的状态徽标:(文案, 颜色, 任务 id)。
fn bg_badge(session: &Entity<SessionModel>, tool: &ToolCall, cx: &App) -> Option<(String, Hsla, String)> {
    let task_id = tool.task_id.clone()?;
    let started_here = tool.input.get("run_in_background").and_then(|v| v.as_bool()).unwrap_or(false) || tool.name == "Monitor";
    if !started_here {
        return None;
    }
    let status = session.read(cx).state.task(&task_id).map(|t| t.status).unwrap_or(TaskStatus::Running);
    let text = if status == TaskStatus::Running { "后台运行".to_string() } else { format!("后台 · {}", status.label()) };
    Some((text, crate::task_ui::task_color(status, cx), task_id))
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
    let density = cx.try_global::<crate::UiPrefs>().map_or_else(Default::default, |p| p.tool_density);
    let detailed = crate::row_groups::expands_by_default(tool, density);
    // MCP / 浏览器 / 其它工具:展开后先给一块「参数」(入参的 JSON),不然看不出调用了什么
    let params = params_json(tool);
    let has_body = !tool.content.is_empty() || params.is_some();
    // 失败的命令默认展开,直接看到报错输出(详细档本来就展开);手动点击反转默认状态
    // 运行中的命令一旦有了实时输出也默认展开:长命令要能边等边看进度
    let live_exec = tool.status == ToolStatus::Running && tool.kind == ToolKind::Execute && has_body;
    let by_default = detailed || live_exec || (tool.status == ToolStatus::Failed && tool.kind == ToolKind::Execute && has_body);
    let expanded = session.read(cx).is_expanded(&tool.id) != by_default;
    let id = tool.id.clone();
    let s = session.clone();
    let stats = diff_stats(tool);
    let failed = tool.status == ToolStatus::Failed;
    let p = crate::tool_present::present(tool);
    // 命令带说明时,行内显示说明(Agent 写的一句话),具体命令留给展开详情
    let described = tool.kind == ToolKind::Execute && p.detail.is_some();
    let group = SharedString::from(format!("trow-{}", tool.id));
    let open_path = (matches!(tool.kind, ToolKind::Read | ToolKind::Edit | ToolKind::Write)).then(|| tool.locations.first().cloned().or_else(|| p.is_path.then(|| p.subject.clone()))).flatten();
    let running = tool.status == ToolStatus::Running;
    let subject_color = if failed { t.danger } else { t.foreground };
    let subject = if described {
        // 说明优先占宽,命令首行跟在后面(灰色等宽、截断),对标 Claude Code 两者都显示
        h_flex().min_w_0().gap(px(8.)).items_baseline()
            .child(div().min_w_0().truncate().text_size(px(13.)).text_color(subject_color).child(live_text(p.detail.clone().unwrap_or_default(), format!("tsh-d-{}", tool.id), running, cx)))
            .child(div().min_w_0().flex_shrink(1.).truncate().text_size(px(11.5)).font_family(MONO).text_color(t.muted_foreground.opacity(0.7)).child(p.subject.clone()))
            .into_any_element()
    } else if p.is_path {
        let name = p.subject.rsplit('/').next().unwrap_or(&p.subject).to_string();
        h_flex().min_w_0().gap(px(5.)).items_center()
            .child(crate::file_icons::file_icon_img(&name, false, false, 14.))
            .child(div().min_w_0().truncate().text_size(px(13.)).font_weight(FontWeight::MEDIUM).text_color(subject_color).child(live_text(name, format!("tsh-n-{}", tool.id), running, cx)))
            .into_any_element()
    } else if tool.kind == ToolKind::Execute {
        div().min_w_0().truncate().text_size(px(12.)).font_family(MONO).text_color(subject_color).child(if running { crate::shimmer_text::shimmer_label(p.subject.lines().next().unwrap_or("").to_string(), format!("tsh-c-{}", tool.id), cx) } else { crate::code_hl::shell_text(p.subject.lines().next().unwrap_or(""), "", cx).into_any_element() }).into_any_element()
    } else if tool.kind == ToolKind::Search {
        div().min_w_0().truncate().text_size(px(12.)).font_family(MONO).text_color(subject_color).child(live_text(p.subject.clone(), format!("tsh-s-{}", tool.id), running, cx)).into_any_element()
    } else {
        div().min_w_0().truncate().text_size(px(13.)).font_weight(FontWeight::MEDIUM).text_color(subject_color).child(live_text(p.subject.clone(), format!("tsh-o-{}", tool.id), running, cx)).into_any_element()
    };
    // 纯文字行(对齐 Cursor / OhMyAgent):贴合内容宽度,没有整行底色;箭头与「打开文件」悬停才出现;成功不打勾
    let header = h_flex()
        .id(SharedString::from(format!("tool-{}", tool.id)))
        .group(group.clone())
        .self_start()
        .max_w_full()
        .px_2()
        .min_h(px(crate::flow_style::ROW_H))
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
        .when(!p.verb.is_empty(), |d| d.child(div().flex_none().text_size(px(12.)).text_color(t.muted_foreground).child(live_text(p.verb.clone(), format!("tsh-v-{}", tool.id), running, cx))))
        .child(subject)
        .when(!described, |d| d.when_some(p.detail.clone().filter(|x| !x.is_empty()), |d, x| {
            d.child(div().flex_none().text_size(px(11.)).text_color(t.muted_foreground.opacity(0.75)).child(x))
        }))
        // 后台启动的命令 / 监听:徽标显示任务当前状态,点击在右侧「任务」标签里查看输出
        .when_some(bg_badge(session, tool, cx), |d, (text, color, task_id)| {
            let sess = session.clone();
            d.child(
                div().id(SharedString::from(format!("tool-bg-{}", tool.id))).flex_none().cursor_pointer()
                    .on_click(move |_, _, cx| { cx.stop_propagation(); sess.update(cx, |_, cx| cx.emit(crate::model::UiRequest::OpenTasks(Some(task_id.clone())))); })
                    .child(pill(text, color, cx.theme().muted)),
            )
        })
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
        // 被拒绝 / 被中断:用文字说明,和「失败」区分开
        .when(tool.status == ToolStatus::Denied, |d| d.child(div().flex_none().text_size(px(11.)).text_color(t.warning).child("已拒绝")))
        .when(tool.status == ToolStatus::Interrupted, |d| d.child(div().flex_none().text_size(px(11.)).text_color(t.muted_foreground).child("已中断")))
        .child(status_indicator(&tool.id, tool.status, cx))
        .when(has_body, |d| {
            d.child(
                div().flex_none().when(!expanded, |d| d.invisible().group_hover(group.clone(), |d| d.visible()))
                    .child(session.read(cx).motion.chevron(&tool.id, expanded, 11., t.muted_foreground)),
            )
        });

    v_flex()
        .w_full()
        .child(header)
        .when(expanded && has_body, |d| {
            let params_block = params.clone().map(|text| {
                let key = format!("cb:{}:params", tool.id);
                let fold = crate::code_box::Fold {
                    expanded: session.read(cx).is_expanded(&key),
                    collapsed_h: None,
                    copy: None,
                    on_toggle: std::rc::Rc::new({
                        let s = session.clone();
                        move |_, cx| { s.update(cx, |m, cx| m.toggle(&key, cx)); }
                    }),
                };
                v_flex().w_full().gap_0p5()
                    .child(div().text_size(px(11.)).text_color(t.muted_foreground).child("参数"))
                    .child(crate::tool_content::render_content(&ToolContent::Text(text), Some(&format!("{}-params", tool.id)), "json", Some((SharedString::from(format!("tc-{}-params", tool.id)), fold, false)), cx))
                    .into_any_element()
            });
            d.child(v_flex().w_full().pl(px(30.)).pr_2().py_1().gap_1p5().children(params_block).children(tool.content.iter().enumerate().map(|(ix, c)| {
                // 每块内容一个展开状态(键 `cb:<工具 id>:<第几块>`),长输出限高滚动
                let key = format!("cb:{}:{ix}", tool.id);
                let fold = crate::code_box::Fold {
                    expanded: session.read(cx).is_expanded(&key),
                    // 编辑 Diff 收起时只给约 7 行的预览
                    collapsed_h: matches!(c, ToolContent::Diff { .. }).then_some(DIFF_PREVIEW_H),
                    copy: None,
                    on_toggle: std::rc::Rc::new({
                        let s = session.clone();
                        move |_, cx| { s.update(cx, |m, cx| m.toggle(&key, cx)); }
                    }),
                };
                crate::tool_content::render_content(c, Some(&tool.id), &content_lang(tool), Some((SharedString::from(format!("tc-{}-{ix}", tool.id)), fold, !tool.status.is_finished())), cx)
            })))
        })
        .into_any_element()
}

/// 需要展示入参的工具(MCP、浏览器、其它)的入参 JSON(缩进排版);没有入参返回 `None`。
fn params_json(tool: &ToolCall) -> Option<String> {
    if !matches!(tool.kind, ToolKind::Mcp | ToolKind::Browser | ToolKind::Other) {
        return None;
    }
    let obj = tool.input.as_object().filter(|o| !o.is_empty())?;
    serde_json::to_string_pretty(obj).ok()
}

/// 工具结果文本的语言:读 / 写单个文件时按扩展名,其他工具(搜索、列表等)不着色。
///
/// 参数:`tool` 工具调用。返回:语言名,`text` 表示不着色。
fn content_lang(tool: &ToolCall) -> String {
    // MCP 等工具返回的 JSON 文本按 json 着色
    if matches!(tool.kind, ToolKind::Mcp | ToolKind::Browser | ToolKind::Other)
        && tool.content.iter().any(|c| matches!(c, ToolContent::Text(t) if { let t = t.trim_start(); (t.starts_with('{') || t.starts_with('[')) && serde_json::from_str::<serde_json::Value>(t).is_ok() }))
    {
        return "json".into();
    }
    if !matches!(tool.kind, ToolKind::Read | ToolKind::Write) {
        return "text".into();
    }
    let p = crate::tool_present::present(tool);
    let path = tool.locations.first().cloned().or_else(|| p.is_path.then(|| p.subject.clone()));
    path.map_or("text".into(), |p| crate::file_tree::language_for(std::path::Path::new(&p)).to_string())
}
