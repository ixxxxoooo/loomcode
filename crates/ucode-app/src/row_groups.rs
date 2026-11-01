//! 会话流的「显示行」:把连续的普通工具调用折成一行摘要(「运行了 2 条命令、读取了 3 个文件」)。
//!
//! 纯逻辑,不依赖界面。虚拟列表里的一行不再一定是一个时间线条目,而是 [`Row`]:
//! 单个条目,或一段连续的工具调用。
//!
//! @author ixxxxoooo

use std::ops::Range;
use ucode_core::{TimelineItem, ToolCall, ToolKind, ToolStatus};

/// 一行显示内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// 单个时间线条目(下标)
    Item(usize),
    /// 一段连续的工具调用(条目下标范围,至少 2 个)
    Tools(Range<usize>),
    /// 末尾的「处理中」提示行(回合在进行、但没有任何条目正在流式输出时显示)
    Working,
}

impl Row {
    /// 这一行覆盖的条目范围。
    pub fn range(&self) -> Range<usize> {
        match self {
            Row::Item(i) => *i..*i + 1,
            Row::Tools(r) => r.clone(),
            Row::Working => 0..0,
        }
    }
}

/// 参与折叠的工具:普通工具调用(不含子 Agent、待办、切换模式,它们有自己的卡片)。
fn foldable(item: &TimelineItem) -> Option<&ToolCall> {
    match item {
        TimelineItem::Tool(t) if !matches!(t.kind, ToolKind::Subagent | ToolKind::Todo | ToolKind::SwitchMode) => Some(t),
        _ => None,
    }
}

/// 回合在进行时,末尾是否需要「处理中」提示:最后一个条目没有在流式输出 / 没有运行中的工具 /
/// 没有在等待用户(那些自己已经有动态指示)。
///
/// 参数:`items` 时间线条目。
pub fn needs_working_row(items: &[TimelineItem]) -> bool {
    match items.last() {
        None => true,
        Some(TimelineItem::Assistant { streaming, .. }) | Some(TimelineItem::Thinking { streaming, .. }) => !*streaming,
        Some(TimelineItem::Tool(t)) => t.status.is_finished(),
        Some(TimelineItem::Subagent(_)) | Some(TimelineItem::Permission(_)) | Some(TimelineItem::Question(_)) | Some(TimelineItem::PlanApproval(_)) => false,
        Some(_) => true,
    }
}

/// 由时间线条目算出显示行。连续 2 个及以上的普通工具调用合成一行。
///
/// 参数:`items` 时间线条目;`working` 回合是否在进行(需要时在末尾加「处理中」行)。
/// 返回:显示行列表。
pub fn build_rows(items: &[TimelineItem], working: bool) -> Vec<Row> {
    let mut rows = Vec::with_capacity(items.len());
    let mut i = 0;
    while i < items.len() {
        if foldable(&items[i]).is_some() {
            let mut j = i + 1;
            while j < items.len() && foldable(&items[j]).is_some() {
                j += 1;
            }
            if j - i >= 2 {
                rows.push(Row::Tools(i..j));
            } else {
                rows.push(Row::Item(i));
            }
            i = j;
        } else {
            rows.push(Row::Item(i));
            i += 1;
        }
    }
    if working && needs_working_row(items) {
        rows.push(Row::Working);
    }
    rows
}

/// 条目下标 → 所在显示行下标。
///
/// 参数:`rows` 显示行;`item_count` 条目总数。返回:长度为 `item_count` 的映射。
pub fn item_to_row(rows: &[Row], item_count: usize) -> Vec<usize> {
    let mut map = vec![0; item_count];
    for (ri, r) in rows.iter().enumerate() {
        for i in r.range() {
            if let Some(slot) = map.get_mut(i) {
                *slot = ri;
            }
        }
    }
    map
}

/// 一段工具调用是否需要默认展开:还有没结束的,或有失败的。
///
/// 参数:`tools` 这一段里的工具。
pub fn needs_attention(tools: &[&ToolCall]) -> bool {
    tools.iter().any(|t| !t.status.is_finished() || t.status == ToolStatus::Failed)
}

/// 摘要文案:按类别计数,如「运行了 2 条命令、读取了 3 个文件」。
///
/// 参数:`tools` 这一段里的工具。返回:用顿号连接的摘要;类别顺序固定。
pub fn summary(tools: &[&ToolCall]) -> String {
    let count = |f: &dyn Fn(ToolKind) -> bool| tools.iter().filter(|t| f(t.kind)).count();
    let parts: [(usize, &str, &str); 7] = [
        (count(&|k| k == ToolKind::Execute), "运行了", "条命令"),
        (count(&|k| k == ToolKind::Read), "读取了", "个文件"),
        (count(&|k| matches!(k, ToolKind::Edit | ToolKind::Write)), "编辑了", "个文件"),
        (count(&|k| matches!(k, ToolKind::Delete | ToolKind::Move)), "整理了", "个文件"),
        (count(&|k| k == ToolKind::Search), "搜索了", "次"),
        (count(&|k| matches!(k, ToolKind::Fetch | ToolKind::Browser)), "访问了", "个网页"),
        (count(&|k| matches!(k, ToolKind::Mcp | ToolKind::Other | ToolKind::Think)), "调用了", "次其他工具"),
    ];
    parts.iter().filter(|(n, _, _)| *n > 0).map(|(n, verb, unit)| format!("{verb} {n} {unit}")).collect::<Vec<_>>().join("、")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(id: &str, kind: ToolKind, status: ToolStatus) -> TimelineItem {
        let mut t = ToolCall::new(id, "x", "x");
        t.kind = kind;
        t.status = status;
        TimelineItem::Tool(t)
    }

    fn text(id: &str) -> TimelineItem {
        TimelineItem::Assistant { id: id.into(), text: "a".into(), streaming: false }
    }

    #[test]
    fn 连续两个以上工具合并成一行() {
        let items = vec![text("a"), tool("1", ToolKind::Read, ToolStatus::Completed), tool("2", ToolKind::Execute, ToolStatus::Completed), text("b"), tool("3", ToolKind::Read, ToolStatus::Completed), text("c")];
        assert_eq!(build_rows(&items, false), vec![Row::Item(0), Row::Tools(1..3), Row::Item(3), Row::Item(4), Row::Item(5)]);
    }

    #[test]
    fn 子agent和待办不参与合并() {
        let items = vec![tool("1", ToolKind::Read, ToolStatus::Completed), tool("2", ToolKind::Subagent, ToolStatus::Completed), tool("3", ToolKind::Read, ToolStatus::Completed)];
        assert_eq!(build_rows(&items, false).len(), 3);
    }

    #[test]
    fn 条目到行的映射() {
        let items = vec![text("a"), tool("1", ToolKind::Read, ToolStatus::Completed), tool("2", ToolKind::Read, ToolStatus::Completed), text("b")];
        let rows = build_rows(&items, false);
        assert_eq!(item_to_row(&rows, items.len()), vec![0, 1, 1, 2]);
    }

    #[test]
    fn 摘要按类别计数() {
        let a = ToolCall::new("1", "x", "x");
        let mut r = a.clone();
        r.kind = ToolKind::Read;
        let mut e = a.clone();
        e.kind = ToolKind::Execute;
        let mut e2 = a.clone();
        e2.kind = ToolKind::Execute;
        assert_eq!(summary(&[&e, &e2, &r]), "运行了 2 条命令、读取了 1 个文件");
    }

    #[test]
    fn 失败或未结束需要展开() {
        let mut ok = ToolCall::new("1", "x", "x");
        ok.status = ToolStatus::Completed;
        let mut bad = ok.clone();
        bad.status = ToolStatus::Failed;
        let mut run = ok.clone();
        run.status = ToolStatus::Running;
        assert!(!needs_attention(&[&ok]));
        assert!(needs_attention(&[&ok, &bad]));
        assert!(needs_attention(&[&run]));
    }
}

/// 第 `ix` 个条目是不是这一回合最后一条助手消息(它下面才显示复制等操作条)。
///
/// 规则:它自己不在流式输出;往后一直到下一条用户消息(或结尾)之间没有别的助手消息;
/// 如果后面就是结尾,回合还在进行(`running`)时不算。
///
/// 参数:`items` 时间线条目;`ix` 条目下标;`running` 回合是否在进行。
pub fn is_turn_last_assistant(items: &[TimelineItem], ix: usize, running: bool) -> bool {
    match items.get(ix) {
        Some(TimelineItem::Assistant { streaming: false, .. }) => {}
        _ => return false,
    }
    for item in &items[ix + 1..] {
        match item {
            TimelineItem::Assistant { .. } => return false,
            TimelineItem::User { .. } => return true,
            _ => {}
        }
    }
    !running
}

#[cfg(test)]
mod turn_tests {
    use super::*;

    fn a(id: &str, streaming: bool) -> TimelineItem {
        TimelineItem::Assistant { id: id.into(), text: "x".into(), streaming }
    }
    fn u() -> TimelineItem {
        TimelineItem::User { id: "u".into(), text: "q".into(), images: vec![], files: vec![] }
    }

    #[test]
    fn 回合最后一条助手消息() {
        let items = vec![u(), a("1", false), a("2", false), u(), a("3", false)];
        assert!(!is_turn_last_assistant(&items, 1, false));
        assert!(is_turn_last_assistant(&items, 2, false));
        assert!(is_turn_last_assistant(&items, 4, false));
    }

    #[test]
    fn 进行中与流式不显示() {
        let items = vec![u(), a("1", false)];
        assert!(!is_turn_last_assistant(&items, 1, true));
        assert!(!is_turn_last_assistant(&[u(), a("1", true)], 1, false));
    }
}
