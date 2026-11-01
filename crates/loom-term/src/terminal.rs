//! 终端实例:用 pty 起 shell,读线程把输出喂给 alacritty 的解析器,界面按需取快照。
//!
//! @author ixxxxoooo

use crate::snapshot::{self, Snapshot};
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
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

/// 选择的粒度:单击拖动选字符,双击选单词,三击选整行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectKind {
    Simple,
    Word,
    Line,
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

pub(crate) struct Size {
    pub cols: usize,
    pub rows: usize,
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

/// 画面上的 (行, 列) → 网格坐标(要减去回看偏移);越界夹到边缘。
fn screen_point<T>(term: &Term<T>, row: usize, col: usize) -> Point {
    let offset = term.grid().display_offset() as i32;
    let row = row.min(term.screen_lines().saturating_sub(1)) as i32;
    Point::new(Line(row - offset), Column(col.min(term.columns().saturating_sub(1))))
}

fn side(right: bool) -> Side {
    if right { Side::Right } else { Side::Left }
}

/// 调整终端网格大小。
///
/// 列数和行数同时变化时分两步:先改列数(保持原行数),再改行数。alacritty 内部先处理行数、
/// 再重排列数,一步到位的话,缩窄重排多出来的折行会把上面的内容顶进回看历史;
/// 分两步则加高行数时会把历史里的行拉回画面。
pub(crate) fn resize_term<T: EventListener>(term: &mut Term<T>, cols: usize, rows: usize) {
    let old_rows = term.screen_lines();
    if cols != term.columns() && rows != old_rows {
        term.resize(Size { cols, rows: old_rows });
    }
    term.resize(Size { cols, rows });
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
        resize_term(&mut inner.term, cols as usize, rows as usize);
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

    /// 开始一次选择。`row` / `col` 是画面上的行列(0 起始,越界会夹到边缘),
    /// `right` 表示鼠标落在字符右半边(决定边界字符是否被选中)。
    pub fn start_selection(&self, row: usize, col: usize, right: bool, kind: SelectKind) {
        let mut g = self.inner.lock().unwrap();
        let p = screen_point(&g.term, row, col);
        let ty = match kind {
            SelectKind::Simple => SelectionType::Simple,
            SelectKind::Word => SelectionType::Semantic,
            SelectKind::Line => SelectionType::Lines,
        };
        g.term.selection = Some(Selection::new(ty, p, side(right)));
    }

    /// 拖动时更新选择终点;没有进行中的选择则忽略。
    pub fn update_selection(&self, row: usize, col: usize, right: bool) {
        let mut g = self.inner.lock().unwrap();
        let p = screen_point(&g.term, row, col);
        if let Some(sel) = g.term.selection.as_mut() {
            sel.update(p, side(right));
        }
    }

    /// 清除选择。
    pub fn clear_selection(&self) {
        self.inner.lock().unwrap().term.selection = None;
    }

    /// 选中的文本;没有选择或选区为空返回 `None`。
    pub fn selection_text(&self) -> Option<String> {
        self.inner.lock().unwrap().term.selection_to_string().filter(|s| !s.is_empty())
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
#[path = "terminal_tests.rs"]
mod tests;
