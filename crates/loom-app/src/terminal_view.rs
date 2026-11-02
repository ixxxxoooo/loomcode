//! 终端视图:把 `loom-term` 的画面快照画出来,处理键盘、粘贴、滚轮和尺寸跟随。
//!
//! @author ixxxxoooo

use crate::term_geom::cell_at;
use crate::term_paint::GridPaint;
use crate::term_palette::indexed_rgb;
use crate::ui::MONO;
use gpui_kit::component::ActiveTheme;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::sync::Arc;
use loom_term::{Mods, MouseButton as TermButton, SelectKind, SpawnOptions, TermEvent, Terminal, encode_key};

/// 终端内容距边框的留白
const PAD: f32 = 6.;

/// 响铃时边框闪烁的时长
const BELL_FLASH: std::time::Duration = std::time::Duration::from_millis(180);

/// 一个终端的视图。
pub struct TerminalView {
    pub(crate) term: Arc<Terminal>,
    pub(crate) focus: FocusHandle,
    /// 启动目录(相对路径的链接按它解析)
    pub(crate) cwd: String,
    /// 程序接管鼠标时,当前被按住并已上报的键
    pub(crate) reported: Option<TermButton>,
    /// 最近一次上报鼠标移动时所在的格子 (行, 列),同一格内不重复发
    pub(crate) last_cell: Option<(usize, usize)>,
    /// 上次渲染时终端是否有焦点(焦点进出上报用)
    pub(crate) last_focus: bool,
    /// 搜索栏状态(⌘F);`None` 表示没打开
    pub(crate) find: Option<crate::term_search::FindState>,
    /// 响铃闪烁到什么时候
    bell_until: Option<std::time::Instant>,
    /// 当前字号与行高(随设置变化,变了就重新量字宽)
    pub(crate) metrics: (f32, f32),
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
    pub(crate) wheel_acc: f32,
    /// 负责决定终端尺寸的窗口(同一个终端被两个窗口同时显示时,只听它的,避免 SIGWINCH 来回抖动)及其最近一次绘制时间
    size_owner: Option<(WindowId, std::time::Instant)>,
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
        let opts_cwd = cwd.clone();
        let opts = SpawnOptions {
            cwd: Some(cwd),
            command,
            cols: 80,
            rows: 24,
            shell: Some(tp.shell.clone()),
            no_login: !tp.login_shell,
            scrollback: Some(tp.scrollback as usize),
            theme: Some(term_theme(cx.theme())),
        };
        let term = Terminal::spawn(opts, move || {
            let _ = tx.try_send(());
        })?;
        let term = Arc::new(term);
        let start_cwd = opts_cwd.clone();
        Ok(cx.new(|cx| {
            let events_term = term.clone();
            let pump = cx.spawn(async move |this, cx| {
                while rx.recv().await.is_ok() {
                    let evs = events_term.drain_events();
                    if this.update(cx, |this: &mut Self, cx| this.on_term_events(evs, cx)).is_err() {
                        break;
                    }
                }
            });
            // 调试用:LOOM_TERM_SELECT=起点行,起点列,终点行,终点列,一秒后自动选中这一段,方便截图检查高亮
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
            Self { term, focus: cx.focus_handle(), cwd: start_cwd, reported: None, last_cell: None, last_focus: false, find: None, bell_until: None, metrics: metrics_of(tp.font_size), cell_w: None, grid: (80, 24), bounds: None, size_owner: None, selecting: false, dragged: false, wheel_acc: 0., fg_cache: std::cell::RefCell::new((std::time::Instant::now() - std::time::Duration::from_secs(10), None)), _pump: pump }
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

    /// 是否有命令正在这个终端里运行(退出 / 重启前提醒用)。
    pub fn is_busy(&self) -> bool {
        self.term.is_busy()
    }

    /// 这个窗口现在能不能决定终端尺寸:没有负责窗口、自己就是负责窗口、自己有键盘焦点、或负责窗口已经半秒没画过(关了 / 切走了)就可以。
    ///
    /// 参数:`win` 当前绘制的窗口;`focused` 终端在该窗口里是否有焦点。
    fn claim_size(&mut self, win: WindowId, focused: bool) -> bool {
        let now = std::time::Instant::now();
        let ok = match self.size_owner {
            None => true,
            Some((w, seen)) => w == win || focused || now.duration_since(seen) > std::time::Duration::from_millis(500),
        };
        if ok {
            self.size_owner = Some((win, now));
        }
        ok
    }

    /// 由容器尺寸换算成列数 / 行数并通知终端调整大小。
    fn apply_bounds(&mut self, bounds: Bounds<Pixels>, win: WindowId, focused: bool, cx: &mut Context<Self>) {
        if !self.claim_size(win, focused) {
            return;
        }
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

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let (k, m) = (&ev.keystroke, &ev.keystroke.modifiers);
        if m.platform {
            match k.key.as_str() {
                // ⌘F 搜索;搜索栏开着时 ⌘G / ⌘⇧G 跳下一个 / 上一个
                "f" => {
                    self.toggle_find(window, cx);
                    cx.stop_propagation();
                }
                "g" if self.find.is_some() => {
                    self.step_find(if m.shift { -1 } else { 1 }, cx);
                    cx.stop_propagation();
                }
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
    pub(crate) fn cell_under(&self, pos: Point<Pixels>) -> Option<crate::term_geom::CellPos> {
        let (b, cell_w) = (self.bounds?, self.cell_w?);
        let (x, y) = (f32::from(pos.x - b.origin.x), f32::from(pos.y - b.origin.y));
        Some(cell_at(x, y, f32::from(cell_w), self.metrics.1, PAD, self.grid.0 as usize, self.grid.1 as usize))
    }

    /// 按下左键:单击拖动选字符,双击选单词,三击选整行。
    pub(crate) fn begin_select(&mut self, ev: &MouseDownEvent, cx: &mut Context<Self>) {
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
    pub(crate) fn drag_select(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
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
    pub(crate) fn end_select(&mut self, ev: &MouseUpEvent, cx: &mut Context<Self>) {
        if self.selecting && !self.dragged && ev.click_count <= 1 {
            self.term.clear_selection();
        }
        self.selecting = false;
        cx.notify();
    }

    /// 处理终端事件:响铃闪一下边框,OSC 52 写系统剪贴板。
    fn on_term_events(&mut self, events: Vec<TermEvent>, cx: &mut Context<Self>) {
        for ev in events {
            match ev {
                TermEvent::Bell => {
                    self.bell_until = Some(std::time::Instant::now() + BELL_FLASH);
                    cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(BELL_FLASH).await;
                        let _ = this.update(cx, |_, cx| cx.notify());
                    })
                    .detach();
                }
                TermEvent::ClipboardStore(text) => cx.write_to_clipboard(ClipboardItem::new_string(text)),
            }
        }
        cx.notify();
    }
}

/// 界面主题 → 终端主题(前景、背景、调色板与界面绘制用的同一套)。
fn term_theme(t: &gpui_kit::component::Theme) -> loom_term::TermTheme {
    let c = |h: Hsla| { let r = h.to_rgb(); ((r.r * 255.).round() as u8, (r.g * 255.).round() as u8, (r.b * 255.).round() as u8) };
    let dark = t.is_dark();
    loom_term::TermTheme { fg: c(t.foreground), bg: c(t.background), dark, palette: Arc::new(move |i| indexed_rgb(i, dark)) }
}

/// 字号 → (字号, 行高);行高取字号的 4/3 并取整,保证行与行之间不出现亚像素缝。
fn metrics_of(font_size: u8) -> (f32, f32) {
    let fs = f32::from(font_size.clamp(8, 28));
    (fs, (fs * 4. / 3.).round())
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
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
        // 把当前主题同步给终端,程序查询背景色时才答得对(浅色主题下 opencode 等才会用浅色)
        self.term.set_theme(term_theme(&t));
        let focused = self.focus.is_focused(window);
        self.sync_focus_report(focused);
        self.refresh_find(cx);
        let snap = self.term.snapshot();
        let matches = self.visible_matches(&snap);
        let exit_code = snap.exit_code;
        let grid = GridPaint {
            snap, cell_w, line_h, font_size, pad: PAD, dark: t.is_dark(), fg: t.foreground, bg: t.background,
            accent: t.primary, find: t.warning, cursor: tp.cursor, focused, matches,
        };
        let bell = self.bell_until.is_some_and(|b| b > std::time::Instant::now());
        let view = cx.entity();
        div().id("terminal").key_context("Terminal").track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| this.on_key(ev, window, cx)))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, ev: &MouseDownEvent, window, cx| this.mouse_down(MouseButton::Left, ev, window, cx)))
            .on_mouse_down(MouseButton::Right, cx.listener(|this, ev: &MouseDownEvent, window, cx| this.mouse_down(MouseButton::Right, ev, window, cx)))
            .on_mouse_down(MouseButton::Middle, cx.listener(|this, ev: &MouseDownEvent, window, cx| this.mouse_down(MouseButton::Middle, ev, window, cx)))
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| this.mouse_move(ev, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, ev: &MouseUpEvent, _, cx| this.mouse_up(MouseButton::Left, ev, cx)))
            .on_mouse_up(MouseButton::Right, cx.listener(|this, ev: &MouseUpEvent, _, cx| this.mouse_up(MouseButton::Right, ev, cx)))
            .on_mouse_up(MouseButton::Middle, cx.listener(|this, ev: &MouseUpEvent, _, cx| this.mouse_up(MouseButton::Middle, ev, cx)))
            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| this.on_wheel(ev, cx)))
            .relative().size_full().overflow_hidden().bg(t.background).text_color(t.foreground)
            .font_family(MONO).text_size(px(font_size)).line_height(px(line_h))
            // 整个网格在一个画布里按格子坐标画出来
            .child(canvas(|_, _, _| (), move |bounds, _, window, cx| grid.paint(bounds, window, cx)).absolute().size_full())
            .when(bell, |d| d.child(div().absolute().size_full().border_2().border_color(t.warning.opacity(0.7))))
            .when_some(exit_code, |d, code| d.child(
                div().absolute().bottom_0().left_0().right_0().px_2().py_1().bg(t.muted).text_color(t.muted_foreground)
                    .child(format!("进程已退出(退出码 {code}),可以关闭这个终端标签"))))
            .children(self.find_bar(cx))
            // 隐形的测量层:拿到容器尺寸后换算列数 / 行数
            .child(canvas(move |bounds, window, cx| { let (win, focused) = (window.window_handle().window_id(), view.read(cx).focus.is_focused(window)); view.update(cx, |v, cx| v.apply_bounds(bounds, win, focused, cx)) }, |_, _, _, _| {}).absolute().size_full())
    }
}
