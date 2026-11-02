//! 文件检查点回退(claude 的 `rewind_files` 控制请求):把 Claude 用编辑工具改过的文件
//! 恢复到某条用户消息发出之前的样子。
//!
//! 检查点按 claude 的用户消息 uuid 记录:实时发送时由我们生成并随消息发出,
//! 历史回放时取 jsonl 里那条记录的 `uuid`;时间线条目 id 与 uuid 的对应存在
//! `SessionState.user_uuids`(由 `AgentEvent::UserUuid` 写入)。
//!
//! @author ixxxxoooo

/// 一次回退请求的结果(预览或实际执行)。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RewindOutcome {
    /// 目标用户消息的 uuid。
    pub uuid: String,
    /// 是否只是预览(`dry_run`)。
    pub dry_run: bool,
    /// 能否回退(没有检查点、或者请求失败时为假)。
    pub can_rewind: bool,
    /// 会被恢复的文件(绝对路径;只有预览时 claude 才给出)。
    pub files: Vec<String>,
    /// 预览:回退会恢复的行数(相对于当前内容是新增)。
    pub insertions: u32,
    /// 预览:回退会去掉的行数。
    pub deletions: u32,
    /// 失败原因。
    pub error: Option<String>,
}

impl RewindOutcome {
    /// 失败结果。参数:`uuid` 目标消息,`dry_run` 是否预览,`error` 原因。返回:结果。
    pub fn failed(uuid: &str, dry_run: bool, error: impl Into<String>) -> Self {
        Self { uuid: uuid.into(), dry_run, error: Some(error.into()), ..Default::default() }
    }

    /// 给用户看的失败原因(把 claude 的英文原因翻成人话,其余原样)。
    pub fn reason(&self) -> String {
        match self.error.as_deref() {
            Some(e) if e.contains("No file checkpoint") => "这条消息没有文件检查点(可能没改过文件,或会话不是在开启检查点后发出的)".into(),
            Some(e) => e.to_string(),
            None => "没有可回退的改动".into(),
        }
    }

    /// 预览是否有实际可恢复的改动。
    pub fn has_changes(&self) -> bool {
        self.can_rewind && (!self.files.is_empty() || self.insertions > 0 || self.deletions > 0)
    }
}

/// 回退确认卡的状态(界面一次只确认一条)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RewindPrompt {
    /// 已发出预览请求,等待结果。
    Checking { item_id: String, uuid: String },
    /// 拿到预览结果,等用户确认。
    Ready { item_id: String, outcome: RewindOutcome },
    /// 已确认,正在回退;`files` 是预览里的文件数(实际执行的应答不带文件列表)。
    Applying { item_id: String, uuid: String, files: usize },
}

impl RewindPrompt {
    /// 对应的时间线条目 id。
    pub fn item_id(&self) -> &str {
        match self {
            Self::Checking { item_id, .. } | Self::Ready { item_id, .. } | Self::Applying { item_id, .. } => item_id,
        }
    }

    /// 目标 uuid。
    pub fn uuid(&self) -> &str {
        match self {
            Self::Checking { uuid, .. } | Self::Applying { uuid, .. } => uuid,
            Self::Ready { outcome, .. } => &outcome.uuid,
        }
    }
}

use crate::{NoticeLevel, Phase, SessionState, TimelineItem};

impl SessionState {
    /// 用户点了某条消息的「回退文件」:进入预览状态。
    /// 参数:`item_id` 用户消息条目 id。返回:要发预览请求的 uuid;不能回退时返回原因。
    pub fn rewind_begin(&mut self, item_id: &str) -> Result<String, String> {
        if self.phase == Phase::Running {
            return Err("回合进行中,结束或中断后再回退".into());
        }
        let Some(uuid) = self.user_uuids.get(item_id).cloned() else {
            return Err("这条消息没有检查点记录".into());
        };
        self.rewind = Some(RewindPrompt::Checking { item_id: item_id.into(), uuid: uuid.clone() });
        Ok(uuid)
    }

    /// 用户在确认卡上点了「回退」。返回:要执行回退的 uuid;预览没有改动或状态不对时为 `None`。
    pub fn rewind_confirm(&mut self) -> Option<String> {
        let Some(RewindPrompt::Ready { item_id, outcome }) = self.rewind.as_ref() else { return None };
        if !outcome.has_changes() || self.phase == Phase::Running {
            return None;
        }
        let uuid = outcome.uuid.clone();
        self.rewind = Some(RewindPrompt::Applying { item_id: item_id.clone(), uuid: uuid.clone(), files: outcome.files.len() });
        Some(uuid)
    }

    /// 关掉确认卡(取消,或看完「没有可回退的改动」)。
    pub fn rewind_cancel(&mut self) {
        self.rewind = None;
    }

    /// 归约后端的回退结果:预览 → 等确认;执行完 → 关卡并在时间线末尾留一条提示。过期的结果忽略。
    pub(crate) fn rewind_result(&mut self, o: RewindOutcome) {
        match self.rewind.take() {
            Some(RewindPrompt::Checking { item_id, uuid }) if o.dry_run && uuid == o.uuid => {
                self.rewind = Some(RewindPrompt::Ready { item_id, outcome: o });
            }
            Some(RewindPrompt::Applying { item_id, uuid, files }) if !o.dry_run && uuid == o.uuid => {
                let what = self.user_excerpt(&item_id);
                let (level, text) = if o.can_rewind && o.error.is_none() {
                    (NoticeLevel::Info, format!("已把 {files} 个文件回退到「{what}」发出之前(对话记录保留不变)"))
                } else {
                    (NoticeLevel::Warning, format!("回退失败:{}", o.reason()))
                };
                self.apply(crate::AgentEvent::Notice { level, text });
            }
            other => self.rewind = other,
        }
    }

    /// 用户消息的前 24 个字符(提示里指代这条消息)。
    fn user_excerpt(&self, item_id: &str) -> String {
        let text = self.items.iter().find_map(|i| match i {
            TimelineItem::User { id, text, .. } if id == item_id => Some(text.as_str()),
            _ => None,
        });
        let one_line: String = text.unwrap_or("这条消息").split_whitespace().collect::<Vec<_>>().join(" ");
        if one_line.chars().count() > 24 { format!("{}…", one_line.chars().take(24).collect::<String>()) } else { one_line }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AgentEvent;

    fn state() -> SessionState {
        let mut s = SessionState::default();
        s.phase = Phase::Idle;
        s.apply(AgentEvent::Append { parent: None, item: TimelineItem::User { id: "u1".into(), text: "把 README 改一下".into(), images: vec![], files: vec![] } });
        s.apply(AgentEvent::UserUuid { id: "u1".into(), uuid: "uuid-1".into() });
        s
    }

    fn preview(files: &[&str]) -> RewindOutcome {
        RewindOutcome { uuid: "uuid-1".into(), dry_run: true, can_rewind: true, files: files.iter().map(|f| f.to_string()).collect(), insertions: 1, deletions: 2, error: None }
    }

    #[test]
    fn 预览确认执行完整流程() {
        let mut s = state();
        assert_eq!(s.rewind_begin("u1").unwrap(), "uuid-1");
        s.apply(AgentEvent::Rewind(preview(&["/p/a", "/p/b"])));
        assert!(matches!(s.rewind, Some(RewindPrompt::Ready { .. })));
        assert_eq!(s.rewind_confirm().as_deref(), Some("uuid-1"));
        s.apply(AgentEvent::Rewind(RewindOutcome { uuid: "uuid-1".into(), can_rewind: true, ..Default::default() }));
        assert!(s.rewind.is_none());
        let last = s.items.last().unwrap();
        assert!(matches!(last, TimelineItem::Notice { text, .. } if text.contains("2 个文件") && text.contains("把 README 改一下")), "{last:?}");
    }

    #[test]
    fn 运行中或没有uuid不能回退() {
        let mut s = state();
        assert!(s.rewind_begin("u-未知").is_err());
        s.phase = Phase::Running;
        assert!(s.rewind_begin("u1").unwrap_err().contains("回合进行中"));
    }

    #[test]
    fn 没有改动不能确认_过期结果忽略() {
        let mut s = state();
        s.rewind_begin("u1").unwrap();
        s.apply(AgentEvent::Rewind(RewindOutcome { uuid: "别的".into(), dry_run: true, can_rewind: true, ..Default::default() }));
        assert!(matches!(s.rewind, Some(RewindPrompt::Checking { .. })), "别的 uuid 的结果不影响当前确认");
        s.apply(AgentEvent::Rewind(preview(&[])));
        let mut empty = preview(&[]);
        empty.insertions = 0;
        empty.deletions = 0;
        s.rewind = Some(RewindPrompt::Ready { item_id: "u1".into(), outcome: empty });
        assert!(s.rewind_confirm().is_none());
        s.rewind_cancel();
        assert!(s.rewind.is_none());
    }

    #[test]
    fn 执行失败留警告() {
        let mut s = state();
        s.rewind = Some(RewindPrompt::Applying { item_id: "u1".into(), uuid: "uuid-1".into(), files: 1 });
        s.apply(AgentEvent::Rewind(RewindOutcome::failed("uuid-1", false, "No file checkpoint found for this message.")));
        assert!(matches!(s.items.last(), Some(TimelineItem::Notice { level: NoticeLevel::Warning, .. })));
    }

    #[test]
    fn 失败原因翻成人话() {
        let o = RewindOutcome::failed("u", true, "No file checkpoint found for this message.");
        assert!(o.reason().contains("没有文件检查点"));
        assert_eq!(RewindOutcome::failed("u", true, "boom").reason(), "boom");
        assert!(!o.has_changes());
    }

    #[test]
    fn 有改动才算可回退() {
        let mut o = RewindOutcome { uuid: "u".into(), dry_run: true, can_rewind: true, ..Default::default() };
        assert!(!o.has_changes());
        o.files.push("/p/a.rs".into());
        assert!(o.has_changes());
        o.can_rewind = false;
        assert!(!o.has_changes());
    }
}
