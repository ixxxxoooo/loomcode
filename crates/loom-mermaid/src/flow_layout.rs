//! 分层布局(Sugiyama 简化版):破环 → 最长路径分层 → 虚拟节点 → 重心法减少交叉 → 保序等距回归定位 → 连线走线。
//! 内部一律按「自上而下」排,最后按方向翻转 / 转置。

use crate::flow_parse::Dir;
use crate::geom::{P, chaikin};

const NODE_SEP: f32 = 36.;
const DUMMY_SEP: f32 = 14.;
const RANK_SEP: f32 = 52.;
const LABEL_EXTRA: f32 = 22.;

/// 布局输入的一条边。
pub struct EdgeIn {
    pub from: usize,
    pub to: usize,
    pub has_label: bool,
}

/// 布局结果(真实坐标:节点中心与每条边的折线,自环的折线为空)。
pub struct Layout {
    pub centers: Vec<P>,
    pub routes: Vec<Vec<P>>,
}

/// 保序等距回归:求使 `sum (x_i - z_i)^2` 最小且 `x` 非降的解。
fn isotonic(z: &[f32]) -> Vec<f32> {
    let mut blocks: Vec<(f32, usize)> = Vec::new();
    for &v in z {
        blocks.push((v, 1));
        while blocks.len() > 1 {
            let (b, a) = (blocks[blocks.len() - 1], blocks[blocks.len() - 2]);
            if a.0 <= b.0 {
                break;
            }
            let n = a.1 + b.1;
            blocks.truncate(blocks.len() - 2);
            blocks.push(((a.0 * a.1 as f32 + b.0 * b.1 as f32) / n as f32, n));
        }
    }
    blocks.iter().flat_map(|&(m, n)| std::iter::repeat_n(m, n)).collect()
}

fn crossings(a: &[usize], b: &[usize], down: &[Vec<usize>], pos: &[usize]) -> usize {
    let mut es: Vec<(usize, usize)> = Vec::new();
    for &u in a {
        for &v in &down[u] {
            if b.contains(&v) {
                es.push((pos[u], pos[v]));
            }
        }
    }
    let mut c = 0;
    for i in 0..es.len() {
        for j in i + 1..es.len() {
            if (es[i].0 < es[j].0 && es[i].1 > es[j].1) || (es[i].0 > es[j].0 && es[i].1 < es[j].1) {
                c += 1;
            }
        }
    }
    c
}

/// 排版。
///
/// 参数:`dir` 方向;`sizes` 每个节点的真实宽高;`edges` 边;`rectlike` 节点是否有平直的上下边(可错开端口);
/// `groups` 每个节点所属的顶层分组(`usize::MAX` 为无),同组节点在每层保持相邻。
pub fn layout(dir: Dir, sizes: &[(f32, f32)], edges: &[EdgeIn], rectlike: &[bool], groups: &[usize]) -> Layout {
    let n = sizes.len();
    let horiz = matches!(dir, Dir::LR | Dir::RL);
    // 布局空间:x 为同层展开方向,y 为层间方向
    let mut vw: Vec<f32> = sizes.iter().map(|&(w, h)| if horiz { h } else { w }).collect();
    let mut vh: Vec<f32> = sizes.iter().map(|&(w, h)| if horiz { w } else { h }).collect();
    let mut group: Vec<usize> = groups.to_vec();

    // 破环:DFS 遇到回边就反向
    let mut adj = vec![Vec::new(); n];
    for (k, e) in edges.iter().enumerate() {
        if e.from != e.to {
            adj[e.from].push((e.to, k));
        }
    }
    let mut state = vec![0u8; n];
    let mut rev = vec![false; edges.len()];
    fn dfs(u: usize, adj: &[Vec<(usize, usize)>], state: &mut [u8], rev: &mut [bool]) {
        state[u] = 1;
        for &(v, k) in &adj[u] {
            match state[v] {
                0 => dfs(v, adj, state, rev),
                1 => rev[k] = true,
                _ => {}
            }
        }
        state[u] = 2;
    }
    for u in 0..n {
        if state[u] == 0 {
            dfs(u, &adj, &mut state, &mut rev);
        }
    }
    let dag: Vec<(usize, usize, usize)> = edges.iter().enumerate().filter(|(_, e)| e.from != e.to).map(|(k, e)| if rev[k] { (e.to, e.from, k) } else { (e.from, e.to, k) }).collect();

    // 分层:最长路径
    let mut indeg = vec![0usize; n];
    let mut out = vec![Vec::new(); n];
    for &(u, v, _) in &dag {
        indeg[v] += 1;
        out[u].push(v);
    }
    let mut rank = vec![0usize; n];
    let mut queue: Vec<usize> = (0..n).filter(|&i| indeg[i] == 0).collect();
    let mut head = 0;
    while head < queue.len() {
        let u = queue[head];
        head += 1;
        for &v in &out[u] {
            rank[v] = rank[v].max(rank[u] + 1);
            indeg[v] -= 1;
            if indeg[v] == 0 {
                queue.push(v);
            }
        }
    }

    // 虚拟节点
    let mut vrank = rank.clone();
    let mut chains: Vec<(usize, Vec<usize>)> = Vec::new();
    for &(u, v, k) in &dag {
        let mut chain = vec![u];
        for r in rank[u] + 1..rank[v] {
            vrank.push(r);
            vw.push(0.);
            vh.push(0.);
            group.push(usize::MAX);
            chain.push(vrank.len() - 1);
        }
        chain.push(v);
        chains.push((k, chain));
    }
    let total = vrank.len();
    let depth = vrank.iter().copied().max().map_or(0, |m| m + 1);
    let (mut up, mut down) = (vec![Vec::new(); total], vec![Vec::new(); total]);
    for (_, chain) in &chains {
        for w in chain.windows(2) {
            down[w[0]].push(w[1]);
            up[w[1]].push(w[0]);
        }
    }
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); depth];
    for v in 0..total {
        layers[vrank[v]].push(v);
    }
    let mut pos = vec![0usize; total];
    let index = |layers: &[Vec<usize>], pos: &mut [usize]| layers.iter().for_each(|l| l.iter().enumerate().for_each(|(i, &v)| pos[v] = i));
    index(&layers, &mut pos);

    // 重心法
    let score = |layers: &[Vec<usize>], pos: &[usize]| (0..depth.saturating_sub(1)).map(|l| crossings(&layers[l], &layers[l + 1], &down, pos)).sum::<usize>();
    let mut best = (score(&layers, &pos), layers.clone());
    for round in 0..10 {
        let order: Vec<usize> = if round % 2 == 0 { (1..depth).collect() } else { (0..depth.saturating_sub(1)).rev().collect() };
        for l in order {
            let nb = if round % 2 == 0 { &up } else { &down };
            let mut keyed: Vec<(f32, usize)> = layers[l].iter().map(|&v| (if nb[v].is_empty() { pos[v] as f32 } else { nb[v].iter().map(|&u| pos[u] as f32).sum::<f32>() / nb[v].len() as f32 }, v)).collect();
            keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut ord: Vec<usize> = keyed.into_iter().map(|(_, v)| v).collect();
            // 同组节点相邻:按组平均位置排,组内保持顺序
            let gkey = |v: usize, ord: &[usize]| -> f32 {
                if group[v] == usize::MAX { return ord.iter().position(|&x| x == v).unwrap() as f32; }
                let m: Vec<usize> = ord.iter().enumerate().filter(|(_, x)| group[**x] == group[v]).map(|(i, _)| i).collect();
                m.iter().sum::<usize>() as f32 / m.len() as f32
            };
            let keys: Vec<(f32, usize)> = ord.iter().enumerate().map(|(i, &v)| (gkey(v, &ord) + i as f32 * 1e-3, v)).collect();
            let mut keys = keys;
            keys.sort_by(|a, b| a.0.total_cmp(&b.0));
            ord = keys.into_iter().map(|(_, v)| v).collect();
            layers[l] = ord;
            layers[l].iter().enumerate().for_each(|(i, &v)| pos[v] = i);
        }
        let s = score(&layers, &pos);
        if s < best.0 {
            best = (s, layers.clone());
        }
    }
    layers = best.1;
    index(&layers, &mut pos);

    // 横向坐标:先紧凑排列,再用保序回归向邻居靠拢
    let sep = |a: usize, b: usize| (vw[a] + vw[b]) / 2. + if vw[a] == 0. || vw[b] == 0. { DUMMY_SEP } else { NODE_SEP };
    let mut x = vec![0f32; total];
    for l in &layers {
        let mut cur = 0.;
        for (i, &v) in l.iter().enumerate() {
            if i > 0 {
                cur += sep(l[i - 1], v);
            }
            x[v] = cur;
        }
    }
    for round in 0..8 {
        let order: Vec<usize> = if round % 2 == 0 { (0..depth).collect() } else { (0..depth).rev().collect() };
        for l in order {
            let nb = if round % 2 == 0 { &up } else { &down };
            let layer = &layers[l];
            let mut offs = vec![0f32; layer.len()];
            for i in 1..layer.len() {
                offs[i] = offs[i - 1] + sep(layer[i - 1], layer[i]);
            }
            let z: Vec<f32> = layer.iter().enumerate().map(|(i, &v)| if nb[v].is_empty() { x[v] } else { nb[v].iter().map(|&u| x[u]).sum::<f32>() / nb[v].len() as f32 } - offs[i]).collect();
            for (i, m) in isotonic(&z).into_iter().enumerate() {
                x[layer[i]] = m + offs[i];
            }
        }
    }

    // 纵向坐标
    let mut labeled_gap = vec![false; depth];
    for (k, chain) in &chains {
        if edges[*k].has_label {
            let mid = (chain.len() - 1) / 2;
            labeled_gap[vrank[chain[mid]]] = true;
        }
    }
    let layer_h: Vec<f32> = layers.iter().map(|l| l.iter().map(|&v| vh[v]).fold(0., f32::max)).collect();
    let mut yc = vec![0f32; depth];
    let mut y = 0.;
    for l in 0..depth {
        yc[l] = y + layer_h[l] / 2.;
        y += layer_h[l] + RANK_SEP + if labeled_gap[l] { LABEL_EXTRA } else { 0. };
    }
    let total_h = (y - RANK_SEP).max(0.);

    // 端口:同一节点的多条出 / 入边在边上错开
    let mut out_ports: Vec<Vec<(f32, usize)>> = vec![Vec::new(); n];
    let mut in_ports: Vec<Vec<(f32, usize)>> = vec![Vec::new(); n];
    for (ci, (_, chain)) in chains.iter().enumerate() {
        out_ports[chain[0]].push((x[chain[1]], ci));
        in_ports[*chain.last().unwrap()].push((x[chain[chain.len() - 2]], ci));
    }
    let offset = |ports: &mut Vec<(f32, usize)>, node: usize, ci: usize| -> f32 {
        ports.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let k = ports.len();
        let i = ports.iter().position(|p| p.1 == ci).unwrap();
        if k < 2 || !rectlike[node] {
            return 0.;
        }
        let spread = (vw[node] * 0.7).min(16. * (k - 1) as f32);
        -spread / 2. + spread * i as f32 / (k - 1) as f32
    };

    let mut routes = vec![Vec::new(); edges.len()];
    for (ci, (k, chain)) in chains.iter().enumerate() {
        let (a, b) = (chain[0], *chain.last().unwrap());
        let mut pts: Vec<P> = vec![(x[a] + offset(&mut out_ports[a], a, ci), yc[vrank[a]] + vh[a] / 2.)];
        pts.extend(chain[1..chain.len() - 1].iter().map(|&d| (x[d], yc[vrank[d]])));
        pts.push((x[b] + offset(&mut in_ports[b], b, ci), yc[vrank[b]] - vh[b] / 2.));
        let mut skel = vec![pts[0]];
        for w in pts.windows(2) {
            if (w[0].0 - w[1].0).abs() > 1. {
                let mid = (w[0].1 + w[1].1) / 2.;
                skel.push((w[0].0, mid));
                skel.push((w[1].0, mid));
            }
            skel.push(w[1]);
        }
        let mut smooth = chaikin(&skel, 3);
        if rev[*k] {
            smooth.reverse();
        }
        routes[*k] = smooth.into_iter().map(|p| to_real(dir, p, total_h)).collect();
    }
    let centers = (0..n).map(|v| to_real(dir, (x[v], yc[vrank[v]]), total_h)).collect();
    Layout { centers, routes }
}

/// 布局空间坐标 → 真实坐标。
fn to_real(dir: Dir, (x, y): P, total_h: f32) -> P {
    match dir {
        Dir::TB => (x, y),
        Dir::BT => (x, total_h - y),
        Dir::LR => (y, x),
        Dir::RL => (total_h - y, x),
    }
}
