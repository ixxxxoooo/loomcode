//! 终端事件:程序对终端的查询应答、响铃、剪贴板写入与标题。
//!
//! 应答不在解析中途直接写回 pty,而是先按原顺序排队,等读线程放掉终端锁之后统一发出:
//! 这样既保持「先问先答」的顺序(如 `OSC 11 ? ST` 后接 `CSI c`),又能读到程序自己用 OSC 4 / 10 / 11 改过的颜色。
//!
//! @author ixxxxoooo

use crate::theme::{self, TermTheme};
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::vte::ansi::Rgb;
use std::io::Write;
use std::sync::{Arc, Mutex};

/// 界面需要处理的终端事件(用 `Terminal::drain_events` 取走)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TermEvent {
    /// 程序响铃(BEL)
    Bell,
    /// 程序用 OSC 52 请求写系统剪贴板
    ClipboardStore(String),
}

/// OSC 52 写剪贴板的内容上限,防止程序塞入超大数据。
const CLIPBOARD_MAX: usize = 1 << 20;

type ColorFormat = Arc<dyn Fn(Rgb) -> String + Sync + Send + 'static>;

/// 排队等待写回 pty 的应答。
pub(crate) enum Reply {
    Text(String),
    Color(usize, ColorFormat),
}

pub(crate) type Pending = Arc<Mutex<Vec<Reply>>>;
pub(crate) type SharedWriter = Arc<Mutex<Box<dyn Write + Send>>>;

/// 接收 alacritty 的事件:应答排队,标题记录,响铃与剪贴板交给界面。
#[derive(Clone)]
pub(crate) struct Listener {
    pub pending: Pending,
    pub title: Arc<Mutex<String>>,
    pub events: Arc<Mutex<Vec<TermEvent>>>,
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        match event {
            Event::PtyWrite(text) => self.pending.lock().unwrap().push(Reply::Text(text)),
            Event::ColorRequest(index, format) => self.pending.lock().unwrap().push(Reply::Color(index, format)),
            Event::Title(t) => *self.title.lock().unwrap() = t,
            Event::ResetTitle => self.title.lock().unwrap().clear(),
            Event::Bell => self.events.lock().unwrap().push(TermEvent::Bell),
            Event::ClipboardStore(_, text) if text.len() <= CLIPBOARD_MAX => self.events.lock().unwrap().push(TermEvent::ClipboardStore(text)),
            // 程序读剪贴板(OSC 52 ?)会泄露用户剪贴板内容,一律不答
            _ => {}
        }
    }
}

/// 把排队的应答按顺序写回 pty。
///
/// 参数:`colors` 取程序自己设置过的颜色(序号 → 颜色);没设置过就按主题 `theme` 答。
pub(crate) fn flush(pending: &Pending, writer: &SharedWriter, theme: &Mutex<Option<TermTheme>>, colors: impl Fn(usize) -> Option<Rgb>) {
    let replies = std::mem::take(&mut *pending.lock().unwrap());
    if replies.is_empty() {
        return;
    }
    let mut out = String::new();
    for r in replies {
        match r {
            Reply::Text(t) => out.push_str(&t),
            Reply::Color(index, format) => {
                let rgb = colors(index).or_else(|| theme.lock().ok().and_then(|g| g.as_ref().and_then(|t| theme::lookup(t, index))));
                if let Some(rgb) = rgb {
                    out.push_str(&format(rgb));
                }
            }
        }
    }
    if let Ok(mut w) = writer.lock() {
        let _ = w.write_all(out.as_bytes());
        let _ = w.flush();
    }
}
