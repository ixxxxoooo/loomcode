//! 资源采样:每秒读一次本进程内存与 CPU,留最近 5 分钟的曲线,并按规则写日志。
//!
//! 日志规则(参考 Zed 的 reliability):每 30 秒最多一条;内存相对上次记录变化 ≥10% 且 ≥64 MB 才记;
//! 其余时候每 10 分钟记一次心跳。数据来自 macOS 的 `proc_pidinfo` 与 `getrusage`,不起子进程。
//!
//! @author ixxxxoooo

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

/// 曲线保留的采样点数(每秒一个,共 5 分钟)。
const KEEP: usize = 300;

/// 一个采样点。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Sample {
    /// 采样时刻(Unix 秒)。
    pub t: u64,
    /// 常驻内存(MB)。
    pub rss_mb: u64,
    /// 该秒内的 CPU 占用(%,100 = 一个核心满载)。
    pub cpu: f32,
}

static SAMPLES: Mutex<VecDeque<Sample>> = Mutex::new(VecDeque::new());

/// 最近的采样曲线(旧在前)。
pub fn samples() -> Vec<Sample> {
    SAMPLES.lock().map(|s| s.iter().copied().collect()).unwrap_or_default()
}

/// 写日志的节流判定(纯函数状态机,便于测试)。
#[derive(Debug, Default)]
pub struct LogGate {
    last_log: Option<u64>,
    last_rss: u64,
}

impl LogGate {
    /// 现在是否应该写一条内存日志;返回原因(`None` 为不写)。
    pub fn check(&mut self, now: u64, rss_mb: u64) -> Option<&'static str> {
        let Some(last) = self.last_log else {
            self.last_log = Some(now);
            self.last_rss = rss_mb;
            return Some("首次采样");
        };
        let since = now.saturating_sub(last);
        let delta = rss_mb.abs_diff(self.last_rss);
        let big = delta >= 64 && delta * 10 >= self.last_rss;
        let why = if since >= 30 && big {
            Some("内存明显变化")
        } else if since >= 600 {
            Some("心跳")
        } else {
            None
        };
        if why.is_some() {
            self.last_log = Some(now);
            self.last_rss = rss_mb;
        }
        why
    }
}

/// 本进程当前常驻内存(MB)。非 macOS 返回 0。
pub fn read_rss_mb() -> u64 {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: 读取自身的 proc_taskinfo,缓冲区大小与结构体一致。
        unsafe {
            let mut info: libc::proc_taskinfo = std::mem::zeroed();
            let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
            let n = libc::proc_pidinfo(libc::getpid(), libc::PROC_PIDTASKINFO, 0, &mut info as *mut _ as *mut libc::c_void, size);
            if n == size { info.pti_resident_size / (1024 * 1024) } else { 0 }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        0
    }
}

/// 本进程累计 CPU 时间(用户 + 系统,秒)。
pub fn read_cpu_secs() -> f64 {
    // SAFETY: getrusage 只写入我们提供的结构体。
    unsafe {
        let mut ru: libc::rusage = std::mem::zeroed();
        if libc::getrusage(libc::RUSAGE_SELF, &mut ru) != 0 {
            return 0.;
        }
        let tv = |t: libc::timeval| t.tv_sec as f64 + t.tv_usec as f64 / 1e6;
        tv(ru.ru_utime) + tv(ru.ru_stime)
    }
}

/// 启动采样线程。
pub fn start() {
    let _ = std::thread::Builder::new().name("loom-perf".into()).spawn(|| {
        let mut gate = LogGate::default();
        let mut last_cpu = read_cpu_secs();
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let (rss, cpu_now) = (read_rss_mb(), read_cpu_secs());
            let cpu = ((cpu_now - last_cpu) * 100.).max(0.) as f32;
            last_cpu = cpu_now;
            crate::stats::set_rss_mb(rss);
            let t = crate::stats::now_secs();
            if let Ok(mut s) = SAMPLES.lock() {
                if s.len() >= KEEP {
                    s.pop_front();
                }
                s.push_back(Sample { t, rss_mb: rss, cpu });
            }
            if let Some(why) = gate.check(t, rss) {
                let g = crate::stats::gauges().iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(" ");
                crate::info!("资源({why}):内存 {rss} MB,CPU {cpu:.0}% {g}");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 日志节流规则() {
        let mut g = LogGate::default();
        assert_eq!(g.check(0, 100), Some("首次采样"));
        assert_eq!(g.check(10, 500), None); // 变化大但不足 30 秒
        assert_eq!(g.check(31, 500), Some("内存明显变化"));
        assert_eq!(g.check(100, 520), None); // 变化太小
        assert_eq!(g.check(700, 520), Some("心跳"));
    }

    #[test]
    fn 能读到自身内存与cpu() {
        #[cfg(target_os = "macos")]
        assert!(read_rss_mb() > 0);
        assert!(read_cpu_secs() >= 0.);
    }
}
