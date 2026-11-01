//! 主线程卡死检测:前台每 100 ms 打一次心跳,后台看门狗每秒检查心跳是否停滞。
//!
//! - 停滞超过警告阈值:写 WARN 日志(带最近面包屑);
//! - 停滞超过深度阈值:用 `/usr/bin/sample` 抓一份主线程调用栈到 `crashes/hang-*.txt`,并写一条 `hang` 记录;
//! - 恢复后补一条「卡顿 N 秒后恢复」。
//!
//! 判定逻辑是纯函数(`Watch::observe`),可以用注入的时钟测试。
//!
//! @author ixxxxoooo

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static START: OnceLock<Instant> = OnceLock::new();
/// 最近一次心跳距进程启动的毫秒数 + 1;0 表示还没开始心跳。
static LAST_BEAT: AtomicU64 = AtomicU64::new(0);
/// 最近一次卡顿的时长(毫秒)与发生时间(Unix 秒),诊断页展示。
static LAST_STALL_MS: AtomicU64 = AtomicU64::new(0);
static LAST_STALL_AT: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// 前台心跳:由主线程上的定时器每 100 ms 调用一次。
pub fn beat() {
    LAST_BEAT.store(now_ms() + 1, Ordering::Relaxed);
}

/// 最近一次卡顿:(时长毫秒, 发生的 Unix 秒);没有过为 `None`。
pub fn last_stall() -> Option<(u64, u64)> {
    let ms = LAST_STALL_MS.load(Ordering::Relaxed);
    (ms > 0).then(|| (ms, LAST_STALL_AT.load(Ordering::Relaxed)))
}

/// 看门狗的一次判定结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// 正常。
    Ok,
    /// 首次超过警告阈值(毫秒为当前停滞时长)。
    Warn(u64),
    /// 首次超过深度阈值,需要抓调用栈。
    Deep(u64),
    /// 之前卡住过,现在恢复了(毫秒为卡顿总时长)。
    Recovered(u64),
}

/// 判定状态机。
#[derive(Debug, Default)]
pub struct Watch {
    warned: bool,
    deep: bool,
    worst: u64,
    last_tick: Option<u64>,
}

impl Watch {
    /// 做一次判定。
    ///
    /// 参数:`now` 当前时刻(毫秒);`beat` 最近心跳时刻(毫秒,`None` 为还没开始);
    /// `warn` / `deep` 两级阈值(毫秒)。看门狗自己迟到超过 `3 × 1 s` 视为系统休眠,清空状态不报。
    pub fn observe(&mut self, now: u64, beat: Option<u64>, warn: u64, deep: u64) -> Verdict {
        let slept = self.last_tick.is_some_and(|t| now.saturating_sub(t) > 3_000);
        self.last_tick = Some(now);
        let Some(beat) = beat else { return Verdict::Ok };
        if slept {
            *self = Watch { last_tick: Some(now), ..Default::default() };
            return Verdict::Ok;
        }
        let gap = now.saturating_sub(beat);
        if gap >= deep && !self.deep {
            self.deep = true;
            self.warned = true;
            self.worst = gap;
            return Verdict::Deep(gap);
        }
        if gap >= warn {
            self.worst = self.worst.max(gap);
            if !self.warned {
                self.warned = true;
                return Verdict::Warn(gap);
            }
            return Verdict::Ok;
        }
        if self.warned {
            let total = self.worst.max(gap);
            self.warned = false;
            self.deep = false;
            self.worst = 0;
            return Verdict::Recovered(total);
        }
        Verdict::Ok
    }
}

/// 启动看门狗线程。`dev` 为开发版时阈值放宽(调试构建本来就慢)。
pub fn start(dev: bool) {
    let (warn, deep) = if dev { (5_000, 15_000) } else { (3_000, 10_000) };
    let _ = std::thread::Builder::new().name("loom-hang".into()).spawn(move || {
        let mut w = Watch::default();
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let b = LAST_BEAT.load(Ordering::Relaxed);
            let beat = (b > 0).then(|| b - 1);
            match w.observe(now_ms(), beat, warn, deep) {
                Verdict::Ok => {}
                Verdict::Warn(ms) => {
                    crate::warn!("主线程停滞 {ms} ms;最近操作:{}", crate::crumbs::recent(5).join(" | "));
                }
                Verdict::Deep(ms) => on_deep(ms),
                Verdict::Recovered(ms) => {
                    LAST_STALL_MS.store(ms, Ordering::Relaxed);
                    LAST_STALL_AT.store(crate::stats::now_secs(), Ordering::Relaxed);
                    crate::warn!("主线程卡顿 {ms} ms 后恢复");
                }
            }
        }
    });
}

/// 深度卡死:抓调用栈 + 写记录。
fn on_deep(ms: u64) {
    let pid = std::process::id();
    let file = crate::logger::dirs().map(|d| d.crashes.join(format!("hang-{}-{pid}.txt", crate::records::file_stamp())));
    let mut detail = format!("主线程停滞 {ms} ms 仍未恢复。");
    if let Some(f) = &file {
        // sample 会阻塞 3 秒,放到独立线程,不拖慢看门狗。
        let f2 = f.clone();
        let _ = std::thread::Builder::new().name("loom-sample".into()).spawn(move || {
            let _ = std::process::Command::new("/usr/bin/sample").arg(pid.to_string()).args(["3", "-file"]).arg(&f2).output();
        });
        detail.push_str(&format!("\n调用栈采样文件:{}", crate::logger::redact_home(&f.to_string_lossy())));
    }
    crate::crash::record_event("hang", format!("主线程停滞超过 {} 秒", ms / 1000), detail);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 未开始心跳不报() {
        let mut w = Watch::default();
        assert_eq!(w.observe(10_000, None, 3_000, 10_000), Verdict::Ok);
    }

    #[test]
    fn 警告_深度_恢复各只报一次() {
        let mut w = Watch::default();
        assert_eq!(w.observe(1_000, Some(900), 3_000, 10_000), Verdict::Ok);
        assert_eq!(w.observe(4_000, Some(900), 3_000, 10_000), Verdict::Warn(3_100));
        assert_eq!(w.observe(5_000, Some(900), 3_000, 10_000), Verdict::Ok);
        assert_eq!(w.observe(11_000, Some(900), 3_000, 10_000), Verdict::Ok); // 看门狗自己迟到 6 秒:按休眠处理
        assert_eq!(w.observe(12_000, Some(900), 3_000, 10_000), Verdict::Deep(11_100));
        assert_eq!(w.observe(13_000, Some(900), 3_000, 10_000), Verdict::Ok);
        assert_eq!(w.observe(14_000, Some(13_900), 3_000, 10_000), Verdict::Recovered(12_100));
        assert_eq!(w.observe(15_000, Some(14_900), 3_000, 10_000), Verdict::Ok);
    }

    #[test]
    fn 系统休眠后不误报() {
        let mut w = Watch::default();
        assert_eq!(w.observe(1_000, Some(900), 3_000, 10_000), Verdict::Ok);
        // 休眠 1 小时:心跳和看门狗都停了,醒来第一次判定不报
        assert_eq!(w.observe(3_601_000, Some(900), 3_000, 10_000), Verdict::Ok);
    }
}
