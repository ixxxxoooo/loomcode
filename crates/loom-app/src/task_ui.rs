//! 后台任务与子 Agent 的公共展示:状态样式、完成通知行、输入框上方的「任务坞」。
//!
//! 输入框上方的任务坞见 `task_dock.rs`。
//!
//! @author ixxxxoooo

use crate::model::{SessionModel, UiRequest};
use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex};
use gpui_kit::*;
use loom_core::*;

/// 子 Agent 状态的文案与颜色。
pub fn subagent_style(status: SubagentStatus, cx: &App) -> (&'static str, Hsla) {
    let t = cx.theme();
    match status {
        SubagentStatus::Starting => ("启动中", t.muted_foreground),
        SubagentStatus::Running => ("运行中", t.primary),
        SubagentStatus::Background => ("后台运行", t.primary),
        SubagentStatus::Completed => ("已完成", t.success),
        SubagentStatus::Failed => ("已失败", t.danger),
        SubagentStatus::Stopped => ("已停止", t.muted_foreground),
    }
}

/// 后台任务状态的颜色。
pub fn task_color(status: TaskStatus, cx: &App) -> Hsla {
    let t = cx.theme();
    match status {
        TaskStatus::Running => t.primary,
        TaskStatus::Completed => t.success,
        TaskStatus::Failed => t.danger,
        TaskStatus::Stopped => t.muted_foreground,
    }
}

/// 后台任务类别的图标。
pub fn kind_icon(kind: TaskKind) -> IconName {
    match kind {
        TaskKind::Shell => IconName::Terminal,
        TaskKind::Agent => IconName::Bot,
        TaskKind::Monitor => IconName::Eye,
        TaskKind::Workflow => IconName::Workflow,
        TaskKind::Other => IconName::Zap,
    }
}

/// 后台任务状态的图标(运行中用转圈,另行处理)。
pub fn status_icon(status: TaskStatus) -> IconName {
    match status {
        TaskStatus::Running => IconName::Activity,
        TaskStatus::Completed => IconName::CircleCheck,
        TaskStatus::Failed => IconName::CircleX,
        TaskStatus::Stopped => IconName::CircleStop,
    }
}

/// 一行紧凑的任务统计:`用时 · 工具数 · token`,没有的项跳过。
pub fn meta_text(elapsed_ms: Option<u64>, tools: u32, tokens: u64) -> String {
    let mut parts = Vec::new();
    if let Some(ms) = elapsed_ms {
        parts.push(format_elapsed(ms));
    }
    if tools > 0 {
        parts.push(format!("{tools} 次工具"));
    }
    if tokens > 0 {
        parts.push(format!("{} token", format_tokens(tokens)));
    }
    parts.join(" · ")
}

/// 时间线里的任务结束通知行:状态图标 + 摘要,点击在右侧「任务」标签里查看。
pub fn render_note(session: &Entity<SessionModel>, task_id: &str, kind: TaskKind, status: TaskStatus, text: &str, cx: &App) -> AnyElement {
    let t = cx.theme();
    let color = match status {
        TaskStatus::Failed => t.danger,
        TaskStatus::Completed => t.muted_foreground,
        _ => t.muted_foreground,
    };
    let (s, id) = (session.clone(), task_id.to_string());
    h_flex().id(SharedString::from(format!("tnote-{task_id}-{}", status.label()))).self_start().max_w_full().px_2().py(px(3.)).gap_1p5().items_center().rounded(px(6.)).cursor_pointer()
        .hover(|d| d.bg(t.list_hover))
        .on_click(move |_, _, cx| { s.update(cx, |_, cx| cx.emit(UiRequest::OpenTasks(Some(id.clone())))); })
        .child(icon(kind_icon(kind), 12., t.muted_foreground))
        .child(div().min_w_0().truncate().text_size(px(12.)).text_color(color).child(text.to_string()))
        .child(icon(status_icon(status), 12., task_color(status, cx)))
        .into_any_element()
}

/// 任务坞里最多直接列出几行,其余在右侧「任务」标签里看。

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 统计文案() {
        assert_eq!(meta_text(Some(65_000), 3, 12_300), "1 分 05 秒 · 3 次工具 · 12.3k token");
        assert_eq!(meta_text(None, 0, 0), "");
        assert_eq!(meta_text(Some(5_000), 0, 800), "5 秒 · 800 token");
    }
}
