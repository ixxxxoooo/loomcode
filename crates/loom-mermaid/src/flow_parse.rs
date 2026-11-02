//! 流程图(graph / flowchart)解析。

use crate::RenderError;
use crate::text::{parse_color, split_lines};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    TB,
    BT,
    LR,
    RL,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Rect,
    Round,
    Stadium,
    Subroutine,
    Cylinder,
    Circle,
    Diamond,
    Hexagon,
    ParaR,
    ParaL,
    TrapU,
    TrapD,
    Flag,
    /// 状态图的起止点(实心圆)
    Dot,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineKind {
    Solid,
    Dotted,
    Thick,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Head {
    None,
    Arrow,
    Circle,
    Cross,
}

#[derive(Clone, Default, Debug)]
pub struct NodeStyle {
    pub fill: Option<u32>,
    pub stroke: Option<u32>,
    pub color: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub id: String,
    pub label: Vec<String>,
    pub shape: Shape,
    pub style: NodeStyle,
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    pub label: Option<Vec<String>>,
    pub line: LineKind,
    pub end: Head,
    pub start: Head,
}

#[derive(Clone, Debug)]
pub struct Sub {
    pub title: String,
    pub parent: Option<usize>,
    pub nodes: Vec<usize>,
}

#[derive(Debug)]
pub struct Flow {
    pub dir: Dir,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub subs: Vec<Sub>,
    pub index: HashMap<String, usize>,
}

impl Flow {
    pub fn new(dir: Dir) -> Self {
        Self { dir, nodes: vec![], edges: vec![], subs: vec![], index: HashMap::new() }
    }

    /// 按 id 取节点下标,不存在则新建(默认矩形,标签即 id)。
    pub fn ensure(&mut self, id: &str) -> usize {
        if let Some(&i) = self.index.get(id) {
            return i;
        }
        self.nodes.push(Node { id: id.into(), label: vec![id.into()], shape: Shape::Rect, style: NodeStyle::default() });
        self.index.insert(id.into(), self.nodes.len() - 1);
        self.nodes.len() - 1
    }
}

/// 一条连线的解析结果。
struct Link {
    line: LineKind,
    start: Head,
    end: Head,
    label: Option<String>,
}

fn head_of(c: char) -> Head {
    match c {
        '>' => Head::Arrow,
        'o' => Head::Circle,
        'x' => Head::Cross,
        _ => Head::None,
    }
}

/// 在 `s[i..]` 解析连线记号(`-->`、`-.->`、`==>`、`-- 文字 -->`、`-->|文字|` 等)。
fn parse_link(s: &[char], mut i: usize) -> Option<(Link, usize)> {
    let at = |k: usize| s.get(k).copied().unwrap_or('\0');
    let is_run = |c: char| matches!(c, '-' | '=' | '.');
    let boundary = |c: char| c == '\0' || c.is_whitespace() || c == '|';
    let mut start = Head::None;
    if at(i) == '<' {
        start = Head::Arrow;
        i += 1;
    } else if matches!(at(i), 'o' | 'x') && matches!(at(i + 1), '-' | '=') {
        start = head_of(at(i));
        i += 1;
    }
    let run_start = i;
    while is_run(at(i)) {
        i += 1;
    }
    let run: String = s[run_start..i].iter().collect();
    if run.len() < 2 {
        return None;
    }
    let line = if run.contains('=') { LineKind::Thick } else if run.contains('.') { LineKind::Dotted } else { LineKind::Solid };
    let mut label = None;
    let end;
    if at(i) == '>' {
        end = Head::Arrow;
        i += 1;
    } else if matches!(at(i), 'o' | 'x') && boundary(at(i + 1)) && run.len() >= 2 {
        end = head_of(at(i));
        i += 1;
    } else if at(i) == ' ' && run.len() == 2 {
        // `-- 文字 -->`:向后找结束记号
        let mut j = i;
        loop {
            while j < s.len() && !is_run(s[j]) {
                j += 1;
            }
            if j >= s.len() {
                return None;
            }
            let k0 = j;
            while is_run(at(j)) {
                j += 1;
            }
            if j - k0 >= 2 {
                label = Some(s[i..k0].iter().collect::<String>());
                end = if at(j) == '>' {
                    j += 1;
                    Head::Arrow
                } else if matches!(at(j), 'o' | 'x') && boundary(at(j + 1)) {
                    j += 1;
                    head_of(s[j - 1])
                } else {
                    Head::None
                };
                i = j;
                break;
            }
        }
    } else if run.len() >= 3 {
        end = Head::None;
    } else {
        return None;
    }
    // `-->|文字|`
    let mut k = i;
    while at(k) == ' ' {
        k += 1;
    }
    if at(k) == '|' && label.is_none() {
        let from = k + 1;
        let to = (from..s.len()).find(|&x| s[x] == '|')?;
        label = Some(s[from..to].iter().collect());
        i = to + 1;
    }
    Some((Link { line, start, end, label }, i))
}

/// 形状开闭符:(开符, 闭符候选, 形状)。
const SHAPES: &[(&str, &[&str], Shape)] = &[
    ("(((", &[")))"], Shape::Circle),
    ("((", &["))"], Shape::Circle),
    ("([", &["])"], Shape::Stadium),
    ("(", &[")"], Shape::Round),
    ("[[", &["]]"], Shape::Subroutine),
    ("[(", &[")]"], Shape::Cylinder),
    ("[/", &["/]"], Shape::ParaR),
    ("[/", &["\\]"], Shape::TrapU),
    ("[\\", &["\\]"], Shape::ParaL),
    ("[\\", &["/]"], Shape::TrapD),
    ("[", &["]"], Shape::Rect),
    ("{{", &["}}"], Shape::Hexagon),
    ("{", &["}"], Shape::Diamond),
    (">", &["]"], Shape::Flag),
];

fn starts(s: &[char], i: usize, pat: &str) -> bool {
    pat.chars().enumerate().all(|(k, c)| s.get(i + k) == Some(&c))
}

/// 解析一个节点(`id` + 可选形状标签 + 可选 `:::类`);返回节点下标与新位置。
fn parse_node(flow: &mut Flow, s: &[char], mut i: usize, classes: &mut Vec<(usize, String)>) -> Option<(usize, usize)> {
    let id_start = i;
    while i < s.len() && (s[i].is_alphanumeric() || s[i] == '_') {
        i += 1;
    }
    if i == id_start {
        return None;
    }
    let id: String = s[id_start..i].iter().collect();
    let mut shape_label: Option<(Shape, String)> = None;
    // 先试长开符;`[/` 与 `[\` 有两种闭符,逐个尝试能找到闭符的组合
    for (open, closes, shape) in SHAPES {
        if !starts(s, i, open) {
            continue;
        }
        let body = i + open.chars().count();
        let quoted = s.get(body) == Some(&'"');
        let mut search = body;
        if quoted && let Some(q) = (body + 1..s.len()).find(|&k| s[k] == '"') {
            search = q + 1;
        }
        let found = closes.iter().find_map(|c| (search..s.len()).find(|&k| starts(s, k, c)).map(|k| (k, c.chars().count())));
        if let Some((k, clen)) = found {
            shape_label = Some((*shape, s[body..k].iter().collect()));
            i = k + clen;
            break;
        }
    }
    let idx = flow.ensure(&id);
    if let Some((shape, label)) = shape_label {
        flow.nodes[idx].shape = shape;
        flow.nodes[idx].label = split_lines(&label);
    }
    if starts(s, i, ":::") {
        let st = i + 3;
        let mut e = st;
        while e < s.len() && (s[e].is_alphanumeric() || s[e] == '_' || s[e] == '-') {
            e += 1;
        }
        classes.push((idx, s[st..e].iter().collect()));
        i = e;
    }
    Some((idx, i))
}

/// 解析 `fill:#f9f,stroke:#333,color:#fff` 样式串。
fn parse_style(spec: &str) -> NodeStyle {
    let mut st = NodeStyle::default();
    for kv in spec.split(',') {
        if let Some((k, v)) = kv.split_once(':') {
            let c = parse_color(v);
            match k.trim() {
                "fill" => st.fill = c,
                "stroke" => st.stroke = c,
                "color" => st.color = c,
                _ => {}
            }
        }
    }
    st
}

/// 按语句拆分:换行与不在括号 / 引号里的分号。
pub fn statements(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in src.lines() {
        let (mut cur, mut depth, mut quote) = (String::new(), 0i32, false);
        for c in line.chars() {
            match c {
                '"' => quote = !quote,
                '[' | '(' | '{' if !quote => depth += 1,
                ']' | ')' | '}' if !quote => depth -= 1,
                ';' if !quote && depth <= 0 => {
                    out.push(std::mem::take(&mut cur));
                    continue;
                }
                _ => {}
            }
            cur.push(c);
        }
        out.push(cur);
    }
    out.into_iter().map(|l| l.trim().to_string()).filter(|l| !l.is_empty() && !l.starts_with("%%")).collect()
}

/// 解析流程图源码。
pub fn parse(src: &str) -> Result<Flow, RenderError> {
    let mut stmts = statements(src).into_iter();
    // 跳过前置元数据
    let mut head = stmts.next().ok_or(RenderError::Empty)?;
    if head == "---" {
        head = stmts.by_ref().skip_while(|l| l != "---").nth(1).ok_or(RenderError::Empty)?;
    }
    let mut parts = head.split_whitespace();
    parts.next();
    let dir = match parts.next().map(str::to_ascii_uppercase).as_deref() {
        Some("LR") => Dir::LR,
        Some("RL") => Dir::RL,
        Some("BT") => Dir::BT,
        _ => Dir::TB,
    };
    let mut flow = Flow::new(dir);
    let (mut stack, mut class_defs, mut classes): (Vec<usize>, HashMap<String, NodeStyle>, Vec<(usize, String)>) = (vec![], HashMap::new(), vec![]);
    for line in stmts {
        let lower = line.to_ascii_lowercase();
        if lower == "end" {
            stack.pop();
        } else if lower.starts_with("subgraph ") || lower == "subgraph" {
            let rest = line[8..].trim();
            let title = match (rest.find('['), rest.rfind(']')) {
                (Some(a), Some(b)) if b > a => rest[a + 1..b].trim().trim_matches('"').to_string(),
                _ => rest.trim_matches('"').to_string(),
            };
            flow.subs.push(Sub { title, parent: stack.last().copied(), nodes: vec![] });
            stack.push(flow.subs.len() - 1);
        } else if lower.starts_with("direction ") || lower.starts_with("linkstyle ") || lower.starts_with("click ") || lower.starts_with("accTitle") || lower.starts_with("accDescr") {
        } else if let Some(rest) = line.strip_prefix("classDef ") {
            let mut it = rest.trim().splitn(2, char::is_whitespace);
            if let (Some(name), Some(spec)) = (it.next(), it.next()) {
                for n in name.split(',') {
                    class_defs.insert(n.to_string(), parse_style(spec));
                }
            }
        } else if let Some(rest) = line.strip_prefix("class ") {
            let mut it = rest.trim().rsplitn(2, char::is_whitespace);
            if let (Some(name), Some(ids)) = (it.next(), it.next()) {
                for id in ids.split(',') {
                    let i = flow.ensure(id.trim());
                    classes.push((i, name.to_string()));
                }
            }
        } else if let Some(rest) = line.strip_prefix("style ") {
            let mut it = rest.trim().splitn(2, char::is_whitespace);
            if let (Some(id), Some(spec)) = (it.next(), it.next()) {
                let i = flow.ensure(id);
                flow.nodes[i].style = parse_style(spec);
            }
        } else {
            parse_chain(&mut flow, &line, &stack, &mut classes)?;
        }
        if flow.nodes.len() > crate::MAX_NODES {
            return Err(RenderError::TooLarge(crate::MAX_NODES));
        }
    }
    for (i, name) in classes {
        if let Some(st) = class_defs.get(&name) {
            flow.nodes[i].style = st.clone();
        }
    }
    Ok(flow)
}

/// 解析一条「节点 连线 节点 …」链。
fn parse_chain(flow: &mut Flow, line: &str, stack: &[usize], classes: &mut Vec<(usize, String)>) -> Result<(), RenderError> {
    let s: Vec<char> = line.chars().collect();
    let bad = || RenderError::Syntax(line.to_string());
    let mut i = 0;
    let mut prev: Option<(Vec<usize>, Link)> = None;
    loop {
        // 一组节点(`A & B`)
        let mut group = Vec::new();
        loop {
            while s.get(i) == Some(&' ') {
                i += 1;
            }
            let (idx, next) = parse_node(flow, &s, i, classes).ok_or_else(bad)?;
            group.push(idx);
            if let Some(&sub) = stack.last() {
                if !flow.subs.iter().any(|x| x.nodes.contains(&idx)) {
                    flow.subs[sub].nodes.push(idx);
                }
            }
            i = next;
            while s.get(i) == Some(&' ') {
                i += 1;
            }
            if s.get(i) == Some(&'&') {
                i += 1;
            } else {
                break;
            }
        }
        if let Some((pending, link)) = prev.take() {
            for &a in &pending {
                for &b in &group {
                    flow.edges.push(Edge { from: a, to: b, label: link.label.clone().map(|l| split_lines(&l)), line: link.line, end: link.end, start: link.start });
                }
            }
        }
        while s.get(i) == Some(&' ') {
            i += 1;
        }
        if i >= s.len() {
            return Ok(());
        }
        let (link, next) = parse_link(&s, i).ok_or_else(bad)?;
        i = next;
        prev = Some((group, link));
    }
}
