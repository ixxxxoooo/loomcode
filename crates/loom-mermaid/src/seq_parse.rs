//! 时序图解析。

use crate::RenderError;
use crate::flow_parse::statements;
use crate::text::split_lines;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tip {
    /// 实心三角(`>>`)
    Arrow,
    /// 无箭头(`->`、`-->`)
    None,
    /// 叉(`-x`)
    Cross,
    /// 异步开口箭头(`-)`)
    Async,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoteAt {
    /// 跨越 a..=b
    Over(usize, usize),
    Left(usize),
    Right(usize),
}

#[derive(Clone, Debug)]
pub enum Ev {
    Msg { from: usize, to: usize, text: Vec<String>, dotted: bool, tip: Tip, activate: bool, deactivate: bool },
    Note { at: NoteAt, text: Vec<String> },
    Activate(usize),
    Deactivate(usize),
    /// 开始一个框(loop / alt / opt / par …)
    Open { kind: String, label: String },
    Else { label: String },
    Close,
}

#[derive(Clone, Debug)]
pub struct Part {
    pub id: String,
    pub label: Vec<String>,
    pub actor: bool,
}

#[derive(Debug, Default)]
pub struct Seq {
    pub parts: Vec<Part>,
    pub evs: Vec<Ev>,
    pub title: Option<String>,
    pub autonumber: bool,
}

impl Seq {
    fn ensure(&mut self, id: &str) -> usize {
        let id = id.trim();
        if let Some(i) = self.parts.iter().position(|p| p.id == id) {
            return i;
        }
        self.parts.push(Part { id: id.into(), label: vec![id.into()], actor: false });
        self.parts.len() - 1
    }
}

const OPS: &[(&str, bool, Tip)] = &[
    ("-->>", true, Tip::Arrow),
    ("->>", false, Tip::Arrow),
    ("--x", true, Tip::Cross),
    ("-x", false, Tip::Cross),
    ("--)", true, Tip::Async),
    ("-)", false, Tip::Async),
    ("-->", true, Tip::None),
    ("->", false, Tip::None),
];

const BLOCKS: &[&str] = &["loop", "alt", "opt", "par", "critical", "break", "rect", "box"];

pub fn parse(src: &str) -> Result<Seq, RenderError> {
    let mut seq = Seq::default();
    let mut stmts = statements(src).into_iter();
    stmts.next().ok_or(RenderError::Empty)?;
    let mut blocks: Vec<bool> = Vec::new();
    for line in stmts {
        let (word, rest) = line.split_once(char::is_whitespace).map_or((line.as_str(), ""), |(a, b)| (a, b.trim()));
        let lw = word.to_ascii_lowercase();
        match lw.as_str() {
            "participant" | "actor" => {
                let (id, label) = rest.split_once(" as ").map_or((rest, rest), |(a, b)| (a, b));
                let i = seq.ensure(id);
                seq.parts[i].label = split_lines(label);
                seq.parts[i].actor = lw == "actor";
            }
            "autonumber" => seq.autonumber = true,
            "title" => seq.title = Some(rest.trim_start_matches(':').trim().to_string()),
            "activate" | "deactivate" => {
                let i = seq.ensure(rest);
                seq.evs.push(if lw == "activate" { Ev::Activate(i) } else { Ev::Deactivate(i) });
            }
            "else" | "and" | "option" => seq.evs.push(Ev::Else { label: rest.to_string() }),
            "end" => {
                if blocks.pop().unwrap_or(true) {
                    seq.evs.push(Ev::Close);
                }
            }
            k if BLOCKS.contains(&k) => {
                let draw = k != "box" && k != "rect";
                blocks.push(draw);
                if draw {
                    seq.evs.push(Ev::Open { kind: k.to_string(), label: rest.to_string() });
                }
            }
            "note" => {
                let (head, text) = rest.split_once(':').ok_or_else(|| RenderError::Syntax(line.clone()))?;
                let lh = head.to_ascii_lowercase();
                let at = if let Some(names) = lh.strip_prefix("over ") {
                    let mut it = head[head.len() - names.len()..].split(',');
                    let a = seq.ensure(it.next().unwrap_or(""));
                    let b = it.next().map_or(a, |n| seq.ensure(n));
                    NoteAt::Over(a.min(b), a.max(b))
                } else if lh.starts_with("left of ") {
                    NoteAt::Left(seq.ensure(&head[8..]))
                } else if lh.starts_with("right of ") {
                    NoteAt::Right(seq.ensure(&head[9..]))
                } else {
                    return Err(RenderError::Syntax(line));
                };
                seq.evs.push(Ev::Note { at, text: split_lines(text) });
            }
            "create" | "destroy" | "link" | "links" | "properties" | "details" => {
                if let Some(r) = rest.strip_prefix("participant ").or_else(|| rest.strip_prefix("actor ")) {
                    let (id, label) = r.split_once(" as ").map_or((r, r), |(a, b)| (a, b));
                    let i = seq.ensure(id);
                    seq.parts[i].label = split_lines(label);
                }
            }
            _ => message(&mut seq, &line)?,
        }
    }
    if seq.parts.is_empty() {
        return Err(RenderError::Empty);
    }
    Ok(seq)
}

fn message(seq: &mut Seq, line: &str) -> Result<(), RenderError> {
    let bad = || RenderError::Syntax(line.to_string());
    let (left, text) = line.split_once(':').map_or((line, ""), |(a, b)| (a, b));
    let chars: Vec<(usize, char)> = left.char_indices().collect();
    let (pos, op) = chars.iter().find_map(|&(i, c)| (c == '-').then(|| OPS.iter().find(|o| left[i..].starts_with(o.0)).map(|o| (i, o))).flatten()).ok_or_else(bad)?;
    let from = left[..pos].trim();
    let mut to = left[pos + op.0.len()..].trim();
    let (mut activate, mut deactivate) = (false, false);
    if let Some(t) = to.strip_prefix('+') {
        activate = true;
        to = t.trim();
    } else if let Some(t) = to.strip_prefix('-') {
        deactivate = true;
        to = t.trim();
    }
    if from.is_empty() || to.is_empty() {
        return Err(bad());
    }
    let (a, b) = (seq.ensure(from), seq.ensure(to));
    seq.evs.push(Ev::Msg { from: a, to: b, text: split_lines(text), dotted: op.1, tip: op.2, activate, deactivate });
    Ok(())
}
