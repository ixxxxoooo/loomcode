//! 终端实例:用 pty 起 shell,读线程把输出喂给 alacritty 的解析器,界面按需取快照。
//!
//! @author ixxxxoooo

use crate::snapshot::{self, Snapshot};
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::Processor;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

/// 启动参数。
#[derive(Debug, Clone, Default)]
pub struct SpawnOptions {
    /// 工作目录;为空用进程当前目录
    pub cwd: Option<String>,
    /// 要运行的命令(程序 + 参数);为空则启动登录 shell(`$SHELL -l`,没有则 `/bin/zsh`)
    pub command: Option<Vec<String>>,
    pub cols: u16,
    pub rows: u16,
}

type SharedWriter = Arc<Mutex<Box<dyn Write + Send>>>;

/// 接收终端事件:把程序对终端的查询应答写回 pty,记录标题。
#[derive(Clone)]
struct Listener {
    writer: SharedWriter,
    title: Arc<Mutex<String>>,
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        match event {
            Event::PtyWrite(text) => {
                if let Ok(mut w) = self.writer.lock() {
                    let _ = w.write_all(text.as_bytes());
                    let _ = w.flush();
                }
            }
            Event::Title(t) => *self.title.lock().unwrap() = t,
            Event::ResetTitle => self.title.lock().unwrap().clear(),
            _ => {}
        }
    }
}

struct Size {
    cols: usize,
    rows: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

struct Inner {
    term: Term<Listener>,
    parser: Processor,
}

/// 一个终端。丢弃时会结束它的子进程。
pub struct Terminal {
    inner: Arc<Mutex<Inner>>,
    writer: SharedWriter,
    master: Mutex<Box<dyn MasterPty + Send>>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    title: Arc<Mutex<String>>,
    exit_code: Arc<Mutex<Option<u32>>>,
}

impl Terminal {
    /// 启动终端。
    ///
    /// 参数:`opts` 启动参数;`on_update` 在有新输出或进程退出时被调用(来自读线程,
    /// 界面应只在里面发一个「请重绘」的信号,不要做重活)。
    /// 返回:成功返回终端;失败返回给用户看的原因。
    pub fn spawn(opts: SpawnOptions, on_update: impl Fn() + Send + Sync + 'static) -> Result<Self, String> {
        let (cols, rows) = (opts.cols.max(2), opts.rows.max(1));
        let pair = native_pty_system().openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }).map_err(|e| format!("无法创建终端:{e}"))?;
        let mut cmd = match &opts.command {
            Some(argv) if !argv.is_empty() => {
                let mut c = CommandBuilder::new(&argv[0]);
                c.args(&argv[1..]);
                c
            }
            _ => {
                let mut c = CommandBuilder::new(std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into()));
                c.arg("-l");
                c
            }
        };
        if let Some(cwd) = opts.cwd.as_deref().filter(|d| std::path::Path::new(d).is_dir()) {
            cmd.cwd(cwd);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        let child = pair.slave.spawn_command(cmd).map_err(|e| format!("无法启动 shell:{e}"))?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| format!("无法读取终端:{e}"))?;
        let writer: SharedWriter = Arc::new(Mutex::new(pair.master.take_writer().map_err(|e| format!("无法写入终端:{e}"))?));
        let title = Arc::new(Mutex::new(String::new()));
        let listener = Listener { writer: writer.clone(), title: title.clone() };
        let term = Term::new(Config::default(), &Size { cols: cols as usize, rows: rows as usize }, listener);
        let inner = Arc::new(Mutex::new(Inner { term, parser: Processor::default() }));
        let child = Arc::new(Mutex::new(child));
        let exit_code = Arc::new(Mutex::new(None));
        let on_update = Arc::new(on_update);

        let (inner2, child2, exit2, notify) = (inner.clone(), child.clone(), exit_code.clone(), on_update.clone());
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        {
                            let mut g = inner2.lock().unwrap();
                            let Inner { term, parser } = &mut *g;
                            parser.advance(term, &buf[..n]);
                        }
                        notify();
                    }
                }
            }
            // 输出读完说明 shell 退出了:收尸并记下退出码
            let code = child2.lock().unwrap().wait().map(|s| s.exit_code()).unwrap_or(1);
            *exit2.lock().unwrap() = Some(code);
            notify();
        });
        Ok(Self { inner, writer, master: Mutex::new(pair.master), child, title, exit_code })
    }

    /// 向终端写入字节(键盘输入、粘贴)。
    pub fn write(&self, bytes: &[u8]) {
        if let Ok(mut w) = self.writer.lock() {
            let _ = w.write_all(bytes);
            let _ = w.flush();
        }
    }

    /// 调整大小;尺寸没变时什么都不做。
    pub fn resize(&self, cols: u16, rows: u16) {
        let (cols, rows) = (cols.max(2), rows.max(1));
        let mut inner = self.inner.lock().unwrap();
        if inner.term.columns() == cols as usize && inner.term.screen_lines() == rows as usize {
            return;
        }
        inner.term.resize(Size { cols: cols as usize, rows: rows as usize });
        drop(inner);
        let _ = self.master.lock().unwrap().resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
    }

    /// 滚动回看历史:正数向上(更早),负数向下。
    pub fn scroll(&self, lines: i32) {
        self.inner.lock().unwrap().term.scroll_display(Scroll::Delta(lines));
    }

    /// 回到最底部。
    pub fn scroll_to_bottom(&self) {
        self.inner.lock().unwrap().term.scroll_display(Scroll::Bottom);
    }

    /// 程序是否开启了「应用光标键模式」(决定方向键的编码)。
    pub fn app_cursor(&self) -> bool {
        self.inner.lock().unwrap().term.mode().contains(TermMode::APP_CURSOR)
    }

    /// 程序是否开启了括号粘贴模式(粘贴时要用 `ESC[200~` 包起来)。
    pub fn bracketed_paste(&self) -> bool {
        self.inner.lock().unwrap().term.mode().contains(TermMode::BRACKETED_PASTE)
    }

    /// 取当前画面快照。
    pub fn snapshot(&self) -> Snapshot {
        let title = self.title.lock().unwrap().clone();
        let code = *self.exit_code.lock().unwrap();
        snapshot::build(&self.inner.lock().unwrap().term, title, code)
    }

    /// shell 的退出码;还在运行返回 `None`。
    pub fn exit_code(&self) -> Option<u32> {
        *self.exit_code.lock().unwrap()
    }

    /// 结束子进程(重复调用无害)。
    pub fn kill(&self) {
        let _ = self.child.lock().unwrap().kill();
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// 轮询直到画面满足条件,最多等 5 秒;返回最后一次的画面文本。
    fn wait_for(t: &Terminal, cond: impl Fn(&Snapshot) -> bool) -> Snapshot {
        let start = Instant::now();
        loop {
            let s = t.snapshot();
            if cond(&s) || start.elapsed() > Duration::from_secs(5) {
                return s;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn sh(script: &str) -> Terminal {
        Terminal::spawn(SpawnOptions { command: Some(vec!["/bin/sh".into(), "-c".into(), script.into()]), cols: 80, rows: 24, ..Default::default() }, || {}).unwrap()
    }

    #[test]
    fn 命令输出与退出码() {
        let t = sh("echo hello-ucode; exit 3");
        let s = wait_for(&t, |s| s.exit_code.is_some());
        assert!(s.text().contains("hello-ucode"), "画面:{:?}", s.text());
        assert_eq!(s.exit_code, Some(3));
        assert_eq!(t.exit_code(), Some(3));
    }

    #[test]
    fn 交互输入与回显() {
        let t = Terminal::spawn(SpawnOptions { command: Some(vec!["/bin/sh".into()]), cols: 80, rows: 24, ..Default::default() }, || {}).unwrap();
        t.write(b"echo $((6*7))\n");
        let s = wait_for(&t, |s| s.text().lines().any(|l| l.trim() == "42"));
        assert!(s.text().lines().any(|l| l.trim() == "42"), "画面:{:?}", s.text());
        t.write(b"exit 0\n");
        assert_eq!(wait_for(&t, |s| s.exit_code.is_some()).exit_code, Some(0));
    }

    #[test]
    fn 调整大小后程序看到新尺寸() {
        let t = Terminal::spawn(SpawnOptions { command: Some(vec!["/bin/sh".into()]), cols: 80, rows: 24, ..Default::default() }, || {}).unwrap();
        t.resize(100, 30);
        t.write(b"stty size\n");
        let s = wait_for(&t, |s| s.text().contains("30 100"));
        assert!(s.text().contains("30 100"), "画面:{:?}", s.text());
        assert_eq!((s.rows, s.cols), (30, 100));
    }

    #[test]
    fn 工作目录与颜色输出() {
        let dir = std::env::temp_dir().canonicalize().unwrap();
        let t = Terminal::spawn(SpawnOptions { cwd: Some(dir.to_string_lossy().to_string()), command: Some(vec!["/bin/sh".into(), "-c".into(), "pwd; printf '\\033[31mred\\033[0m\\n'".into()]), cols: 80, rows: 24 }, || {}).unwrap();
        let s = wait_for(&t, |s| s.exit_code.is_some());
        assert!(s.text().contains(dir.to_string_lossy().as_ref()), "画面:{:?}", s.text());
        let red = s.lines.iter().flatten().find(|r| r.text == "red").expect("应有红色文字段");
        assert_eq!(red.fg, crate::TermColor::Indexed(1));
    }

    #[test]
    fn 更新回调会被触发() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let n = Arc::new(AtomicUsize::new(0));
        let n2 = n.clone();
        let t = Terminal::spawn(SpawnOptions { command: Some(vec!["/bin/sh".into(), "-c".into(), "echo x".into()]), cols: 80, rows: 24, ..Default::default() }, move || {
            n2.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
        wait_for(&t, |s| s.exit_code.is_some());
        assert!(n.load(Ordering::SeqCst) >= 1);
    }

    #[test]
    fn kill_后读线程会收尸() {
        let t = sh("sleep 30");
        assert_eq!(t.exit_code(), None);
        t.kill();
        let s = wait_for(&t, |s| s.exit_code.is_some());
        assert!(s.exit_code.is_some(), "kill 后读线程应收尸");
    }

    /// 进程是否还活着(`kill -0` 不发信号,只探测)。
    fn alive(pid: u32) -> bool {
        std::process::Command::new("kill").args(["-0", &pid.to_string()]).stderr(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
    }

    #[test]
    fn 丢弃终端会结束子进程() {
        let t = sh("sleep 30");
        let pid = t.child.lock().unwrap().process_id().expect("应有进程号");
        assert!(alive(pid));
        drop(t);
        let start = Instant::now();
        while alive(pid) && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!alive(pid), "Drop 后子进程应被结束");
    }
}
