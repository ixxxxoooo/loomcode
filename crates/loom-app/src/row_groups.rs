//! 会话流的「显示行」:把连续的普通工具调用折成一行摘要(「运行了 2 条命令、读取了 3 个文件」)。
//!
//! 纯逻辑,不依赖界面。虚拟列表里的一行不再一定是一个时间线条目,而是 [`Row`]:
//! 单个条目,或一段连续的工具调用。
//!
//! @author ixxxxoooo

use std::collections::HashMap;
use std::ops::Range;
use loom_config::ToolDensity;
use loom_core::{TimelineItem, ToolCall, ToolKind, ToolStatus};

/// 一行显示内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// 单个时间线条目(下标)
    Item(usize),
    /// 一段连续的工具调用(条目下标范围,至少 2 个)
    Tools(Range<usize>),
    /// 已结束回合的整段过程(思考、工具调用、中间叙述)折成的「已工作 12分8秒」一行
    Digest(Range<usize>),
    /// 末尾的「处理中」提示行(回合在进行、但没有任何条目正在流式输出时显示)
    Working,
}

impl Row {
    /// 这一行覆盖的条目范围。
    pub fn range(&self) -> Range<usize> {
        match self {
            Row::Item(i) => *i..*i + 1,
            Row::Tools(r) | Row::Digest(r) => r.clone(),
            Row::Working => 0..0,
        }
    }
}

/// 参与折叠的工具:普通工具调用(不含子 Agent、待办、切换模式,它们有自己的卡片)。
///
/// 精简档所有普通工具都参与;标准档只有浏览类(读、搜、访问网页、思考);详细档都不折叠。
fn foldable(item: &TimelineItem, density: ToolDensity) -> Option<&ToolCall> {
    let TimelineItem::Tool(t) = item else { return None };
    if matches!(t.kind, ToolKind::Subagent | ToolKind::Todo | ToolKind::SwitchMode) {
        return None;
    }
    let ok = match density {
        ToolDensity::Compact => true,
        ToolDensity::Standard => matches!(t.kind, ToolKind::Read | ToolKind::Search | ToolKind::Fetch | ToolKind::Browser | ToolKind::Think),
        ToolDensity::Detailed => false,
    };
    ok.then_some(t)
}

/// 这个工具在「详细」档下是否默认展开(编辑 Diff、命令输出直接显示)。
pub fn expands_in_detailed(tool: &ToolCall) -> bool {
    tool.content.iter().any(|c| matches!(c, loom_core::ToolContent::Diff { .. } | loom_core::ToolContent::Terminal { .. }))
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

/// 回合折叠行里该折进去的条目:思考、工具调用(含子 Agent)、待办、中间的助手叙述;
/// 用户消息、提示、改动汇总、权限 / 提问 / 计划卡、画布、最终回答都保持可见。
fn digest_foldable(item: &TimelineItem) -> bool {
    matches!(item, TimelineItem::Thinking { .. } | TimelineItem::Tool(_) | TimelineItem::Subagent(_) | TimelineItem::Todos { .. } | TimelineItem::Assistant { .. })
}

/// 算出每个已结束回合的折叠范围。
///
/// 规则:从用户消息之后到该回合的最终回答之前;要折的条目必须连续成一段(中间夹着提示等就不折);
/// 回合必须有用时记录;最后一个回合在运行中(`running`)时不折。
///
/// 参数:`items` 时间线条目;`running` 回合是否在进行;`durations` 用户消息 id → 回合用时。
pub fn digest_ranges(items: &[TimelineItem], running: bool, durations: &HashMap<String, u64>) -> Vec<Range<usize>> {
    let users: Vec<usize> = items.iter().enumerate().filter(|(_, i)| matches!(i, TimelineItem::User { .. })).map(|(ix, _)| ix).collect();
    let mut out = Vec::new();
    for (n, &u) in users.iter().enumerate() {
        let end = users.get(n + 1).copied().unwrap_or(items.len());
        let is_last = n + 1 == users.len();
        let TimelineItem::User { id, .. } = &items[u] else { continue };
        if (is_last && running) || !durations.contains_key(id) {
            continue;
        }
        // 最终回答:往回找到的第一个助手消息,且它之后没有思考 / 工具调用
        let mut stop = end;
        for ix in (u + 1..end).rev() {
            match &items[ix] {
                TimelineItem::Assistant { .. } => { stop = ix; break; }
                TimelineItem::Thinking { .. } | TimelineItem::Tool(_) => break,
                _ => {}
            }
        }
        let folded: Vec<usize> = (u + 1..stop).filter(|&ix| digest_foldable(&items[ix])).collect();
        let (Some(&first), Some(&last)) = (folded.first(), folded.last()) else { continue };
        // 必须连续:first..=last 之间全是可折叠条目
        if folded.len() == last - first + 1 {
            out.push(first..last + 1);
        }
    }
    out
}

/// 由时间线条目算出显示行。连续 2 个及以上的可折叠工具调用合成一行(哪些可折叠由详细程度决定)。
///
/// 参数:`items` 时间线条目;`working` 回合是否在进行(需要时在末尾加「处理中」行);`density` 工具调用详细程度;`durations` 回合用时(有用时记录的已结束回合会折成一行)。
/// 返回:显示行列表。
pub fn build_rows(items: &[TimelineItem], working: bool, density: ToolDensity, durations: &HashMap<String, u64>) -> Vec<Row> {
    let digests = digest_ranges(items, working, durations);
    let mut rows = Vec::with_capacity(items.len());
    let mut i = 0;
    while i < items.len() {
        // 待处理的权限 / 提问在输入框上方显示,时间线里不占行
        if crate::dock_cards::hidden_in_timeline(&items[i]) {
            i += 1;
            continue;
        }
        if let Some(d) = digests.iter().find(|d| d.start == i) {
            rows.push(Row::Digest(d.clone()));
            i = d.end;
            continue;
        }
        if foldable(&items[i], density).is_some() {
            let mut j = i + 1;
            while j < items.len() && foldable(&items[j], density).is_some() {
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
        assert_eq!(build_rows(&items, false, ToolDensity::Compact, &Default::default()), vec![Row::Item(0), Row::Tools(1..3), Row::Item(3), Row::Item(4), Row::Item(5)]);
    }

    #[test]
    fn 子agent和待办不参与合并() {
        let items = vec![tool("1", ToolKind::Read, ToolStatus::Completed), tool("2", ToolKind::Subagent, ToolStatus::Completed), tool("3", ToolKind::Read, ToolStatus::Completed)];
        assert_eq!(build_rows(&items, false, ToolDensity::Compact, &Default::default()).len(), 3);
    }

    #[test]
    fn 标准档只合并浏览类() {
        // 读、读、跑命令、读、读:命令打断,两段浏览各自合并,命令单独一行
        let items = vec![
            tool("1", ToolKind::Read, ToolStatus::Completed), tool("2", ToolKind::Search, ToolStatus::Completed),
            tool("3", ToolKind::Execute, ToolStatus::Completed),
            tool("4", ToolKind::Read, ToolStatus::Completed), tool("5", ToolKind::Fetch, ToolStatus::Completed),
            tool("6", ToolKind::Edit, ToolStatus::Completed),
        ];
        assert_eq!(build_rows(&items, false, ToolDensity::Standard, &Default::default()), vec![Row::Tools(0..2), Row::Item(2), Row::Tools(3..5), Row::Item(5)]);
    }

    #[test]
    fn 详细档不合并() {
        let items = vec![tool("1", ToolKind::Read, ToolStatus::Completed), tool("2", ToolKind::Read, ToolStatus::Completed), tool("3", ToolKind::Execute, ToolStatus::Completed)];
        assert_eq!(build_rows(&items, false, ToolDensity::Detailed, &Default::default()), vec![Row::Item(0), Row::Item(1), Row::Item(2)]);
    }

    #[test]
    fn 详细档只展开有_diff_或输出的工具() {
        let mut edit = ToolCall::new("1", "x", "x");
        edit.content = vec![loom_core::ToolContent::Diff { path: "a".into(), old: "".into(), new: "b".into() }];
        let mut read = ToolCall::new("2", "x", "x");
        read.content = vec![loom_core::ToolContent::Text("很长的文件内容".into())];
        assert!(expands_in_detailed(&edit));
        assert!(!expands_in_detailed(&read));
        assert!(!expands_in_detailed(&ToolCall::new("3", "x", "x")));
    }

    fn user(id: &str) -> TimelineItem {
        TimelineItem::User { id: id.into(), text: "q".into(), images: vec![], files: vec![] }
    }

    fn think(id: &str) -> TimelineItem {
        TimelineItem::Thinking { id: id.into(), text: "t".into(), streaming: false }
    }

    #[test]
    fn 已结束回合的过程折成一行() {
        // 用户 → 思考 → 读 → 改 → 最终回答
        let items = vec![user("u1"), think("t1"), tool("1", ToolKind::Read, ToolStatus::Completed), tool("2", ToolKind::Edit, ToolStatus::Completed), text("answer")];
        let d: HashMap<String, u64> = [("u1".to_string(), 90)].into();
        assert_eq!(build_rows(&items, false, ToolDensity::Standard, &d), vec![Row::Item(0), Row::Digest(1..4), Row::Item(4)]);
        // 没有用时记录:不折
        assert_eq!(build_rows(&items, false, ToolDensity::Standard, &Default::default()).len(), 5);
        // 最后一个回合还在运行:不折
        assert!(!build_rows(&items, true, ToolDensity::Standard, &d).iter().any(|r| matches!(r, Row::Digest(_))));
    }

    #[test]
    fn 中间叙述折进去最终回答保留() {
        let items = vec![user("u1"), text("先看看"), tool("1", ToolKind::Read, ToolStatus::Completed), text("最终答案")];
        let d: HashMap<String, u64> = [("u1".to_string(), 5)].into();
        assert_eq!(build_rows(&items, false, ToolDensity::Standard, &d), vec![Row::Item(0), Row::Digest(1..3), Row::Item(3)]);
    }

    #[test]
    fn 只有回答没有过程不折() {
        let items = vec![user("u1"), text("直接回答")];
        let d: HashMap<String, u64> = [("u1".to_string(), 5)].into();
        assert_eq!(build_rows(&items, false, ToolDensity::Standard, &d), vec![Row::Item(0), Row::Item(1)]);
    }

    #[test]
    fn 过程中夹着提示不折() {
        let notice = TimelineItem::Notice { id: "n".into(), level: loom_core::NoticeLevel::Info, text: "x".into() };
        let items = vec![user("u1"), tool("1", ToolKind::Read, ToolStatus::Completed), notice, tool("2", ToolKind::Read, ToolStatus::Completed), text("答")];
        let d: HashMap<String, u64> = [("u1".to_string(), 5)].into();
        assert!(!build_rows(&items, false, ToolDensity::Standard, &d).iter().any(|r| matches!(r, Row::Digest(_))));
    }

    #[test]
    fn 条目到行的映射() {
        let items = vec![text("a"), tool("1", ToolKind::Read, ToolStatus::Completed), tool("2", ToolKind::Read, ToolStatus::Completed), text("b")];
        let rows = build_rows(&items, false, ToolDensity::Compact, &Default::default());
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
