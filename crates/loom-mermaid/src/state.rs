//! 状态图:转换成流程图的节点与连线后复用同一套排版。

use crate::flow_parse::{Dir, Edge, Flow, Head, LineKind, Shape, statements};
use crate::text::split_lines;
use crate::{RenderError, Scene, flow_draw};

pub fn render(src: &str) -> Result<Scene, RenderError> {
    let mut stmts = statements(src).into_iter();
    stmts.next().ok_or(RenderError::Empty)?;
    let mut flow = Flow::new(Dir::TB);
    let mut stack: Vec<usize> = Vec::new();
    let (mut starts, mut ends) = (0usize, 0usize);
    // 取状态:`[*]` 在起点侧是起点圆点,在终点侧是终点圆点
    let state = |flow: &mut Flow, name: &str, is_target: bool, starts: &mut usize, ends: &mut usize| -> usize {
        let name = name.trim();
        if name == "[*]" {
            let id = if is_target {
                *ends += 1;
                format!("__end{}", *ends)
            } else {
                *starts += 1;
                format!("__start{}", *starts)
            };
            let i = flow.ensure(&id);
            flow.nodes[i].shape = Shape::Dot;
            flow.nodes[i].label = vec![String::new()];
            return i;
        }
        let i = flow.ensure(name);
        flow.nodes[i].shape = Shape::Round;
        i
    };
    let mut in_note = false;
    for line in stmts {
        if in_note {
            in_note = line.to_ascii_lowercase() != "end note";
            continue;
        }
        if line == "}" {
            stack.pop();
        } else if line.starts_with("direction ") {
            flow.dir = match line[10..].trim() {
                "LR" => Dir::LR,
                "RL" => Dir::RL,
                "BT" => Dir::BT,
                _ => Dir::TB,
            };
        } else if let Some(rest) = line.strip_prefix("state ") {
            // `state "说明" as id` / `state id {`(复合状态)/ `state id <<choice>>`
            let rest = rest.trim().trim_end_matches('{').trim();
            if let Some((desc, id)) = rest.split_once(" as ") {
                let i = state(&mut flow, id, false, &mut starts, &mut ends);
                flow.nodes[i].label = split_lines(desc);
            } else if rest.ends_with("<<choice>>") {
                let i = state(&mut flow, rest.trim_end_matches("<<choice>>"), false, &mut starts, &mut ends);
                flow.nodes[i].shape = Shape::Diamond;
                flow.nodes[i].label = vec![String::new()];
            } else if line.ends_with('{') {
                flow.subs.push(crate::flow_parse::Sub { title: rest.to_string(), parent: stack.last().copied(), nodes: vec![] });
                stack.push(flow.subs.len() - 1);
            } else {
                state(&mut flow, rest, false, &mut starts, &mut ends);
            }
        } else if let Some((l, r)) = line.split_once("-->") {
            let (target, label) = match r.split_once(':') {
                Some((t, lab)) => (t, Some(lab.trim().to_string())),
                None => (r, None),
            };
            let a = state(&mut flow, l, false, &mut starts, &mut ends);
            let b = state(&mut flow, target, true, &mut starts, &mut ends);
            for i in [a, b] {
                if let Some(&sub) = stack.last() && !flow.subs.iter().any(|s| s.nodes.contains(&i)) {
                    flow.subs[sub].nodes.push(i);
                }
            }
            flow.edges.push(Edge { from: a, to: b, label: label.map(|l| split_lines(&l)), line: LineKind::Solid, end: Head::Arrow, start: Head::None });
        } else if let Some((id, desc)) = line.split_once(':') {
            let i = state(&mut flow, id, false, &mut starts, &mut ends);
            flow.nodes[i].label = split_lines(desc);
        } else if line.starts_with("note ") {
            // 单行 `note … : 文字` 忽略;多行块到 `end note` 为止
            in_note = !line.contains(':');
        } else {
            return Err(RenderError::Syntax(line));
        }
    }
    if flow.nodes.is_empty() {
        return Err(RenderError::Empty);
    }
    Ok(flow_draw::draw(&flow))
}
