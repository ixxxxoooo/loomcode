//! 终端视图:把 `ucode-term` 的画面快照画出来,处理键盘、粘贴、滚轮和尺寸跟随。
//!
//! @author ixxxxoooo

use crate::term_palette::indexed_rgb;
use crate::ui::MONO;
use gpui_kit::component::ActiveTheme;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::ops::Range;
use std::sync::Arc;
use ucode_term::{Mods, Run, SpawnOptions, TermColor, Terminal, encode_key};

const FONT_SIZE: f32 = 12.;
const LINE_H: f32 = 16.;
/// 终端内容距边框的留白
const PAD: f32 = 6.;

/// 一个终端的视图。
pub struct TerminalView {
    term: Arc<Terminal>,
    focus: FocusHandle,
    /// 等宽字体一个字符的宽度(第一次渲染时量出来)
    cell_w: Option<Pixels>,
    /// 当前终端的 (列, 行)
    grid: (u16, u16),
    _pump: Task<()>,
}

impl TerminalView {
    /// 启动一个终端并创建视图。
    ///
    /// 参数:`cwd` 工作目录;`command` 要运行的命令,为空则启动登录 shell。
    /// 返回:成功返回视图实体;失败返回给用户看的原因。
    pub fn start(cwd: &str, command: Option<Vec<String>>, cx: &mut App) -> Result<Entity<Self>, String> {
        // 读线程只发一个「请重绘」信号,容量 1 自然合并连续输出
        let (tx, rx) = async_channel::bounded::<()>(1);
        let term = Terminal::spawn(SpawnOptions { cwd: Some(cwd.to_string()), command, cols: 80, rows: 24 }, move || {
            let _ = tx.try_send(());
        })?;
        Ok(cx.new(|cx| {
            let pump = cx.spawn(async move |this, cx| {
                while rx.recv().await.is_ok() {
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            });
            Self { term: Arc::new(term), focus: cx.focus_handle(), cell_w: None, grid: (80, 24), _pump: pump }
        }))
    }

    /// 程序设置的标题;没有则为空。
    pub fn title(&self) -> String {
        self.term.snapshot().title
    }

    /// shell 的退出码;还在运行返回 `None`。
    pub fn exit_code(&self) -> Option<u32> {
        self.term.exit_code()
    }

    /// 焦点句柄:`window.focus(&handle, cx)` 之后键盘输入都发给这个终端。
    pub fn focus_handle(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// 由容器尺寸换算成列数 / 行数并通知终端调整大小。
    fn apply_bounds(&mut self, bounds: Bounds<Pixels>, cx: &mut Context<Self>) {
        let Some(cell_w) = self.cell_w else { return };
        let cols = (((bounds.size.width - px(2. * PAD)) / cell_w).floor().max(2.)) as u16;
        let rows = (((bounds.size.height - px(2. * PAD)) / px(LINE_H)).floor().max(1.)) as u16;
        if (cols, rows) != self.grid {
            self.grid = (cols, rows);
            self.term.resize(cols, rows);
            cx.notify();
        }
    }

    fn on_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let (k, m) = (&ev.keystroke, &ev.keystroke.modifiers);
        if m.platform {
            // ⌘V 粘贴;其余 ⌘ 组合键(⌘K、⌘B 等)交还给应用
            if k.key == "v" {
                self.paste(cx);
                cx.stop_propagation();
            }
            return;
        }
        let mods = Mods { ctrl: m.control, alt: m.alt, shift: m.shift };
        let bytes = encode_key(&k.key, mods, self.term.app_cursor())
            .or_else(|| k.key_char.as_ref().filter(|_| !m.control && !m.alt).map(|s| s.clone().into_bytes()));
        if let Some(bytes) = bytes {
            self.term.scroll_to_bottom();
            self.term.write(&bytes);
            cx.stop_propagation();
        }
    }

    /// 粘贴剪贴板文本;程序开了括号粘贴模式时用 `ESC[200~ … ESC[201~` 包起来。
    fn paste(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) else { return };
        let text = text.replace("\r\n", "\n").replace('\n', "\r");
        let data = if self.term.bracketed_paste() { format!("\x1b[200~{text}\x1b[201~") } else { text };
        self.term.scroll_to_bottom();
        self.term.write(data.as_bytes());
    }

    fn on_wheel(&mut self, ev: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let lines = (ev.delta.pixel_delta(px(LINE_H)).y / px(LINE_H)).round() as i32;
        if lines != 0 {
            self.term.scroll(lines);
            cx.notify();
            cx.stop_propagation();
        }
    }
}

/// 终端颜色 → 界面颜色;`Default` 用调用方给的默认色。
fn to_hsla(c: TermColor, default: Hsla, dark: bool) -> Hsla {
    let (r, g, b) = match c {
        TermColor::Default => return default,
        TermColor::Indexed(i) => indexed_rgb(i, dark),
        TermColor::Rgb(r, g, b) => (r, g, b),
    };
    rgb(((r as u32) << 16) | ((g as u32) << 8) | b as u32).into()
}

/// 一段文字的高亮样式;和默认样式完全一样时返回 `None`(省掉一个区间)。
fn run_style(run: &Run, fg_default: Hsla, bg_default: Hsla, dark: bool) -> Option<HighlightStyle> {
    let s = run.style;
    let plain = run.fg == TermColor::Default && run.bg == TermColor::Default && !(s.bold || s.italic || s.underline || s.inverse || s.dim || s.strike);
    if plain {
        return None;
    }
    let (mut fg, mut bg) = (to_hsla(run.fg, fg_default, dark), (run.bg != TermColor::Default).then(|| to_hsla(run.bg, bg_default, dark)));
    if s.inverse {
        (fg, bg) = (bg.unwrap_or(bg_default), Some(fg));
    }
    if s.dim {
        fg = fg.opacity(0.6);
    }
    Some(HighlightStyle {
        color: Some(fg),
        background_color: bg,
        font_weight: s.bold.then_some(FontWeight::BOLD),
        font_style: s.italic.then_some(FontStyle::Italic),
        underline: s.underline.then(|| UnderlineStyle { thickness: px(1.), color: None, wavy: false }),
        strikethrough: s.strike.then(|| StrikethroughStyle { thickness: px(1.), color: None }),
        ..Default::default()
    })
}

/// 一行文字 → 文本与高亮区间。
fn line_text(runs: &[Run], fg: Hsla, bg: Hsla, dark: bool) -> (String, Vec<(Range<usize>, HighlightStyle)>) {
    let mut text = String::new();
    let mut hl = Vec::new();
    for r in runs {
        let start = text.len();
        text.push_str(&r.text);
        if let Some(style) = run_style(r, fg, bg, dark) {
            hl.push((start..text.len(), style));
        }
    }
    (text, hl)
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let dark = t.is_dark();
        if self.cell_w.is_none() {
            let font = Font { family: MONO.into(), features: FontFeatures::default(), fallbacks: None, weight: FontWeight::NORMAL, style: FontStyle::Normal };
            let id = window.text_system().resolve_font(&font);
            self.cell_w = window.text_system().advance(id, px(FONT_SIZE), 'm').ok().map(|s| s.width);
        }
        let cell_w = self.cell_w.unwrap_or(px(7.));
        let snap = self.term.snapshot();
        let focused = self.focus.is_focused(window);
        let view = cx.entity();
        let lines = snap.lines.iter().map(|runs| {
            let (text, hl) = line_text(runs, t.foreground, t.background, dark);
            let row = div().h(px(LINE_H)).whitespace_nowrap();
            if text.is_empty() { row } else { row.child(StyledText::new(text).with_highlights(hl)) }
        });
        div().id("terminal").key_context("Terminal").track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| this.on_key(ev, cx)))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| window.focus(&this.focus, cx)))
            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| this.on_wheel(ev, cx)))
            .relative().size_full().overflow_hidden().bg(t.background).text_color(t.foreground)
            .font_family(MONO).text_size(px(FONT_SIZE)).line_height(px(LINE_H))
            .child(div().absolute().top(px(PAD)).left(px(PAD)).flex().flex_col().children(lines))
            .when_some(snap.cursor, |d, (row, col)| {
                let cursor = div().absolute().left(px(PAD) + cell_w * col as f32).top(px(PAD + LINE_H * row as f32)).w(cell_w).h(px(LINE_H));
                d.child(if focused { cursor.bg(t.primary.opacity(0.6)) } else { cursor.border_1().border_color(t.primary) })
            })
            .when_some(snap.exit_code, |d, code| d.child(
                div().absolute().bottom_0().left_0().right_0().px_2().py_1().bg(t.muted).text_color(t.muted_foreground)
                    .child(format!("进程已退出(退出码 {code}),可以关闭这个终端标签"))))
            // 隐形的测量层:拿到容器尺寸后换算列数 / 行数
            .child(canvas(move |bounds, _, cx| view.update(cx, |v, cx| v.apply_bounds(bounds, cx)), |_, _, _, _| {}).absolute().size_full())
    }
}
