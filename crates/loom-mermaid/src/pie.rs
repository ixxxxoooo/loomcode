//! 饼图。

use crate::geom::arc;
use crate::text::split_lines;
use crate::{Anchor, Item, Paint, RenderError, Role, Scene, text_w};
use crate::flow_parse::statements;

const R: f32 = 110.;

pub fn render(src: &str) -> Result<Scene, RenderError> {
    let mut stmts = statements(src).into_iter();
    let head = stmts.next().ok_or(RenderError::Empty)?;
    let mut title = None;
    let show_data = head.contains("showData");
    if let Some(i) = head.find("title ") {
        title = Some(head[i + 6..].trim().to_string());
    }
    let mut data: Vec<(String, f64)> = Vec::new();
    for line in stmts {
        if let Some(t) = line.strip_prefix("title ") {
            title = Some(t.trim().to_string());
            continue;
        }
        let (label, value) = line.rsplit_once(':').ok_or_else(|| RenderError::Syntax(line.clone()))?;
        let v: f64 = value.trim().parse().map_err(|_| RenderError::Syntax(line.clone()))?;
        if v < 0. {
            return Err(RenderError::Syntax(line));
        }
        data.push((split_lines(label).join(" "), v));
    }
    let total: f64 = data.iter().map(|d| d.1).sum();
    if data.is_empty() || total <= 0. {
        return Err(RenderError::Empty);
    }
    // 与 Mermaid 一致:按数值从大到小排
    data.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut items = Vec::new();
    let (cx, cy) = (R + 10., R + 40.);
    if let Some(t) = &title {
        items.push(Item::Text { x: cx, y: 12., text: t.clone(), size: 16., bold: true, anchor: Anchor::Middle, paint: Role::Text.into() });
    }
    let mut start = -std::f32::consts::FRAC_PI_2;
    for (i, (label, v)) in data.iter().enumerate() {
        let frac = (*v / total) as f32;
        let end = start + frac * std::f32::consts::TAU;
        let color = Paint::Role(Role::Accent(i as u8));
        let pts = if frac >= 0.9999 {
            arc(cx, cy, R, R, 0., std::f32::consts::TAU, 72)
        } else {
            let mut p = vec![(cx, cy)];
            p.extend(arc(cx, cy, R, R, start, end, ((frac * 72.) as usize).max(2)));
            p
        };
        items.push(Item::Poly { pts, closed: true, fill: Some(color), stroke: Some(Role::Surface.into()), width: 2. });
        if frac >= 0.04 {
            let mid = (start + end) / 2.;
            items.push(Item::Text { x: cx + R * 0.68 * mid.cos(), y: cy + R * 0.68 * mid.sin(), text: format!("{:.0}%", frac * 100.), size: 12.5, bold: true, anchor: Anchor::Middle, paint: Paint::Rgb(0xffffff) });
        }
        // 图例
        let ly = 50. + i as f32 * 24.;
        let lx = cx + R + 40.;
        items.push(Item::Rect { x: lx, y: ly - 7., w: 14., h: 14., r: 3., fill: Some(color), stroke: None, width: 0. });
        let shown = if show_data { format!("{label} [{}]", fmt(*v)) } else { label.clone() };
        items.push(Item::Text { x: lx + 22., y: ly, text: shown.clone(), size: 13.5, bold: false, anchor: Anchor::Start, paint: Role::Text.into() });
        let _ = text_w(&shown, 13.5);
        start = end;
    }
    Ok(Scene { width: 0., height: 0., items })
}

fn fmt(v: f64) -> String {
    if v.fract() == 0. { format!("{v:.0}") } else { format!("{v}") }
}
