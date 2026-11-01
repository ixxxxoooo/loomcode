//! 文件日志:预先打开的文件句柄、按大小轮转、级别过滤。
//! 不起子进程、不依赖界面,所以 panic 与信号路径也能安全使用(用 `try_write_line` 避免死锁)。
//!
//! @author ixxxxoooo

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// 单个日志文件上限与保留份数(当前文件 + 4 份历史)。
pub const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
pub const KEEP_LOGS: usize = 5;

/// 日志级别,数值越小越重要。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// 完全关闭
    Off = 0,
    Error = 1,
    Warn = 2,
    Info = 3,
    Debug = 4,
}

impl Level {
    /// 全部级别(设置页的选项顺序)。
    pub const ALL: [Level; 5] = [Level::Off, Level::Error, Level::Warn, Level::Info, Level::Debug];

    /// 日志里的大写标签。
    pub fn label(self) -> &'static str {
        match self {
            Level::Off => "OFF",
            Level::Error => "ERROR",
            Level::Warn => "WARN",
            Level::Info => "INFO",
            Level::Debug => "DEBUG",
        }
    }

    /// 稳定的小写名(设置文件与 `LOOM_LOG` 使用)。
    pub fn key(self) -> &'static str {
        match self {
            Level::Off => "off",
            Level::Error => "error",
            Level::Warn => "warn",
            Level::Info => "info",
            Level::Debug => "debug",
        }
    }

    /// 按名称解析(忽略大小写,`warning` 等同 `warn`)。
    pub fn parse(s: &str) -> Option<Level> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" | "none" => Some(Level::Off),
            "error" => Some(Level::Error),
            "warn" | "warning" => Some(Level::Warn),
            "info" => Some(Level::Info),
            "debug" | "trace" => Some(Level::Debug),
            _ => None,
        }
    }

    fn from_u8(v: u8) -> Level {
        match v {
            0 => Level::Off,
            1 => Level::Error,
            2 => Level::Warn,
            3 => Level::Info,
            _ => Level::Debug,
        }
    }
}

struct Inner {
    file: Option<File>,
    size: u64,
}

/// 带轮转的文件日志器。
pub struct Logger {
    inner: Mutex<Inner>,
    path: PathBuf,
    max: u64,
    keep: usize,
}

impl Logger {
    /// 在目录里打开(或创建)`loomcode.log`。
    ///
    /// 参数:`dir` 日志目录;`max` 单文件字节上限;`keep` 保留份数(含当前文件)。
    pub fn open(dir: &Path, max: u64, keep: usize) -> Logger {
        let _ = std::fs::create_dir_all(dir);
        let path = dir.join("loomcode.log");
        let file = OpenOptions::new().create(true).append(true).open(&path).ok();
        let size = file.as_ref().and_then(|f| f.metadata().ok()).map_or(0, |m| m.len());
        Logger { inner: Mutex::new(Inner { file, size }), path, max, keep: keep.max(1) }
    }

    /// 日志文件路径。
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 写一行(带换行),必要时先轮转。
    pub fn write_line(&self, line: &str) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        self.write_locked(&mut g, line);
    }

    /// 尝试写一行;锁被占用(例如崩溃发生在写日志途中)时放弃并返回 `false`。
    pub fn try_write_line(&self, line: &str) -> bool {
        match self.inner.try_lock() {
            Ok(mut g) => {
                self.write_locked(&mut g, line);
                true
            }
            Err(std::sync::TryLockError::Poisoned(e)) => {
                self.write_locked(&mut e.into_inner(), line);
                true
            }
            Err(_) => false,
        }
    }

    fn write_locked(&self, g: &mut Inner, line: &str) {
        if g.size >= self.max {
            self.rotate(g);
        }
        if let Some(f) = g.file.as_mut() {
            let mut buf = String::with_capacity(line.len() + 1);
            buf.push_str(line);
            buf.push('\n');
            if f.write_all(buf.as_bytes()).is_ok() {
                g.size += buf.len() as u64;
            }
        }
    }

    /// 轮转:`log` → `log.1` → `log.2` …,最老的一份被丢弃。
    fn rotate(&self, g: &mut Inner) {
        g.file = None;
        let name = |i: usize| -> PathBuf {
            if i == 0 { self.path.clone() } else { PathBuf::from(format!("{}.{i}", self.path.display())) }
        };
        let _ = std::fs::remove_file(name(self.keep - 1));
        for i in (0..self.keep - 1).rev() {
            let _ = std::fs::rename(name(i), name(i + 1));
        }
        g.file = OpenOptions::new().create(true).append(true).open(&self.path).ok();
        g.size = 0;
    }

    /// 日志末尾 `n` 行(当前文件不够时补上一份历史)。
    pub fn tail(&self, n: usize) -> Vec<String> {
        let mut text = std::fs::read_to_string(&self.path).unwrap_or_default();
        if text.lines().count() < n {
            let prev = std::fs::read_to_string(format!("{}.1", self.path.display())).unwrap_or_default();
            text = format!("{prev}{text}");
        }
        let lines: Vec<&str> = text.lines().collect();
        lines[lines.len().saturating_sub(n)..].iter().map(|s| s.to_string()).collect()
    }
}

/// 全局目录布局:`~/.loomcode` 下的日志、崩溃记录、运行标记。
#[derive(Debug, Clone)]
pub struct Dirs {
    pub base: PathBuf,
    pub logs: PathBuf,
    pub crashes: PathBuf,
    pub run: PathBuf,
}

static LOGGER: OnceLock<Logger> = OnceLock::new();
static DIRS: OnceLock<Dirs> = OnceLock::new();
static LEVEL: AtomicU8 = AtomicU8::new(Level::Info as u8);
static RUN_ID: OnceLock<String> = OnceLock::new();

/// 初始化日志与目录(只生效一次)。级别取环境变量 `LOOM_LOG`,没有就是 info。
///
/// 参数:`home` 用户家目录;数据放在 `<home>/.loomcode`。
pub fn init_home(home: &Path) {
    let base = home.join(".loomcode");
    let dirs = Dirs { logs: base.join("logs"), crashes: base.join("crashes"), run: base.join("run"), base };
    for d in [&dirs.logs, &dirs.crashes, &dirs.run] {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = LOGGER.set(Logger::open(&dirs.logs, MAX_LOG_BYTES, KEEP_LOGS));
    let _ = DIRS.set(dirs);
    if let Some(l) = std::env::var("LOOM_LOG").ok().and_then(|v| Level::parse(&v)) {
        set_level(l);
    }
    let _ = RUN_ID.set(format!("{:x}", (std::process::id() as u64) << 20 ^ (unix_secs() & 0xfffff)));
}

/// 目录布局(未初始化时为 `None`)。
pub fn dirs() -> Option<&'static Dirs> {
    DIRS.get()
}

/// 本次运行的短标识(用来把日志、崩溃记录、运行标记关联起来)。
pub fn run_id() -> &'static str {
    RUN_ID.get().map(String::as_str).unwrap_or("-")
}

/// 当前日志文件路径。
pub fn log_path() -> Option<&'static Path> {
    LOGGER.get().map(Logger::path)
}

/// 全局日志器(崩溃路径用它的 `try_write_line`)。
pub fn logger() -> Option<&'static Logger> {
    LOGGER.get()
}

/// 当前级别。
pub fn level() -> Level {
    Level::from_u8(LEVEL.load(Ordering::Relaxed))
}

/// 修改级别(设置页、`LOOM_LOG`)。
pub fn set_level(l: Level) {
    LEVEL.store(l as u8, Ordering::Relaxed);
}

/// 某级别当前是否会被记录。
pub fn enabled(l: Level) -> bool {
    l != Level::Off && l <= level()
}

fn unix_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// 当前本地时间 `YYYY-MM-DD HH:MM:SS.mmm`,用 libc 计算,不起子进程。
pub fn timestamp() -> String {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs() as libc::time_t;
    // SAFETY: `tm` 由 `localtime_r` 完整填写;失败时保持全零,只会得到 1900 年的时间串。
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&secs, &mut tm) };
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec, d.subsec_millis())
}

/// 把家目录替换为 `~`。
pub fn redact_home(text: &str) -> String {
    match std::env::var("HOME") {
        Ok(h) if !h.is_empty() => text.replace(&h, "~"),
        _ => text.to_string(),
    }
}

/// 格式化一行日志。
fn format_line(level: Level, target: &str, msg: &str) -> String {
    let t = std::thread::current();
    format!("{} [{}] [{}:{}] {}: {}", timestamp(), level.label(), std::process::id(), t.name().unwrap_or("?"), target, redact_home(msg))
}

/// 写一条日志(宏的落点);同时回显到标准错误,开发版的 `target/dev.log` 也能看到。
pub fn log(level: Level, target: &str, args: fmt::Arguments) {
    if !enabled(level) {
        return;
    }
    let line = format_line(level, target, &args.to_string());
    // 标准错误关闭时 `eprintln!` 会 panic,这里忽略写入错误
    let _ = writeln!(std::io::stderr(), "{line}");
    if let Some(l) = LOGGER.get() {
        l.write_line(&line);
    }
}

/// 崩溃路径专用:不阻塞、不 panic;拿不到锁就只写标准错误。
pub fn log_nonblocking(level: Level, target: &str, msg: &str) {
    let line = format_line(level, target, msg);
    let _ = writeln!(std::io::stderr(), "{line}");
    if let Some(l) = LOGGER.get() {
        l.try_write_line(&line);
    }
}

/// 日志末尾 `n` 行。
pub fn tail(n: usize) -> Vec<String> {
    LOGGER.get().map(|l| l.tail(n)).unwrap_or_default()
}

/// 记一条错误日志(级别名按字符串给出,旧调用方式兼容)。
pub fn log_str(level: &str, msg: &str) {
    let lv = Level::parse(level).filter(|l| *l != Level::Off).unwrap_or(Level::Info);
    log(lv, "app", format_args!("{msg}"));
}

/// 错误日志。
#[macro_export]
macro_rules! error {
    ($($a:tt)*) => { $crate::log($crate::Level::Error, module_path!(), format_args!($($a)*)) };
}
/// 警告日志。
#[macro_export]
macro_rules! warn {
    ($($a:tt)*) => { $crate::log($crate::Level::Warn, module_path!(), format_args!($($a)*)) };
}
/// 普通日志。
#[macro_export]
macro_rules! info {
    ($($a:tt)*) => { $crate::log($crate::Level::Info, module_path!(), format_args!($($a)*)) };
}
/// 调试日志。
#[macro_export]
macro_rules! debug {
    ($($a:tt)*) => { $crate::log($crate::Level::Debug, module_path!(), format_args!($($a)*)) };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("loom-diag-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn 级别解析与顺序() {
        assert_eq!(Level::parse("WARN"), Some(Level::Warn));
        assert_eq!(Level::parse("warning"), Some(Level::Warn));
        assert_eq!(Level::parse("乱写"), None);
        assert!(Level::Error < Level::Debug);
        for l in Level::ALL {
            assert_eq!(Level::parse(l.key()), Some(l));
        }
    }

    #[test]
    fn 轮转保留指定份数() {
        let dir = tmp("rotate");
        let lg = Logger::open(&dir, 100, 3);
        for i in 0..40 {
            lg.write_line(&format!("第 {i} 行,凑够长度凑够长度凑够长度"));
        }
        assert!(dir.join("loomcode.log").exists());
        assert!(dir.join("loomcode.log.1").exists());
        assert!(dir.join("loomcode.log.2").exists());
        assert!(!dir.join("loomcode.log.3").exists());
        // 最新的一行一定在当前文件里
        assert!(std::fs::read_to_string(dir.join("loomcode.log")).unwrap().contains("第 39 行"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn 末尾读取跨文件() {
        let dir = tmp("tail");
        let lg = Logger::open(&dir, 60, 3);
        for i in 0..6 {
            lg.write_line(&format!("行{i}-补足补足补足补足"));
        }
        let t = lg.tail(4);
        assert_eq!(t.len(), 4);
        assert!(t.last().unwrap().starts_with("行5"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn 锁被占用时非阻塞写入放弃() {
        let dir = tmp("try");
        let lg = Logger::open(&dir, 1000, 2);
        let _held = lg.inner.lock().unwrap();
        assert!(!lg.try_write_line("不会写入"));
    }

    #[test]
    fn 时间串格式() {
        let t = timestamp();
        assert_eq!(t.len(), 23, "{t}");
        assert_eq!(&t[4..5], "-");
        assert_eq!(&t[19..20], ".");
    }
}
