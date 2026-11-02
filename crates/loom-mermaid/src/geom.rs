//! 几何小工具:折线平滑、虚线切分、箭头、弧线。

pub type P = (f32, f32);

/// Chaikin 切角平滑,首尾点保持不动。
pub fn chaikin(pts: &[P], iters: usize) -> Vec<P> {
    let mut cur = pts.to_vec();
    for _ in 0..iters {
        if cur.len() < 3 {
            break;
        }
        let mut next = vec![cur[0]];
        for w in cur.windows(2) {
            let (a, b) = (w[0], w[1]);
            next.push((a.0 * 0.75 + b.0 * 0.25, a.1 * 0.75 + b.1 * 0.25));
            next.push((a.0 * 0.25 + b.0 * 0.75, a.1 * 0.25 + b.1 * 0.75));
        }
        next.push(*cur.last().unwrap());
        cur = next;
    }
    cur
}

/// 折线总长。
pub fn length(pts: &[P]) -> f32 {
    pts.windows(2).map(|w| dist(w[0], w[1])).sum()
}

pub fn dist(a: P, b: P) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// 折线上距起点 `d` 处的点。
pub fn point_at(pts: &[P], d: f32) -> P {
    let mut left = d;
    for w in pts.windows(2) {
        let l = dist(w[0], w[1]);
        if left <= l && l > 0. {
            let t = left / l;
            return (w[0].0 + (w[1].0 - w[0].0) * t, w[0].1 + (w[1].1 - w[0].1) * t);
        }
        left -= l;
    }
    *pts.last().unwrap_or(&(0., 0.))
}

/// 把折线终点缩短 `d`(让线条不穿出箭头)。
pub fn trim_end(pts: &[P], d: f32) -> Vec<P> {
    let total = length(pts);
    if total <= d * 1.5 || pts.len() < 2 {
        return pts.to_vec();
    }
    let keep = total - d;
    let mut out = vec![pts[0]];
    let mut acc = 0.;
    for w in pts.windows(2) {
        let l = dist(w[0], w[1]);
        if acc + l >= keep {
            out.push(point_at(&[w[0], w[1]], keep - acc));
            return out;
        }
        acc += l;
        out.push(w[1]);
    }
    out
}

/// 把折线切成若干段虚线。
pub fn dashes(pts: &[P], on: f32, off: f32) -> Vec<Vec<P>> {
    let total = length(pts);
    let mut out = Vec::new();
    let mut d = 0.;
    while d < total {
        let end = (d + on).min(total);
        let mut seg = vec![point_at(pts, d)];
        let mut acc = 0.;
        for w in pts.windows(2) {
            let l = dist(w[0], w[1]);
            if acc > d && acc < end {
                seg.push(w[0]);
            }
            acc += l;
        }
        seg.push(point_at(pts, end));
        out.push(seg);
        d += on + off;
    }
    out
}

/// 箭头三角形:尖端 `tip`,方向取自 `from → tip`。
pub fn arrow_head(tip: P, from: P, len: f32, half_w: f32) -> Vec<P> {
    let d = dist(tip, from).max(0.001);
    let (ux, uy) = ((tip.0 - from.0) / d, (tip.1 - from.1) / d);
    let base = (tip.0 - ux * len, tip.1 - uy * len);
    vec![tip, (base.0 - uy * half_w, base.1 + ux * half_w), (base.0 + uy * half_w, base.1 - ux * half_w)]
}

/// 椭圆(或圆弧)上的点,角度以弧度计,0 在正右方,顺时针(屏幕坐标)。
pub fn arc(cx: f32, cy: f32, rx: f32, ry: f32, a0: f32, a1: f32, steps: usize) -> Vec<P> {
    (0..=steps).map(|i| {
        let a = a0 + (a1 - a0) * i as f32 / steps as f32;
        (cx + rx * a.cos(), cy + ry * a.sin())
    }).collect()
}
