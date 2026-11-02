//! 流程图绘制:节点形状、连线、箭头、标签与子图框。

use crate::flow_layout::{EdgeIn, layout};
use crate::flow_parse::{Edge, Flow, Head, LineKind, Node, Shape};
use crate::geom::{self, P};
use crate::text::{contrast, lines_w};
use crate::{Anchor, Item, Paint, Role, Scene};

const FONT: f32 = 14.;
const LINE_H: f32 = 19.;

/// 节点的真实宽高。
fn node_size(n: &Node) -> (f32, f32) {
    let tw = lines_w(&n.label, FONT);
    let th = n.label.len() as f32 * LINE_H;
    let (w, h) = (tw + 28., th + 18.);
    match n.shape {
        Shape::Rect | Shape::Round => (w.max(44.), h.max(36.)),
        Shape::Stadium => ((w + 16.).max(60.), h.max(36.)),
        Shape::Subroutine => ((w + 18.).max(60.), h.max(36.)),
        Shape::Cylinder => (w.max(54.), (th + 32.).max(50.)),
        Shape::Circle => {
            let d = ((tw + 16.).max(th + 12.) * 1.15).max(48.);
            (d, d)
        }
        Shape::Diamond => ((tw * 1.7 + 36.).max(70.), (th * 1.8 + 20.).max(60.)),
        Shape::Hexagon => ((w + 24.).max(70.), h.max(40.)),
        Shape::ParaR | Shape::ParaL | Shape::TrapU | Shape::TrapD => ((w + 26.).max(70.), h.max(36.)),
        Shape::Flag => ((w + 16.).max(60.), h.max(36.)),
        Shape::Dot => (16., 16.),
    }
}

fn rect_like(s: Shape) -> bool {
    matches!(s, Shape::Rect | Shape::Round | Shape::Stadium | Shape::Subroutine | Shape::Cylinder | Shape::Hexagon)
}

/// 顶层分组:每个节点所属的最外层子图序号。
fn top_groups(flow: &Flow) -> Vec<usize> {
    let mut g = vec![usize::MAX; flow.nodes.len()];
    for (si, sub) in flow.subs.iter().enumerate() {
        let mut top = si;
        while let Some(p) = flow.subs[top].parent {
            top = p;
        }
        for &n in &sub.nodes {
            g[n] = top;
        }
    }
    g
}

/// 子图包含的全部节点(含嵌套子图)。
fn sub_nodes(flow: &Flow, si: usize) -> Vec<usize> {
    let mut v = flow.subs[si].nodes.clone();
    for (ci, c) in flow.subs.iter().enumerate() {
        if c.parent == Some(si) {
            v.extend(sub_nodes(flow, ci));
        }
    }
    v
}

fn sub_depth(flow: &Flow, si: usize) -> usize {
    flow.subs.iter().enumerate().filter(|(_, c)| c.parent == Some(si)).map(|(ci, _)| 1 + sub_depth(flow, ci)).max().unwrap_or(0)
}

/// 绘制整张流程图。
pub fn draw(flow: &Flow) -> Scene {
    let sizes: Vec<(f32, f32)> = flow.nodes.iter().map(node_size).collect();
    let rect: Vec<bool> = flow.nodes.iter().map(|n| rect_like(n.shape)).collect();
    let edges_in: Vec<EdgeIn> = flow.edges.iter().map(|e| EdgeIn { from: e.from, to: e.to, has_label: e.label.is_some() }).collect();
    let lay = layout(flow.dir, &sizes, &edges_in, &rect, &top_groups(flow));
    let mut items = Vec::new();

    // 子图框(外层先画)
    let mut order: Vec<usize> = (0..flow.subs.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(sub_depth(flow, i)));
    for si in order {
        let members = sub_nodes(flow, si);
        if members.is_empty() {
            continue;
        }
        let pad = 16. + 14. * sub_depth(flow, si) as f32;
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for &m in &members {
            let (c, s) = (lay.centers[m], sizes[m]);
            x0 = x0.min(c.0 - s.0 / 2.);
            x1 = x1.max(c.0 + s.0 / 2.);
            y0 = y0.min(c.1 - s.1 / 2.);
            y1 = y1.max(c.1 + s.1 / 2.);
        }
        let title = &flow.subs[si].title;
        let w = (x1 - x0 + pad * 2.).max(crate::text_w(title, 13.) + 24.);
        items.push(Item::Rect { x: (x0 + x1) / 2. - w / 2., y: y0 - pad - 20., w, h: y1 - y0 + pad * 2. + 20., r: 8., fill: Some(Role::Group.into()), stroke: Some(Role::Border.into()), width: 1. });
        items.push(Item::Text { x: (x0 + x1) / 2. - w / 2. + 12., y: y0 - pad - 8., text: title.clone(), size: 13., bold: true, anchor: Anchor::Start, paint: Role::Muted.into() });
    }

    // 连线本体
    let mut late: Vec<Item> = Vec::new();
    for (k, e) in flow.edges.iter().enumerate() {
        let route = if e.from == e.to { self_loop(lay.centers[e.from], sizes[e.from]) } else { lay.routes[k].clone() };
        draw_edge(e, &route, &mut items, &mut late);
    }
    for (i, n) in flow.nodes.iter().enumerate() {
        draw_node(n, lay.centers[i], sizes[i], &mut items);
    }
    items.extend(late);
    Scene { width: 0., height: 0., items }
}

fn self_loop(c: P, s: (f32, f32)) -> Vec<P> {
    let (x, y0, y1) = (c.0 + s.0 / 2., c.1 - s.1 / 4., c.1 + s.1 / 4.);
    geom::chaikin(&[(x, y0), (x + 24., y0), (x + 24., y1), (x, y1)], 2)
}

/// 画一条边:折线(虚线 / 粗线)放进 `items`,箭头与标签放进 `late`(在节点之上)。
fn draw_edge(e: &Edge, route: &[P], items: &mut Vec<Item>, late: &mut Vec<Item>) {
    if route.len() < 2 {
        return;
    }
    let width = if e.line == LineKind::Thick { 3. } else { 1.5 };
    let (head_len, half) = if e.line == LineKind::Thick { (11., 5.5) } else { (9., 4.5) };
    let mut body = route.to_vec();
    if matches!(e.end, Head::Arrow) {
        body = geom::trim_end(&body, head_len * 0.8);
    }
    if matches!(e.start, Head::Arrow) {
        body.reverse();
        body = geom::trim_end(&body, head_len * 0.8);
        body.reverse();
    }
    let line = |pts: Vec<P>| Item::Poly { pts, closed: false, fill: None, stroke: Some(Role::Line.into()), width };
    if e.line == LineKind::Dotted {
        items.extend(geom::dashes(&body, 4., 4.).into_iter().map(line));
    } else {
        items.push(line(body));
    }
    let mut marker = |tip: P, from: P, head: Head| match head {
        Head::None => {}
        Head::Arrow => late.push(Item::Poly { pts: geom::arrow_head(tip, from, head_len, half), closed: true, fill: Some(Role::Line.into()), stroke: Some(Role::Line.into()), width: 1. }),
        Head::Circle => {
            let d = geom::dist(tip, from).max(0.001);
            let c = (tip.0 + (from.0 - tip.0) / d * 5., tip.1 + (from.1 - tip.1) / d * 5.);
            late.push(Item::Poly { pts: geom::arc(c.0, c.1, 5., 5., 0., 6.2832, 16), closed: true, fill: Some(Role::Surface.into()), stroke: Some(Role::Line.into()), width: 1.5 });
        }
        Head::Cross => {
            let d = geom::dist(tip, from).max(0.001);
            let (ux, uy) = ((from.0 - tip.0) / d, (from.1 - tip.1) / d);
            let c = (tip.0 + ux * 6., tip.1 + uy * 6.);
            for s in [1., -1.] {
                late.push(Item::Poly { pts: vec![(c.0 - 4. * (ux - s * uy), c.1 - 4. * (uy + s * ux)), (c.0 + 4. * (ux - s * uy), c.1 + 4. * (uy + s * ux))], closed: false, fill: None, stroke: Some(Role::Line.into()), width: 1.8 });
            }
        }
    };
    let n = route.len();
    marker(route[n - 1], route[n - 2], e.end);
    marker(route[0], route[1], e.start);
    if let Some(label) = &e.label {
        let mid = geom::point_at(route, geom::length(route) / 2.);
        let (w, h) = (lines_w(label, 12.5) + 12., label.len() as f32 * 16. + 6.);
        late.push(Item::Rect { x: mid.0 - w / 2., y: mid.1 - h / 2., w, h, r: 4., fill: Some(Role::Surface.into()), stroke: None, width: 0. });
        text_lines(late, mid.0, mid.1, label, 12.5, 16., Role::Muted.into());
    }
}

/// 以 `(cx, cy)` 为中心画多行居中文字。
fn text_lines(items: &mut Vec<Item>, cx: f32, cy: f32, lines: &[String], size: f32, lh: f32, paint: Paint) {
    let top = cy - lh * lines.len() as f32 / 2. + lh / 2.;
    for (i, l) in lines.iter().enumerate() {
        if !l.is_empty() {
            items.push(Item::Text { x: cx, y: top + i as f32 * lh, text: l.clone(), size, bold: false, anchor: Anchor::Middle, paint });
        }
    }
}

fn draw_node(n: &Node, c: P, (w, h): (f32, f32), items: &mut Vec<Item>) {
    let fill: Paint = n.style.fill.map_or(Role::Node.into(), Paint::Rgb);
    let stroke: Paint = n.style.stroke.map_or(Role::Border.into(), Paint::Rgb);
    let text_paint = n.style.color.map(Paint::Rgb).unwrap_or_else(|| n.style.fill.map_or(Role::Text.into(), |f| Paint::Rgb(contrast(f))));
    let (x0, y0, x1, y1) = (c.0 - w / 2., c.1 - h / 2., c.0 + w / 2., c.1 + h / 2.);
    let poly = |pts: Vec<P>| Item::Poly { pts, closed: true, fill: Some(fill), stroke: Some(stroke), width: 1.3 };
    let k = 11.;
    match n.shape {
        Shape::Rect => items.push(Item::Rect { x: x0, y: y0, w, h, r: 3., fill: Some(fill), stroke: Some(stroke), width: 1.3 }),
        Shape::Round => items.push(Item::Rect { x: x0, y: y0, w, h, r: 10., fill: Some(fill), stroke: Some(stroke), width: 1.3 }),
        Shape::Stadium => items.push(Item::Rect { x: x0, y: y0, w, h, r: h / 2., fill: Some(fill), stroke: Some(stroke), width: 1.3 }),
        Shape::Subroutine => {
            items.push(Item::Rect { x: x0, y: y0, w, h, r: 3., fill: Some(fill), stroke: Some(stroke), width: 1.3 });
            for x in [x0 + 8., x1 - 8.] {
                items.push(Item::Poly { pts: vec![(x, y0), (x, y1)], closed: false, fill: None, stroke: Some(stroke), width: 1.3 });
            }
        }
        Shape::Circle => items.push(poly(geom::arc(c.0, c.1, w / 2., h / 2., 0., 6.2832, 40))),
        Shape::Dot => items.push(Item::Poly { pts: geom::arc(c.0, c.1, w / 2., h / 2., 0., 6.2832, 24), closed: true, fill: Some(Role::Line.into()), stroke: Some(Role::Line.into()), width: 1. }),
        Shape::Diamond => items.push(poly(vec![(c.0, y0), (x1, c.1), (c.0, y1), (x0, c.1)])),
        Shape::Hexagon => items.push(poly(vec![(x0 + k, y0), (x1 - k, y0), (x1, c.1), (x1 - k, y1), (x0 + k, y1), (x0, c.1)])),
        Shape::ParaR => items.push(poly(vec![(x0 + k, y0), (x1, y0), (x1 - k, y1), (x0, y1)])),
        Shape::ParaL => items.push(poly(vec![(x0, y0), (x1 - k, y0), (x1, y1), (x0 + k, y1)])),
        Shape::TrapU => items.push(poly(vec![(x0 + k, y0), (x1 - k, y0), (x1, y1), (x0, y1)])),
        Shape::TrapD => items.push(poly(vec![(x0, y0), (x1, y0), (x1 - k, y1), (x0 + k, y1)])),
        Shape::Flag => items.push(poly(vec![(x0 + k, y0), (x1, y0), (x1, y1), (x0 + k, y1), (x0, c.1)])),
        Shape::Cylinder => {
            let ry = 7.;
            let mut pts = geom::arc(c.0, y0 + ry, w / 2., ry, std::f32::consts::PI, 2. * std::f32::consts::PI, 20);
            pts.extend(geom::arc(c.0, y1 - ry, w / 2., ry, 0., std::f32::consts::PI, 20));
            items.push(poly(pts));
            items.push(Item::Poly { pts: geom::arc(c.0, y0 + ry, w / 2., ry, 0., std::f32::consts::PI, 20), closed: false, fill: None, stroke: Some(stroke), width: 1.3 });
        }
    }
    if n.shape != Shape::Dot {
        let off = if n.shape == Shape::Cylinder { 5. } else { 0. };
        text_lines(items, c.0, c.1 + off, &n.label, FONT, LINE_H, text_paint);
    }
}
