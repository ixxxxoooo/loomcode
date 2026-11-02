//! 会话流的节奏常量与语义间距(对标 Cursor):同一回合内的步骤(工具、思考、子 Agent、待办)紧凑排列,
//! 正文与步骤之间留中等间距,新的用户消息前留大间距,让「一问一答」在视觉上成组。
//!
//! @author ixxxxoooo

use crate::row_groups::Row;
use loom_core::TimelineItem;

/// 一行式标题(工具、工具组、回合折叠、子 Agent 组、待办行……)的统一行高。
pub const ROW_H: f32 = 26.;
/// 步骤与步骤之间的间距。
const GAP_STEP: f32 = 1.;
/// 正文 / 卡片与步骤之间的间距。
const GAP_BLOCK: f32 = 8.;
/// 用户消息前的间距(新的一问)。
const GAP_USER: f32 = 20.;

/// 会话流首行的顶部留白。
pub const FIRST_TOP: f32 = 13.;

/// 用户消息行里卡片顶边相对行顶的偏移(吸顶判断用)。
///
/// 参数:`row` 显示行下标。
pub fn user_row_inset(row: usize) -> f32 {
    if row == 0 { FIRST_TOP } else { GAP_USER }
}

/// 显示行的大类,决定与上一行的间距。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// 用户消息
    User,
    /// 回合内的一步:工具、工具组、思考、子 Agent、待办、任务通知、权限结果、回合折叠、处理中
    Step,
    /// 正文与其他卡片(助手回复、提示、改动汇总、计划、提问结果、画布)
    Block,
}

/// 一行属于哪一类。
///
/// 参数:`row` 显示行;`items` 时间线条目。
pub fn kind_of(row: &Row, items: &[TimelineItem]) -> Kind {
    match row {
        Row::Tools(_) | Row::Agents(_) | Row::Digest(_) | Row::Working => Kind::Step,
        Row::Item(i) => match items.get(*i) {
            Some(TimelineItem::User { .. }) => Kind::User,
            Some(TimelineItem::Tool(_) | TimelineItem::Thinking { .. } | TimelineItem::Subagent(_) | TimelineItem::Todos { .. } | TimelineItem::TaskNote { .. } | TimelineItem::Permission(_)) => Kind::Step,
            _ => Kind::Block,
        },
    }
}

/// 本行顶部的间距。
///
/// 参数:`prev` 上一行的类别(首行为 `None`);`cur` 本行类别。返回:像素。
pub fn gap_before(prev: Option<Kind>, cur: Kind) -> f32 {
    match (prev, cur) {
        (None, _) => 0.,
        (_, Kind::User) => GAP_USER,
        (Some(Kind::Step), Kind::Step) => GAP_STEP,
        _ => GAP_BLOCK,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 语义间距() {
        assert_eq!(gap_before(None, Kind::User), 0.);
        assert_eq!(gap_before(Some(Kind::Block), Kind::User), GAP_USER);
        assert_eq!(gap_before(Some(Kind::Step), Kind::Step), GAP_STEP);
        assert_eq!(gap_before(Some(Kind::Block), Kind::Step), GAP_BLOCK);
        assert_eq!(gap_before(Some(Kind::Step), Kind::Block), GAP_BLOCK);
        assert!(GAP_USER > GAP_BLOCK && GAP_BLOCK > GAP_STEP);
    }

    #[::core::prelude::v1::test]
    fn 行的类别() {
        let items = vec![
            TimelineItem::User { id: "u".into(), text: String::new(), images: vec![], files: vec![] },
            TimelineItem::Assistant { id: "a".into(), text: String::new(), streaming: false },
        ];
        assert_eq!(kind_of(&Row::Item(0), &items), Kind::User);
        assert_eq!(kind_of(&Row::Item(1), &items), Kind::Block);
        assert_eq!(kind_of(&Row::Tools(0..2), &items), Kind::Step);
        assert_eq!(kind_of(&Row::Working, &items), Kind::Step);
    }
}
