//! 会话流的节奏常量与语义间距(对标 Cursor):同一回合内的步骤(工具、思考、子 Agent、待办)紧凑排列,
//! 正文与步骤之间留中等间距,新的用户消息前留大间距,让「一问一答」在视觉上成组。
//!
//! @author ixxxxoooo

use crate::row_groups::Row;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::*;
use loom_core::TimelineItem;

/// 一行式标题(工具、工具组、回合折叠、子 Agent 组、待办行……)的统一行高。
pub const ROW_H: f32 = 26.;
/// 步骤行的左右内边距(悬停底色在内容两侧的留白)。行整体向左外扩同样的量,
/// 所以图标列的左边与正文的左边是同一条线,悬停底色则比内容多出一圈。
pub const ROW_PAD: f32 = 8.;
/// 步骤行左侧的图标列宽(图标 / 转圈在其中居中,不同大小的图标中心对齐)。
pub const ICON_COL: f32 = 16.;
/// 图标列与文字之间的间距。
pub const ICON_GAP: f32 = 7.;
/// 步骤行文字相对正文左边的缩进;展开内容、嵌套的下一级都对齐到这里。
pub const TEXT_INSET: f32 = ICON_COL + ICON_GAP;

/// 步骤行的骨架:贴合内容宽度、统一行高与圆角,图标列与正文左对齐(见 [`ROW_PAD`])。
/// 调用方接着 `.id(..)`、加悬停与点击,第一个子元素放 [`icon_col`]。
pub fn step_row() -> Div {
    h_flex().self_start().max_w_full().min_h(px(ROW_H)).px(px(ROW_PAD)).ml(px(-ROW_PAD)).gap(px(ICON_GAP)).items_center().rounded(px(6.))
}

/// 不可点击的状态行(处理中、已中断、权限结果):没有悬停底色,所以不外扩,图标列同样贴正文左边。
pub fn plain_row() -> Div {
    h_flex().w_full().min_h(px(ROW_H)).gap(px(ICON_GAP)).items_center()
}

/// 图标列:固定宽度、内容居中。
///
/// 参数:`el` 图标或转圈。
pub fn icon_col(el: impl IntoElement) -> Div {
    div().flex_none().w(px(ICON_COL)).flex().items_center().justify_center().child(el)
}

/// 嵌套内容的容器(展开的工具组、子 Agent 组、回合过程、子 Agent 的活动):
/// 内容缩进到上一级文字的位置,图标列中线上画一条细引导线,把这一段和上下文分开。
pub fn nested(cx: &App) -> Div {
    let line = ICON_COL / 2. - 0.5;
    v_flex().w_full().ml(px(line)).pl(px(TEXT_INSET - line - 1.)).border_l_1().border_color(cx.theme().border)
}
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
    fn 步骤行的栅格() {
        // 嵌套内容与展开内容都对齐到上一级文字:缩进 = 图标列 + 间距
        assert_eq!(TEXT_INSET, ICON_COL + ICON_GAP);
        // 引导线画在图标列中线上,必须落在图标列以内
        assert!(ICON_COL / 2. < TEXT_INSET);
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
