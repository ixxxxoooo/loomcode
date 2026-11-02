//! 「回退文件」:用户消息卡悬停的 ↶ 按钮 → 预览(dry_run)→ 输入框上方的确认卡 → 执行。
//! 状态机在 `loom_core::rewind`(`SessionState.rewind`),这里只有会话模型的入口与卡片渲染。
//!
//! 检查点只记录 Claude 用编辑工具(Write / Edit 等)改过的文件,命令行里改的不在其中;
//! 回退只动磁盘文件,对话记录保持不变。
//!
//! @author ixxxxoooo

use crate::model::SessionModel;
use crate::ui::{caption, icon};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::{AgentCommand, RewindPrompt};

/// 确认卡里最多列出的文件数,其余折成「等 N 个」。
const MAX_FILES: usize = 6;

impl SessionModel {
    /// 这条用户消息能否发起回退(有检查点 uuid)。
    pub fn can_rewind(&self, item_id: &str) -> bool {
        self.state.user_uuids.contains_key(item_id)
    }

    /// 点了某条消息的「回退文件」:发预览请求;不能回退时返回原因(由调用方提示)。
    /// 参数:`item_id` 用户消息条目 id。返回:失败原因。
    pub fn begin_rewind(&mut self, item_id: &str, cx: &mut Context<Self>) -> Result<(), String> {
        let uuid = self.state.rewind_begin(item_id)?;
        loom_diag::crumb!("回退文件:预览 {item_id}");
        self.handle.send(AgentCommand::RewindFiles { uuid, dry_run: true });
        cx.notify();
        Ok(())
    }

    /// 确认卡上点了「回退」。
    pub fn confirm_rewind(&mut self, cx: &mut Context<Self>) {
        if let Some(uuid) = self.state.rewind_confirm() {
            loom_diag::info!("回退文件:执行 uuid={uuid}");
            self.handle.send(AgentCommand::RewindFiles { uuid, dry_run: false });
        }
        cx.notify();
    }

    /// 关闭确认卡。
    pub fn cancel_rewind(&mut self, cx: &mut Context<Self>) {
        self.state.rewind_cancel();
        cx.notify();
    }
}

/// 调试用:`LOOM_REWIND=第几条用户消息`(从 1 起),回放完后自动发起回退预览,
/// `LOOM_REWIND_APPLY=1` 时再自动确认,方便截图检查确认卡与结果提示。
pub fn debug_autostart<T: 'static>(model: Entity<SessionModel>, window: &mut Window, cx: &mut Context<T>) {
    let Some(n) = std::env::var("LOOM_REWIND").ok().and_then(|v| v.parse::<usize>().ok()) else { return };
    let apply = std::env::var("LOOM_REWIND_APPLY").is_ok_and(|v| v == "1");
    cx.spawn_in(window, async move |_, cx| {
        cx.background_executor().timer(std::time::Duration::from_millis(2500)).await;
        let _ = cx.update(|_, cx| {
            let id = model.read(cx).state.items.iter().filter(|i| matches!(i, loom_core::TimelineItem::User { .. })).nth(n.saturating_sub(1)).map(|i| i.id().to_string());
            if let Some(id) = id
                && let Err(why) = model.update(cx, |m, cx| m.begin_rewind(&id, cx))
            {
                loom_diag::warn!("LOOM_REWIND:{why}");
            }
        });
        if apply {
            cx.background_executor().timer(std::time::Duration::from_millis(1000)).await;
            let _ = cx.update(|_, cx| model.update(cx, |m, cx| m.confirm_rewind(cx)));
        }
    })
    .detach();
}

/// 路径只留最后两段(目录/文件),确认卡里够认出是哪个文件。
pub fn short_path(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    match parts.len() {
        0 => path.to_string(),
        1 => parts[0].to_string(),
        n => format!("{}/{}", parts[n - 2], parts[n - 1]),
    }
}

/// 输入框上方的回退确认卡;没有进行中的回退时返回 `None`。
pub fn render(session: &Entity<SessionModel>, cx: &App) -> Option<AnyElement> {
    let t = cx.theme();
    let prompt = session.read(cx).state.rewind.clone()?;
    let card = v_flex().w_full().p_3().gap_2().rounded(px(12.)).border_1().border_color(t.border).bg(t.background).shadow_sm();
    let title = |text: &str, ic: IconName, color: Hsla| {
        h_flex().gap_2().items_center().child(icon(ic, 14., color)).child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(text.to_string()))
    };
    let (s_ok, s_no) = (session.clone(), session.clone());
    let cancel = |label: &'static str| Button::new("rewind-cancel").small().label(label).on_click(move |_, _, cx| s_no.update(cx, |m, cx| m.cancel_rewind(cx)));
    let body = match prompt {
        RewindPrompt::Checking { .. } => card.child(title("正在检查这条消息之后的文件改动…", IconName::LoaderCircle, t.muted_foreground)).child(h_flex().child(cancel("取消"))),
        RewindPrompt::Applying { files, .. } => card.child(title(&format!("正在回退 {files} 个文件…"), IconName::LoaderCircle, t.muted_foreground)),
        RewindPrompt::Ready { outcome, .. } if !outcome.has_changes() => card
            .child(title("没有可回退的文件改动", IconName::Info, t.muted_foreground))
            .child(caption(outcome.reason(), cx))
            .child(caption("检查点只记录 Claude 用编辑工具改过的文件;命令行里改的文件请用 Git 还原。", cx))
            .child(h_flex().child(cancel("知道了"))),
        RewindPrompt::Ready { outcome, .. } => {
            let n = outcome.files.len();
            let more = n.saturating_sub(MAX_FILES);
            card.child(title(&format!("把 {n} 个文件回退到这条消息发出之前?"), IconName::Undo, t.warning))
                .child(
                    v_flex().gap_0p5().pl(px(22.))
                        .children(outcome.files.iter().take(MAX_FILES).map(|f| {
                            h_flex().gap_1p5().items_center().child(icon(IconName::FileText, 12., t.muted_foreground))
                                .child(div().text_size(px(12.)).truncate().child(short_path(f)))
                                .into_any_element()
                        }))
                        .when(more > 0, |d| d.child(caption(format!("等另外 {more} 个文件"), cx))),
                )
                .child(
                    h_flex().pl(px(22.)).gap_2().text_size(px(12.))
                        .child(div().text_color(t.success).child(format!("+{}", outcome.insertions)))
                        .child(div().text_color(t.danger).child(format!("−{}", outcome.deletions)))
                        .child(div().text_color(t.muted_foreground).child("(回退后相对当前内容的变化)")),
                )
                .child(caption("只恢复 Claude 用编辑工具改过的文件,对话记录与命令行里做的改动不受影响。未保存到 Git 的手动修改可能被覆盖。", cx))
                .child(
                    h_flex().gap_2()
                        .child(Button::new("rewind-ok").small().primary().label("回退").on_click(move |_, _, cx| s_ok.update(cx, |m, cx| m.confirm_rewind(cx))))
                        .child(cancel("取消")),
                )
        }
    };
    Some(
        div().w_full().flex().justify_center().px_4().pt_1().pb_1()
            .child(div().w_full().max_w(px(crate::timeline::content_max_w(cx))).child(body))
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 路径只留最后两段() {
        assert_eq!(short_path("/p/src/router.rs"), "src/router.rs");
        assert_eq!(short_path("README.md"), "README.md");
        assert_eq!(short_path("/a.txt"), "a.txt");
        assert_eq!(short_path(""), "");
    }
}
