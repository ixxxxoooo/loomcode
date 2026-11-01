//! 慢操作计时:超过阈值就写一条警告日志。
//!
//! @author ixxxxoooo

use std::time::{Duration, Instant};

/// 计时守卫:离开作用域时,耗时超过阈值就记 WARN。
pub struct Span {
    name: &'static str,
    start: Instant,
    warn_at: Duration,
}

/// 开始计时。
///
/// 参数:`name` 操作名;`warn_ms` 超过多少毫秒记警告(界面相关 16,IO 类 100)。
pub fn span(name: &'static str, warn_ms: u64) -> Span {
    Span { name, start: Instant::now(), warn_at: Duration::from_millis(warn_ms) }
}

impl Span {
    /// 已用时间。
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        let e = self.start.elapsed();
        if e >= self.warn_at {
            crate::warn!("慢操作 {}:{} ms(阈值 {} ms)", self.name, e.as_millis(), self.warn_at.as_millis());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 计时有读数() {
        let s = span("测试", 10_000);
        std::thread::sleep(Duration::from_millis(5));
        assert!(s.elapsed() >= Duration::from_millis(5));
    }
}
