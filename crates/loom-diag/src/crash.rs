//! 崩溃与异常退出的分层记录:
//! 1. panic 钩子:写 `crashes/*-panic-*.json`(消息、位置、回溯、面包屑、日志尾部);
//! 2. 原生异常(SIGSEGV / SIGBUS / SIGABRT 等):`crash-handler` 在崩溃时往预开的文件写一行摘要,
//!    随后放行,系统照常生成 `.ips`;
//! 3. 运行标记:正常退出才删除,下次启动发现残留就补记「上次异常退出」并归类;
//! 4. 导入系统 `.ips` 报告;
//! 5. SIGTERM / SIGINT / SIGHUP 记为正常退出(开发版重启不再被误判)。
//!
//! @author ixxxxoooo

use crate::records::{self, CrashRecord};
use crate::sentinel::{self, Marker};
use crate::{crumbs, ips, logger, stats};
use std::cell::Cell;
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// 本次运行的基本信息。
#[derive(Debug, Clone)]
pub struct RunInfo {
    pub version: String,
    /// demo / real
    pub mode: String,
}

static INFO: OnceLock<RunInfo> = OnceLock::new();
static START: OnceLock<Instant> = OnceLock::new();
static STARTED_SECS: AtomicU64 = AtomicU64::new(0);
static CLEAN: AtomicBool = AtomicBool::new(false);
static SIGNAL: AtomicI32 = AtomicI32::new(0);
static MARKER_LOCK: Mutex<()> = Mutex::new(());
static NATIVE_FD: AtomicI32 = AtomicI32::new(-1);
static PANIC_SEQ: AtomicU32 = AtomicU32::new(0);
static HANDLER: OnceLock<Mutex<Option<crash_handler::CrashHandler>>> = OnceLock::new();

/// 已运行秒数。
pub fn uptime_s() -> u64 {
    START.get().map_or(0, |s| s.elapsed().as_secs())
}

fn info() -> RunInfo {
    INFO.get().cloned().unwrap_or(RunInfo { version: String::new(), mode: String::new() })
}

/// 安装全部崩溃记录机制;应在日志初始化(`init_home`)之后、创建窗口之前调用。
pub fn install(run: RunInfo) {
    let _ = START.set(Instant::now());
    let _ = INFO.set(run);
    STARTED_SECS.store(stats::now_secs(), Ordering::Relaxed);
    let Some(d) = logger::dirs() else { return };
    install_panic_hook();
    write_marker();
    open_native_file(&d.run);
    if std::env::var("LOOM_NO_CRASH_HANDLER").is_err() {
        install_native_handler();
    }
    install_signals();
    let _ = std::thread::Builder::new().name("loom-heartbeat".into()).spawn(heartbeat_loop);
    let _ = std::thread::Builder::new().name("loom-crash-scan".into()).spawn(|| {
        analyze_previous();
        // 系统的 .ips 往往在崩溃后几十秒才落盘,稍后再补扫一次
        std::thread::sleep(Duration::from_secs(60));
        import_ips();
    });
}

/// 正常退出:记日志并删除运行标记。重复调用无害。
///
/// 参数:`reason` 退出原因(用户退出、模式切换重启、收到 SIGTERM 等)。
pub fn clean_exit(reason: &str) {
    if CLEAN.swap(true, Ordering::SeqCst) {
        return;
    }
    crate::info!("正常退出:{reason}(已运行 {} 秒)", uptime_s());
    let _g = MARKER_LOCK.lock();
    if let Some(d) = logger::dirs() {
        sentinel::remove(&d.run, std::process::id());
    }
}

// ---------- 运行标记与心跳 ----------

fn write_marker() {
    let Some(d) = logger::dirs() else { return };
    let i = info();
    let _g = MARKER_LOCK.lock();
    if CLEAN.load(Ordering::SeqCst) {
        return;
    }
    sentinel::write(&d.run, &Marker { pid: std::process::id(), run_id: logger::run_id().to_string(), started: STARTED_SECS.load(Ordering::Relaxed), version: i.version, mode: i.mode, heartbeat: stats::now_secs(), rss_mb: stats::rss_mb() });
}

/// 每 100ms 检查一次是否收到终止信号,每 5 秒刷新一次心跳。
fn heartbeat_loop() {
    let mut ticks = 0u32;
    loop {
        std::thread::sleep(Duration::from_millis(100));
        let sig = SIGNAL.load(Ordering::SeqCst);
        if sig != 0 {
            clean_exit(&format!("收到信号 {sig}({})", signal_name(sig)));
            // SAFETY: 直接结束进程,不运行 atexit 处理(此时可能不在主线程)。
            unsafe { libc::_exit(128 + sig) };
        }
        ticks += 1;
        if ticks.is_multiple_of(50) {
            write_marker();
        }
    }
}

fn signal_name(sig: i32) -> &'static str {
    match sig {
        libc::SIGTERM => "SIGTERM",
        libc::SIGINT => "SIGINT",
        libc::SIGHUP => "SIGHUP",
        _ => "其他",
    }
}

extern "C" fn on_term_signal(sig: libc::c_int) {
    // 信号处理器里只做原子写入,真正的收尾由心跳线程完成
    SIGNAL.store(sig, Ordering::SeqCst);
}

fn install_signals() {
    for sig in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
        // SAFETY: 处理器只写原子变量,是异步信号安全的。
        unsafe { libc::signal(sig, on_term_signal as *const () as libc::sighandler_t) };
    }
}

// ---------- panic ----------

thread_local! { static IN_HOOK: Cell<bool> = const { Cell::new(false) }; }

fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |pi| {
        // 钩子里再 panic 时直接交给原钩子,避免递归
        if !IN_HOOK.with(|c| c.replace(true)) {
            record_panic(pi);
            IN_HOOK.with(|c| c.set(false));
        }
        prev(pi);
    }));
}

fn record_panic(pi: &std::panic::PanicHookInfo<'_>) {
    let bt = std::backtrace::Backtrace::force_capture();
    let msg = pi.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| pi.payload().downcast_ref::<String>().cloned()).unwrap_or_else(|| "(无消息)".into());
    let loc = pi.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())).unwrap_or_default();
    let t = std::thread::current();
    let thread = t.name().unwrap_or("?").to_string();
    // 在 extern "C" 回调里 panic 无法展开,Rust 会接着再 panic 一次并 abort
    let in_ffi = msg.contains("cannot unwind");
    let seq = PANIC_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
    logger::log_nonblocking(logger::Level::Error, "panic", &format!("崩溃:{pi}\n{bt}"));
    let mut detail = format!("线程:{thread}\n消息:{msg}\n位置:{loc}\n第 {seq} 次 panic{}\n\n调用栈:\n{bt}\n", if in_ffi { "(在 extern \"C\" 回调中 panic,进程随即 abort)" } else { "" });
    detail.push_str("\n最近的操作(面包屑):\n");
    detail.push_str(&crumbs::recent(50).join("\n"));
    if let Some(l) = logger::logger() {
        detail.push_str("\n\n日志尾部:\n");
        detail.push_str(&l.tail(50).join("\n"));
    }
    let i = info();
    records::write(&CrashRecord {
        kind: "panic".into(),
        time: logger::timestamp().chars().take(19).collect(),
        pid: std::process::id(),
        run_id: logger::run_id().to_string(),
        version: i.version,
        mode: i.mode,
        uptime_s: uptime_s(),
        summary: format!("{msg} @ {loc}{}", if in_ffi { "(abort)" } else { "" }),
        detail: logger::redact_home(&detail),
        path: Default::default(),
    });
}

/// 写一条自定义崩溃类记录(Objective-C 异常、卡死等),自动附上面包屑与日志尾部。
///
/// 参数:`kind` 记录类别(见 `CrashRecord::kind_label`);`summary` 一句话摘要;`detail` 细节。
pub fn record_event(kind: &str, summary: String, mut detail: String) {
    detail.push_str("\n\n最近的操作(面包屑):\n");
    detail.push_str(&crumbs::recent(50).join("\n"));
    if let Some(l) = logger::logger() {
        detail.push_str("\n\n日志尾部:\n");
        detail.push_str(&l.tail(50).join("\n"));
    }
    let i = info();
    logger::log_nonblocking(logger::Level::Error, kind, &format!("{summary}\n{detail}"));
    records::write(&CrashRecord {
        kind: kind.into(),
        time: logger::timestamp().chars().take(19).collect(),
        pid: std::process::id(),
        run_id: logger::run_id().to_string(),
        version: i.version,
        mode: i.mode,
        uptime_s: uptime_s(),
        summary,
        detail: logger::redact_home(&detail),
        path: Default::default(),
    });
}

// ---------- 原生异常 ----------

fn open_native_file(run_dir: &Path) {
    use std::os::unix::io::IntoRawFd;
    if let Ok(f) = std::fs::OpenOptions::new().create(true).append(true).open(sentinel::native_path(run_dir, std::process::id())) {
        NATIVE_FD.store(f.into_raw_fd(), Ordering::SeqCst);
    }
}

/// 往固定缓冲里追加,不分配内存(崩溃现场不能依赖分配器)。
struct Buf {
    data: [u8; 256],
    len: usize,
}

impl Buf {
    fn str(&mut self, s: &str) {
        let n = s.len().min(self.data.len() - self.len);
        self.data[self.len..self.len + n].copy_from_slice(&s.as_bytes()[..n]);
        self.len += n;
    }

    fn num(&mut self, mut v: u64) {
        let mut tmp = [0u8; 20];
        let mut i = tmp.len();
        loop {
            i -= 1;
            tmp[i] = b'0' + (v % 10) as u8;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        let n = (tmp.len() - i).min(self.data.len() - self.len);
        self.data[self.len..self.len + n].copy_from_slice(&tmp[i..i + n]);
        self.len += n;
    }
}

fn install_native_handler() {
    // SAFETY: 处理闭包只用 `write(2)` 和栈上缓冲,不分配内存、不加锁;返回「未处理」让系统继续生成崩溃报告。
    let ev = unsafe {
        crash_handler::make_crash_event(move |ctx: &crash_handler::CrashContext| {
            let mut b = Buf { data: [0; 256], len: 0 };
            b.str("exception=");
            #[cfg(target_os = "macos")]
            match ctx.exception {
                Some(e) => {
                    b.str(match e.kind {
                        1 => "EXC_BAD_ACCESS",
                        2 => "EXC_BAD_INSTRUCTION",
                        3 => "EXC_ARITHMETIC",
                        5 => "EXC_SOFTWARE",
                        6 => "EXC_BREAKPOINT",
                        _ => "EXC_OTHER",
                    });
                    b.str(" kind=");
                    b.num(u64::from(e.kind));
                    b.str(" code=");
                    b.num(e.code);
                    if let Some(sub) = e.subcode {
                        b.str(" subcode(地址)=");
                        b.num(sub);
                    }
                }
                None => b.str("SIGABRT(进程主动 abort)"),
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = ctx;
                b.str("native");
            }
            b.str("\n");
            let fd = NATIVE_FD.load(Ordering::SeqCst);
            if fd >= 0 {
                // fd 是启动时打开的文件;缓冲在栈上,长度由 `Buf` 约束
                libc::write(fd, b.data.as_ptr().cast(), b.len);
            }
            crash_handler::CrashEventResult::Handled(false)
        })
    };
    match crash_handler::CrashHandler::attach(ev) {
        Ok(h) => {
            let _ = HANDLER.set(Mutex::new(Some(h)));
        }
        Err(e) => crate::warn!("安装原生崩溃处理器失败:{e}"),
    }
}

// ---------- 上次运行的分析 ----------

/// 导入系统崩溃报告,返回新导入的数量。
pub fn import_ips() -> usize {
    let (Some(d), Some(dir)) = (logger::dirs(), ips::reports_dir()) else { return 0 };
    let seen_path = d.crashes.join("imported-ips.txt");
    let mut seen: HashSet<String> = std::fs::read_to_string(&seen_path).unwrap_or_default().lines().map(str::to_string).collect();
    let version = info().version;
    let mut n = 0;
    for p in ips::list_reports(&dir) {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        if seen.contains(&name) {
            continue;
        }
        let age = std::fs::metadata(&p).and_then(|m| m.modified()).ok().and_then(|m| m.elapsed().ok()).unwrap_or_default();
        // 超过 30 天的旧报告不导入
        if age > Duration::from_secs(30 * 86400) {
            continue;
        }
        let parsed = std::fs::read_to_string(&p).ok().and_then(|t| ips::parse(&t));
        match parsed {
            Some(s) => {
                let rec = s.to_record(&name, &version);
                crate::warn!("导入系统崩溃报告 {name}:{}", rec.summary);
                records::write(&rec);
                n += 1;
            }
            // 可能还在写入:两分钟内的先不标记,下次再试
            None if age < Duration::from_secs(120) => continue,
            None => crate::warn!("系统崩溃报告 {name} 无法解析"),
        }
        seen.insert(name.clone());
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&seen_path) {
            use std::io::Write;
            let _ = writeln!(f, "{name}");
        }
    }
    n
}

/// 一个残留运行标记的归类结果。
#[derive(Debug, Clone, PartialEq)]
pub enum ExitClass {
    /// 有 panic 记录
    Panic,
    /// 有原生异常记录或系统 .ips 报告
    Native,
    /// 有卡死记录,之后被强制结束
    Hang,
    /// 没有任何崩溃线索:被强杀、内存不足被系统终止、断电等
    Unknown,
}

impl ExitClass {
    /// 中文说明。
    pub fn label(&self) -> &'static str {
        match self {
            ExitClass::Panic => "程序崩溃(panic),详见崩溃记录",
            ExitClass::Native => "原生崩溃,详见系统崩溃报告",
            ExitClass::Hang => "主线程卡死后被强制结束",
            ExitClass::Unknown => "没有崩溃线索:被强制结束(kill -9、强制退出)、内存不足被系统终止或断电",
        }
    }
}

/// 按已有记录给残留标记归类(纯函数,便于测试)。
pub fn classify(m: &Marker, records: &[CrashRecord], native_text: &str) -> ExitClass {
    let same_run = |kind: &str| records.iter().any(|r| r.kind == kind && r.run_id == m.run_id);
    if same_run("panic") {
        ExitClass::Panic
    } else if !native_text.trim().is_empty() || records.iter().any(|r| r.kind == "ips" && r.pid == m.pid) || same_run("signal") || same_run("nsexception") {
        ExitClass::Native
    } else if same_run("hang") {
        ExitClass::Hang
    } else {
        ExitClass::Unknown
    }
}

/// 启动时分析上次运行:导入 .ips,处理残留的运行标记。
pub fn analyze_previous() {
    let Some(d) = logger::dirs() else { return };
    import_ips();
    let records = records::list_in(&d.crashes, 500);
    for m in sentinel::scan_dead(&d.run, std::process::id()) {
        let native = std::fs::read_to_string(sentinel::native_path(&d.run, m.pid)).unwrap_or_default();
        let class = classify(&m, &records, &native);
        let last_alive_s = m.heartbeat.saturating_sub(m.started);
        crate::warn!("上次运行异常退出:pid={} 运行 {} 秒 最后内存 {} MB 模式={} 归类:{}", m.pid, last_alive_s, m.rss_mb, m.mode, class.label());
        // panic 与 .ips 已有自己的记录;原生异常摘要和「无线索」的退出需要补一条
        let rec = |kind: &str, summary: String, detail: String| CrashRecord {
            kind: kind.into(),
            time: logger::timestamp().chars().take(19).collect(),
            pid: m.pid,
            run_id: m.run_id.clone(),
            version: m.version.clone(),
            mode: m.mode.clone(),
            uptime_s: last_alive_s,
            summary,
            detail,
            path: Default::default(),
        };
        if !native.trim().is_empty() && class != ExitClass::Panic {
            records::write(&rec("signal", native.lines().next().unwrap_or_default().to_string(), format!("崩溃时由原生处理器记录:\n{native}\n(完整调用栈见同时导入的系统崩溃报告)")));
        }
        if matches!(class, ExitClass::Unknown | ExitClass::Hang) {
            let detail = format!("最后一次心跳距启动 {last_alive_s} 秒;最后一次内存采样 {} MB。\n归类:{}\n(运行标记残留,说明进程没有走正常退出流程)", m.rss_mb, class.label());
            records::write(&rec("abnormal-exit", class.label().to_string(), detail));
        }
        sentinel::remove(&d.run, m.pid);
    }
}

// ---------- 调试:主动制造故障 ----------

/// 调试用:2 秒后在后台线程制造一种故障,验证记录是否齐全。
///
/// 参数:`kind` 取 `panic`(普通 panic)、`segv`(非法内存访问)、`abort`、`kill`(自己给自己 SIGKILL);`hang` 由应用侧制造。
pub fn trigger_test(kind: &str) {
    let kind = kind.to_string();
    let _ = std::thread::Builder::new().name("loom-crash-test".into()).spawn(move || {
        std::thread::sleep(Duration::from_secs(2));
        crate::warn!("LOOM_CRASH_TEST={kind}:即将制造故障");
        match kind.as_str() {
            "panic" => panic!("LOOM_CRASH_TEST 主动 panic"),
            // SAFETY: 故意写非法地址制造 EXC_BAD_ACCESS,仅用于调试验证。
            "segv" => unsafe { std::ptr::write_volatile(8 as *mut u8, 1) },
            "abort" => std::process::abort(),
            // hang 要卡住界面线程,由应用侧的心跳循环处理
            "hang" => {}
            // SAFETY: 向自己发 SIGKILL,模拟被强杀。
            "kill" => unsafe {
                libc::kill(libc::getpid(), libc::SIGKILL);
            },
            other => crate::warn!("未知的 LOOM_CRASH_TEST 取值:{other}"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker() -> Marker {
        Marker { pid: 42, run_id: "run-a".into(), started: 10, version: "0".into(), mode: "real".into(), heartbeat: 20, rss_mb: 5 }
    }

    fn rec(kind: &str, pid: u32, run: &str) -> CrashRecord {
        CrashRecord { kind: kind.into(), pid, run_id: run.into(), ..Default::default() }
    }

    #[test]
    fn 没有线索归为未知() {
        assert_eq!(classify(&marker(), &[], ""), ExitClass::Unknown);
        // 别的运行的记录不算
        assert_eq!(classify(&marker(), &[rec("panic", 42, "run-b")], ""), ExitClass::Unknown);
    }

    #[test]
    fn panic_优先于原生() {
        let r = [rec("panic", 42, "run-a"), rec("ips", 42, "")];
        assert_eq!(classify(&marker(), &r, "exception=x"), ExitClass::Panic);
    }

    #[test]
    fn ips_或原生文件归为原生() {
        assert_eq!(classify(&marker(), &[rec("ips", 42, "")], ""), ExitClass::Native);
        assert_eq!(classify(&marker(), &[], "exception=EXC_BAD_ACCESS\n"), ExitClass::Native);
        // 不同 pid 的 ips 不算
        assert_eq!(classify(&marker(), &[rec("ips", 7, "")], ""), ExitClass::Unknown);
    }

    #[test]
    fn 卡死记录归为卡死() {
        assert_eq!(classify(&marker(), &[rec("hang", 42, "run-a")], ""), ExitClass::Hang);
    }

    #[test]
    fn 缓冲拼接不越界() {
        let mut b = Buf { data: [0; 256], len: 0 };
        b.str("a=");
        b.num(0);
        b.str(" b=");
        b.num(18446744073709551615);
        assert_eq!(&b.data[..b.len], b"a=0 b=18446744073709551615");
        for _ in 0..100 {
            b.str("填满填满填满");
        }
        assert_eq!(b.len, 256);
    }
}
