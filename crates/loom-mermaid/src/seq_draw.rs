//! 时序图排版与绘制。

use crate::geom::{self, P};
use crate::seq_parse::{Ev, NoteAt, Seq, Tip};
use crate::text::lines_w;
use crate::{Anchor, Item, Paint, Role, Scene, text_w};

const FONT: f32 = 13.5;
const LH: f32 = 17.;

fn text(items: &mut Vec<Item>, x: f32, y: f32, s: &str, size: f32, bold: bool, anchor: Anchor, paint: impl Into<Paint>) {
    if !s.is_empty() {
        items.push(Item::Text { x, y, text: s.into(), size, bold, anchor, paint: paint.into() });
    }
}

/// 多行文字块:`top` 为首行顶部,返回块高。
fn block(items: &mut Vec<Item>, x: f32, top: f32, lines: &[String], anchor: Anchor, paint: Paint) -> f32 {
    for (i, l) in lines.iter().enumerate() {
        text(items, x, top + LH / 2. + i as f32 * LH, l, FONT, false, anchor, paint);
    }
    lines.len() as f32 * LH
}

/// 计算各参与者的中心 x:按相邻间距与跨度较大的消息 / 便签逐步撑开。
fn columns(seq: &Seq, widths: &[f32]) -> Vec<f32> {
    let n = seq.parts.len();
    let mut need: Vec<f32> = (0..n.saturating_sub(1)).map(|k| (widths[k] + widths[k + 1]) / 2. + 30.).collect();
    let mut spans: Vec<(usize, usize, f32)> = Vec::new();
    for ev in &seq.evs {
        match ev {
            Ev::Msg { from, to, text, .. } => {
                let w = lines_w(text, FONT) + 40.;
                if from == to {
                    if *from + 1 < n { spans.push((*from, *from + 1, w + 50.)); }
                } else {
                    spans.push(((*from).min(*to), (*from).max(*to), w));
                }
            }
            Ev::Note { at, text } => {
                let w = lines_w(text, FONT) + 28.;
                match *at {
                    NoteAt::Over(a, b) if a != b => spans.push((a, b, w)),
                    NoteAt::Over(a, _) => {
                        if a > 0 { spans.push((a - 1, a, w / 2. + widths[a - 1] / 2. + 20.)); }
                        if a + 1 < n { spans.push((a, a + 1, w / 2. + widths[a + 1] / 2. + 20.)); }
                    }
                    NoteAt::Left(a) if a > 0 => spans.push((a - 1, a, w + 30.)),
                    NoteAt::Right(a) if a + 1 < n => spans.push((a, a + 1, w + 30.)),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    spans.sort_by_key(|s| s.1 - s.0);
    for (i, j, r) in spans {
        let cur: f32 = need[i..j].iter().sum();
        if cur < r {
            let add = (r - cur) / (j - i) as f32;
            need[i..j].iter_mut().for_each(|v| *v += add);
        }
    }
    let mut xs = vec![widths[0] / 2.];
    for k in 0..n.saturating_sub(1) {
        xs.push(xs[k] + need[k]);
    }
    xs
}

fn head(front: &mut Vec<Item>, tip: Tip, at: P, from: P) {
    match tip {
        Tip::None => {}
        Tip::Arrow => front.push(Item::Poly { pts: geom::arrow_head(at, from, 9., 4.5), closed: true, fill: Some(Role::Line.into()), stroke: Some(Role::Line.into()), width: 1. }),
        Tip::Async => {
            let h = geom::arrow_head(at, from, 9., 5.);
            front.push(Item::Poly { pts: vec![h[1], h[0], h[2]], closed: false, fill: None, stroke: Some(Role::Line.into()), width: 1.5 });
        }
        Tip::Cross => {
            for s in [1f32, -1.] {
                front.push(Item::Poly { pts: vec![(at.0 - 4., at.1 - 4. * s), (at.0 + 4., at.1 + 4. * s)], closed: false, fill: None, stroke: Some(Role::Line.into()), width: 1.8 });
            }
        }
    }
}

pub fn draw(seq: &Seq) -> Scene {
    let n = seq.parts.len();
    let widths: Vec<f32> = seq.parts.iter().map(|p| (lines_w(&p.label, FONT) + 30.).max(84.)).collect();
    let heights: Vec<f32> = seq.parts.iter().map(|p| p.label.len() as f32 * LH + 16.).collect();
    let box_h = heights.iter().copied().fold(0., f32::max).max(34.);
    let xs = columns(seq, &widths);
    let (xmin, xmax) = (xs[0], xs[n - 1]);
    let (mut back, mut front) = (Vec::new(), Vec::new());
    let top = if seq.title.is_some() { 34. } else { 0. };
    if let Some(t) = &seq.title {
        text(&mut front, (xmin + xmax) / 2., 10., t, 15., true, Anchor::Middle, Role::Text);
    }
    let mut y = top + box_h + 24.;
    let mut counter = 0;
    let mut frames: Vec<(f32, String, String, Vec<(f32, String)>)> = Vec::new();
    let mut acts: Vec<Vec<f32>> = vec![Vec::new(); n];
    let mut bars: Vec<(usize, usize, f32, f32)> = Vec::new();
    for ev in &seq.evs {
        match ev {
            Ev::Msg { from, to, text: lines, dotted, tip, activate, deactivate } => {
                let mut lines = lines.clone();
                if seq.autonumber {
                    counter += 1;
                    lines[0] = format!("{counter}. {}", lines[0]);
                }
                let (xa, xb) = (xs[*from], xs[*to]);
                let label_h = if lines.iter().all(String::is_empty) { 0. } else { lines.len() as f32 * LH };
                let ay = y + label_h + 8.;
                let (bar_a, bar_b) = (acts[*from].len() as f32 * 4., acts[*to].len() as f32 * 4. + if *activate { 4. } else { 0. });
                if from == to {
                    let x = xa + 5. + bar_a;
                    let pts = vec![(x, ay), (x + 34., ay), (x + 34., ay + 22.), (x, ay + 22.)];
                    push_line(&mut front, &pts[..3], *dotted);
                    front.push(Item::Poly { pts: vec![pts[2], pts[3]], closed: false, fill: None, stroke: Some(Role::Line.into()), width: 1.5 });
                    head(&mut front, *tip, pts[3], pts[2]);
                    block(&mut front, x + 42., ay - 4., &lines, Anchor::Start, Role::Text.into());
                    y = ay + 22. + 16.;
                } else {
                    let dir = if xb > xa { 1. } else { -1. };
                    let (sx, ex) = (xa + dir * (5. + bar_a), xb - dir * (5. + bar_b));
                    let cx = (sx + ex) / 2.;
                    block(&mut front, cx, y, &lines, Anchor::Middle, Role::Text.into());
                    let end_line = if matches!(tip, Tip::Arrow | Tip::Async) { ex - dir * 8. } else { ex };
                    push_line(&mut front, &[(sx, ay), (end_line, ay)], *dotted);
                    head(&mut front, *tip, (ex, ay), (sx, ay));
                    y = ay + 18.;
                }
                if *activate {
                    acts[*to].push(ay);
                }
                if *deactivate && let Some(s) = acts[*from].pop() {
                    bars.push((*from, acts[*from].len(), s, ay));
                }
            }
            Ev::Note { at, text: lines } => {
                let w = lines_w(lines, FONT) + 24.;
                let h = lines.len() as f32 * LH + 12.;
                let (w, x0) = match *at {
                    NoteAt::Over(a, b) => {
                        let w = w.max(xs[b] - xs[a] + 28.);
                        (w, (xs[a] + xs[b]) / 2. - w / 2.)
                    }
                    NoteAt::Left(a) => (w, xs[a] - 14. - w),
                    NoteAt::Right(a) => (w, xs[a] + 14.),
                };
                front.push(Item::Rect { x: x0, y, w, h, r: 3., fill: Some(Role::Note.into()), stroke: Some(Role::Border.into()), width: 1. });
                block(&mut front, x0 + w / 2., y + 6., lines, Anchor::Middle, Role::Text.into());
                y += h + 14.;
            }
            Ev::Activate(i) => acts[*i].push(y - 8.),
            Ev::Deactivate(i) => {
                if let Some(s) = acts[*i].pop() {
                    bars.push((*i, acts[*i].len(), s, y - 8.));
                }
            }
            Ev::Open { kind, label, .. } => {
                frames.push((y, kind.clone(), label.clone(), Vec::new()));
                y += 28.;
            }
            Ev::Else { label } => {
                if let Some(f) = frames.last_mut() {
                    f.3.push((y, label.clone()));
                }
                y += 24.;
            }
            Ev::Close => {
                if let Some((y0, kind, label, divs)) = frames.pop() {
                    let d = frames.len() as f32;
                    let (x0, x1) = (xmin - 34. + d * 8. - widths[0] / 2. + 40., xmax + 34. - d * 8. + widths[n - 1] / 2. - 40.);
                    back.push(Item::Rect { x: x0, y: y0, w: x1 - x0, h: y - y0, r: 3., fill: None, stroke: Some(Role::Border.into()), width: 1.2 });
                    let tab = text_w(&kind, 12.) + 18.;
                    back.push(Item::Rect { x: x0, y: y0, w: tab, h: 20., r: 3., fill: Some(Role::Group.into()), stroke: Some(Role::Border.into()), width: 1.2 });
                    text(&mut back, x0 + tab / 2., y0 + 10., &kind, 12., true, Anchor::Middle, Role::Text);
                    if !label.is_empty() {
                        text(&mut back, x0 + tab + 8., y0 + 10., &format!("[{label}]"), 12.5, false, Anchor::Start, Role::Muted);
                    }
                    for (dy, dl) in divs {
                        push_line(&mut back, &[(x0, dy), (x1, dy)], true);
                        if !dl.is_empty() {
                            text(&mut back, x0 + 10., dy + 11., &format!("[{dl}]"), 12.5, false, Anchor::Start, Role::Muted);
                        }
                    }
                    y += 12.;
                }
            }
        }
    }
    let end = y + 6.;
    // 未关闭的激活条补到末尾
    for (i, stack) in acts.iter().enumerate() {
        for (d, &s) in stack.iter().enumerate() {
            bars.push((i, d, s, end - 14.));
        }
    }
    let mut lifelines = Vec::new();
    for i in 0..n {
        push_line(&mut lifelines, &[(xs[i], top + box_h), (xs[i], end + 10.)], true);
    }
    let mut boxes = Vec::new();
    for (i, p) in seq.parts.iter().enumerate() {
        for by in [top, end + 10.] {
            let r = if p.actor { box_h / 2. } else { 5. };
            boxes.push(Item::Rect { x: xs[i] - widths[i] / 2., y: by, w: widths[i], h: box_h, r, fill: Some(Role::Node.into()), stroke: Some(Role::Border.into()), width: 1.3 });
            let t = by + box_h / 2. - p.label.len() as f32 * LH / 2.;
            block(&mut boxes, xs[i], t, &p.label, Anchor::Middle, Role::Text.into());
        }
    }
    let mut act_items = Vec::new();
    for (i, d, s, e) in bars {
        act_items.push(Item::Rect { x: xs[i] - 5. + d as f32 * 4., y: s, w: 10., h: (e - s).max(10.), r: 1., fill: Some(Role::Group.into()), stroke: Some(Role::Border.into()), width: 1.2 });
    }
    let mut items = lifelines;
    items.extend(back);
    items.extend(act_items);
    items.extend(boxes);
    items.extend(front);
    Scene { width: 0., height: 0., items }
}

fn push_line(items: &mut Vec<Item>, pts: &[P], dotted: bool) {
    if dotted {
        items.extend(geom::dashes(pts, 5., 4.).into_iter().map(|p| Item::Poly { pts: p, closed: false, fill: None, stroke: Some(Role::Line.into()), width: 1.3 }));
    } else {
        items.push(Item::Poly { pts: pts.to_vec(), closed: false, fill: None, stroke: Some(Role::Line.into()), width: 1.5 });
    }
}
