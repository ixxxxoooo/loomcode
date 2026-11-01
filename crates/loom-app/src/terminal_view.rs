//! 终端视图:把 `loom-term` 的画面快照画出来,处理键盘、粘贴、滚轮和尺寸跟随。
//!
//! @author ixxxxoooo

use crate::term_geom::{cell_at, selection_spans};
use crate::term_palette::indexed_rgb;
use crate::ui::MONO;
use gpui_kit::component::ActiveTheme;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::ops::Range;
use std::sync::Arc;
use loom_config::CursorShape;
use loom_term::{Mods, Run, SelectKind, SpawnOptions, TermColor, Terminal, encode_key};

/// 终端内容距边框的留白
const PAD: f32 = 6.;

/// 一个终端的视图。
pub struct TerminalView {
    term: Arc<Terminal>,
    focus: FocusHandle,
    /// 当前字号与行高(随设置变化,变了就重新量字宽)
    metrics: (f32, f32),
    /// 等宽字体一个字符的宽度(第一次渲染时量出来)
    cell_w: Option<Pixels>,
    /// 当前终端的 (列, 行)
    grid: (u16, u16),
    /// 终端区域在窗口里的位置(把鼠标坐标换算成格子用)
    bounds: Option<Bounds<Pixels>>,
    /// 鼠标左键正按着,在拖动选择
    selecting: bool,
    /// 这次按下之后鼠标是否移动过(没动过的单击要清掉选区)
    dragged: bool,
    /// 滚轮累计的未满一行的像素(平滑滚动 / 触控板每次只给几像素)
    wheel_acc: f32,
    /// 前台进程名的缓存(查询时间, 进程名),见 `label`
    fg_cache: std::cell::RefCell<(std::time::Instant, Option<String>)>,
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
        let tp = crate::prefs(cx).terminal;
        let cwd = if tp.start_in_project { cwd.to_string() } else { std::env::var("HOME").unwrap_or_else(|_| cwd.to_string()) };
        let opts = SpawnOptions {
            cwd: Some(cwd),
            command,
            cols: 80,
            rows: 24,
            shell: Some(tp.shell.clone()),
            no_login: !tp.login_shell,
            scrollback: Some(tp.scrollback as usize),
        };
        let term = Terminal::spawn(opts, move || {
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
            // 调试用:LOOM_TERM_SELECT=起点行,起点列,终点行,终点列,一秒后自动选中这一段,方便截图检查高亮
            let term = Arc::new(term);
            if let Some([r0, c0, r1, c1]) = std::env::var("LOOM_TERM_SELECT").ok().and_then(|v| v.split(',').map(|n| n.trim().parse::<usize>().ok()).collect::<Option<Vec<_>>>()).and_then(|v| <[usize; 4]>::try_from(v).ok()) {
                let (t, ex) = (term.clone(), cx.background_executor().clone());
                cx.spawn(async move |this, cx| {
                    ex.timer(std::time::Duration::from_secs(1)).await;
                    t.start_selection(r0, c0, false, SelectKind::Simple);
                    t.update_selection(r1, c1, true);
                    let _ = this.update(cx, |_, cx| cx.notify());
                })
                .detach();
            }
            Self { term, focus: cx.focus_handle(), metrics: metrics_of(tp.font_size), cell_w: None, grid: (80, 24), bounds: None, selecting: false, dragged: false, wheel_acc: 0., fg_cache: std::cell::RefCell::new((std::time::Instant::now() - std::time::Duration::from_secs(10), None)), _pump: pump }
        }))
    }

    /// 程序设置的标题;没有则为空。
    pub fn title(&self) -> String {
        self.term.snapshot().title
    }

    /// 标签名:`前台进程 · 当前目录`(参考 VS Code);进程名每 0.7 秒最多查一次。
    ///
    /// 参数:`fallback_folder` 标题里取不到目录时用的目录名(项目目录名)。
    pub fn label(&self, fallback_folder: &str) -> String {
        let mut cache = self.fg_cache.borrow_mut();
        if cache.0.elapsed() > std::time::Duration::from_millis(700) {
            *cache = (std::time::Instant::now(), self.term.foreground_pid().and_then(loom_term::label::process_name));
        }
        loom_term::label::tab_label(cache.1.as_deref(), &self.title(), fallback_folder)
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
        self.bounds = Some(bounds);
        let Some(cell_w) = self.cell_w else { return };
        let cols = (((bounds.size.width - px(2. * PAD)) / cell_w).floor().max(2.)) as u16;
        let rows = (((bounds.size.height - px(2. * PAD)) / px(self.metrics.1)).floor().max(1.)) as u16;
        if (cols, rows) != self.grid {
            self.grid = (cols, rows);
            self.term.resize(cols, rows);
            cx.notify();
        }
    }

    fn on_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let (k, m) = (&ev.keystroke, &ev.keystroke.modifiers);
        if m.platform {
            match k.key.as_str() {
                "v" => {
                    self.paste(cx);
                    cx.stop_propagation();
                }
                // ⌘C 复制选中文本;没有选区时不拦截,交还给应用
                "c" => {
                    if let Some(text) = self.term.selection_text() {
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                        cx.stop_propagation();
                    }
                }
                // 其余 ⌘ 组合键(⌘K、⌘B 等)也交还给应用
                _ => {}
            }
            return;
        }
        let mods = Mods { ctrl: m.control, alt: m.alt, shift: m.shift };
        let bytes = encode_key(&k.key, mods, self.term.app_cursor())
            .or_else(|| k.key_char.as_ref().filter(|_| !m.control && !m.alt).map(|s| s.clone().into_bytes()));
        if let Some(bytes) = bytes {
            // 一开始输入,旧选区就失效了
            self.term.clear_selection();
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

    /// 鼠标位置 → 终端格子;还没量出区域大小时返回 `None`。
    fn cell_under(&self, pos: Point<Pixels>) -> Option<crate::term_geom::CellPos> {
        let (b, cell_w) = (self.bounds?, self.cell_w?);
        let (x, y) = (f32::from(pos.x - b.origin.x), f32::from(pos.y - b.origin.y));
        Some(cell_at(x, y, f32::from(cell_w), self.metrics.1, PAD, self.grid.0 as usize, self.grid.1 as usize))
    }

    /// 按下左键:单击拖动选字符,双击选单词,三击选整行。
    fn begin_select(&mut self, ev: &MouseDownEvent, cx: &mut Context<Self>) {
        let Some(c) = self.cell_under(ev.position) else { return };
        let kind = match ev.click_count {
            0 | 1 => SelectKind::Simple,
            2 => SelectKind::Word,
            _ => SelectKind::Line,
        };
        self.term.start_selection(c.row, c.col, c.right, kind);
        self.selecting = true;
        self.dragged = false;
        cx.notify();
    }

    /// 按住左键拖动:更新选区终点。
    fn drag_select(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        if !self.selecting || ev.pressed_button != Some(MouseButton::Left) {
            return;
        }
        if let Some(c) = self.cell_under(ev.position) {
            self.dragged = true;
            self.term.update_selection(c.row, c.col, c.right);
            cx.notify();
        }
    }

    /// 松开左键:没拖动过的单击不留选区。
    fn end_select(&mut self, ev: &MouseUpEvent, cx: &mut Context<Self>) {
        if self.selecting && !self.dragged && ev.click_count <= 1 {
            self.term.clear_selection();
        }
        self.selecting = false;
        cx.notify();
    }

    /// 滚轮回看:像素累计满一行才滚一行,余数留到下次,小步增量不会被吞掉。
    ///
    /// 参数:`ev` 滚轮事件;`cx` 视图上下文。
    fn on_wheel(&mut self, ev: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let line_h = self.metrics.1;
        self.wheel_acc += f32::from(ev.delta.pixel_delta(px(line_h)).y);
        let lines = (self.wheel_acc / line_h).trunc();
        cx.stop_propagation();
        if lines != 0. {
            self.wheel_acc -= lines * line_h;
            self.term.scroll(lines as i32);
            cx.notify();
        }
    }
}

/// 字号 → (字号, 行高);行高取字号的 4/3 并取整,保证行与行之间不出现亚像素缝。
fn metrics_of(font_size: u8) -> (f32, f32) {
    let fs = f32::from(font_size.clamp(8, 28));
    (fs, (fs * 4. / 3.).round())
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
        let tp = crate::prefs(cx).terminal;
        let m = metrics_of(tp.font_size);
        if m != self.metrics {
            // 字号变了:字宽作废,下面重新量,容器尺寸由测量层在本帧重新换算行列数
            self.metrics = m;
            self.cell_w = None;
        }
        let (font_size, line_h) = m;
        if self.cell_w.is_none() {
            let font = Font { family: MONO.into(), features: FontFeatures::default(), fallbacks: None, weight: FontWeight::NORMAL, style: FontStyle::Normal };
            let id = window.text_system().resolve_font(&font);
            self.cell_w = window.text_system().advance(id, px(font_size), 'm').ok().map(|s| s.width);
        }
        let cell_w = self.cell_w.unwrap_or(px(7.));
        let snap = self.term.snapshot();
        let focused = self.focus.is_focused(window);
        let view = cx.entity();
        let lines = snap.lines.iter().map(|runs| {
            let (text, hl) = line_text(runs, t.foreground, t.background, dark);
            let row = div().h(px(line_h)).whitespace_nowrap();
            if text.is_empty() { row } else { row.child(StyledText::new(text).with_highlights(hl)) }
        });
        div().id("terminal").key_context("Terminal").track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| this.on_key(ev, cx)))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                window.focus(&this.focus, cx);
                this.begin_select(ev, cx);
            }))
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| this.drag_select(ev, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, ev: &MouseUpEvent, _, cx| this.end_select(ev, cx)))
            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| this.on_wheel(ev, cx)))
            .relative().size_full().overflow_hidden().bg(t.background).text_color(t.foreground)
            .font_family(MONO).text_size(px(font_size)).line_height(px(line_h))
            .child(div().absolute().top(px(PAD)).left(px(PAD)).flex().flex_col().children(lines))
            // 选区高亮:半透明色块盖在文字上
            .children(snap.selection.into_iter().flat_map(|sel| selection_spans(sel, snap.cols)).map(|(row, from, to)| {
                div().absolute().left(px(PAD) + cell_w * from as f32).top(px(PAD + line_h * row as f32))
                    .w(cell_w * (to - from + 1) as f32).h(px(line_h)).bg(t.primary.opacity(0.28))
            }))
            .when_some(snap.cursor, |d, (row, col)| {
                let (x, y) = (px(PAD) + cell_w * col as f32, px(PAD + line_h * row as f32));
                // 失焦时统一画成空心块;聚焦时按光标形状画
                let cursor = if !focused {
                    div().absolute().left(x).top(y).w(cell_w).h(px(line_h)).border_1().border_color(t.primary)
                } else {
                    let c = div().absolute().bg(t.primary.opacity(0.7));
                    match tp.cursor {
                        CursorShape::Block => c.left(x).top(y).w(cell_w).h(px(line_h)).bg(t.primary.opacity(0.6)),
                        CursorShape::Bar => c.left(x).top(y).w(px(2.)).h(px(line_h)),
                        CursorShape::Underline => c.left(x).top(y + px(line_h - 2.)).w(cell_w).h(px(2.)),
                    }
                };
                d.child(cursor)
            })
            .when_some(snap.exit_code, |d, code| d.child(
                div().absolute().bottom_0().left_0().right_0().px_2().py_1().bg(t.muted).text_color(t.muted_foreground)
                    .child(format!("进程已退出(退出码 {code}),可以关闭这个终端标签"))))
            // 隐形的测量层:拿到容器尺寸后换算列数 / 行数
            .child(canvas(move |bounds, _, cx| view.update(cx, |v, cx| v.apply_bounds(bounds, cx)), |_, _, _, _| {}).absolute().size_full())
    }
}
