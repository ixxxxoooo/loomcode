//! 进程内共享的运行指标(原子量):采样线程写入,运行标记心跳、诊断页读取。
//!
//! @author ixxxxoooo

use std::sync::atomic::{AtomicU64, Ordering};

static RSS_MB: AtomicU64 = AtomicU64::new(0);

/// 记录最近一次采样的常驻内存(MB)。
pub fn set_rss_mb(v: u64) {
    RSS_MB.store(v, Ordering::Relaxed);
}

/// 最近一次采样的常驻内存(MB);还没采样过为 0。
pub fn rss_mb() -> u64 {
    RSS_MB.load(Ordering::Relaxed)
}

/// 当前 Unix 时间(秒)。
pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

use std::collections::BTreeMap;
use std::sync::Mutex;

static GAUGES: Mutex<BTreeMap<&'static str, u64>> = Mutex::new(BTreeMap::new());

/// 登记一个计数指标(如会话数、时间线条目数),诊断页展示。
pub fn set_gauge(name: &'static str, value: u64) {
    if let Ok(mut g) = GAUGES.lock() {
        g.insert(name, value);
    }
}

/// 读出所有计数指标(按名称排序)。
pub fn gauges() -> Vec<(&'static str, u64)> {
    GAUGES.lock().map(|g| g.iter().map(|(k, v)| (*k, *v)).collect()).unwrap_or_default()
}
