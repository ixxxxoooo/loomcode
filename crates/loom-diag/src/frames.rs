//! 界面构建耗时统计:记录最近 120 次根视图 `render` 的耗时,给出 p50 / p95 / 最大值。
//!
//! 注意这是「构建元素树」的耗时,不含 GPU 绘制;足以发现 render 里的重活。
//!
//! @author ixxxxoooo

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

const KEEP: usize = 120;

static FRAMES: Mutex<VecDeque<u32>> = Mutex::new(VecDeque::new());

/// 启动以来的累计构建次数(用来观察是否有多余的重绘)。
static TOTAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 启动以来根视图的累计构建次数。
pub fn total() -> u64 {
    TOTAL.load(std::sync::atomic::Ordering::Relaxed)
}

/// 汇总结果(单位微秒)。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameSummary {
    pub count: usize,
    pub p50_us: u32,
    pub p95_us: u32,
    pub max_us: u32,
}

/// 记录一次构建耗时;超过 50 ms 额外记一条 WARN。
pub fn record(d: Duration) {
    TOTAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let us = d.as_micros().min(u32::MAX as u128) as u32;
    if d >= Duration::from_millis(50) {
        crate::warn!("界面构建很慢:{} ms", d.as_millis());
    }
    if let Ok(mut f) = FRAMES.try_lock() {
        if f.len() >= KEEP {
            f.pop_front();
        }
        f.push_back(us);
    }
}

/// 对一组耗时做汇总。
pub fn summarize(values: &[u32]) -> FrameSummary {
    if values.is_empty() {
        return FrameSummary::default();
    }
    let mut v = values.to_vec();
    v.sort_unstable();
    let at = |q: f64| v[(((v.len() - 1) as f64) * q).round() as usize];
    FrameSummary { count: v.len(), p50_us: at(0.5), p95_us: at(0.95), max_us: *v.last().unwrap() }
}

/// 最近 120 次的汇总。
pub fn summary() -> FrameSummary {
    let v: Vec<u32> = FRAMES.lock().map(|f| f.iter().copied().collect()).unwrap_or_default();
    summarize(&v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 汇总分位数() {
        assert_eq!(summarize(&[]), FrameSummary::default());
        let v: Vec<u32> = (1..=100).collect();
        let s = summarize(&v);
        assert_eq!(s.count, 100);
        assert_eq!(s.max_us, 100);
        assert!((49..=51).contains(&s.p50_us));
        assert!((94..=96).contains(&s.p95_us));
    }
}
