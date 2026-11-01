//! 面包屑:最近的用户动作与系统事件放在固定容量的环形缓冲里,崩溃与卡顿时一并写出,
//! 用来回答「出事前用户最后做了什么」。
//!
//! @author ixxxxoooo

use std::collections::VecDeque;
use std::sync::Mutex;

/// 缓冲容量。
pub const CAPACITY: usize = 100;

static CRUMBS: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

/// 记一条面包屑(自动加时间前缀)。
pub fn crumb(msg: impl AsRef<str>) {
    let line = format!("{} {}", crate::logger::timestamp(), msg.as_ref());
    push_line(&mut CRUMBS.lock().unwrap_or_else(|e| e.into_inner()), line);
}

fn push_line(q: &mut VecDeque<String>, line: String) {
    if q.len() >= CAPACITY {
        q.pop_front();
    }
    q.push_back(line);
}

/// 最近的面包屑(旧的在前)。崩溃路径用 `try_lock`,拿不到就返回空,绝不阻塞。
pub fn recent(n: usize) -> Vec<String> {
    let Ok(q) = CRUMBS.try_lock() else { return Vec::new() };
    q.iter().skip(q.len().saturating_sub(n)).cloned().collect()
}

/// 记录一条面包屑,支持格式化。
#[macro_export]
macro_rules! crumb {
    ($($a:tt)*) => { $crate::crumb(format!($($a)*)) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 超出容量丢弃最旧() {
        let mut q = VecDeque::new();
        for i in 0..(CAPACITY + 5) {
            push_line(&mut q, format!("n{i}"));
        }
        assert_eq!(q.len(), CAPACITY);
        assert_eq!(q.front().unwrap(), "n5");
        assert_eq!(q.back().unwrap(), &format!("n{}", CAPACITY + 4));
    }
}
