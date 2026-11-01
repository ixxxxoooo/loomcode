//! 输入区:多行输入、斜杠 / @ 补全、模型 / 模式 / 强度选择、发送 / 停止 / 排队。

use crate::model::SessionModel;
use crate::ui::{icon, pill};
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Indent, InputEvent, MoveDown, MoveUp, Textarea, TextareaState};
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::*;

actions!(composer, [CycleEffort]);

/// 注册输入框快捷键(⌥T 循环思考强度);全局只注册一次。
fn bind_keys(cx: &mut App) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| cx.bind_keys([KeyBinding::new("alt-t", CycleEffort, Some("Composer"))]));
}

/// 补全项。
#[derive(Clone)]
struct Suggestion {
    /// 插入的文本
    insert: String,
    /// 显示的主文本
    label: String,
    detail: String,
}

/// 弹出的选择列表种类。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pick {
    Model,
    Mode,
    /// 上下文用量面板
    Context,
}

/// Claude Code 内置斜杠命令。
const BUILTIN_COMMANDS: &[(&str, &str)] = &[
    ("/compact", "压缩上下文"),
    ("/clear", "清空会话"),
    ("/model", "切换模型"),
    ("/init", "生成 CLAUDE.md"),
    ("/review", "审查代码"),
    ("/cost", "查看花费"),
    ("/context", "查看上下文用量"),
    ("/mcp", "管理 MCP 服务器"),
    ("/memory", "编辑记忆"),
    ("/help", "帮助"),
];

/// 待发送的图片附件。
#[derive(Clone)]
struct Attachment {
    /// 显示名
    name: String,
    /// 落盘路径(发送时作为图片路径)
    path: String,
}

pub struct Composer {
    pub(crate) session: Entity<SessionModel>,
    pub(crate) input: Entity<TextareaState>,
    open: Option<Pick>,
    /// 刚被「点击外部」关闭的时间,用来避免点触发按钮时又立刻重新打开
    outside_closed: Option<std::time::Instant>,
    suggestions: Vec<Suggestion>,
    /// 补全列表当前高亮项
    sel: usize,
    attachments: Vec<Attachment>,
    attach_counter: usize,
    pub(crate) demo: bool,
    /// 优化提示词的运行环境(真实模式由工作台注入)
    pub(crate) rewrite: Option<crate::composer_send::RewriteCtx>,
    /// 正在优化提示词
    pub(crate) optimizing: bool,
    /// 最近一次优化失败的原因,下次输入变化时清除
    pub(crate) optimize_error: Option<String>,
    /// 自定义斜杠命令与 Skills:(`/名称`, 描述),由工作台扫描 `.claude` 后注入
    custom_commands: Vec<(String, String)>,
    /// 自定义子 Agent 名称(`@` 补全用)
    custom_agents: Vec<String>,
    _sub: Subscription,
}

/// 从文本里提取 `@路径` 形式的文件引用(含 `/` 或 `.` 才视为文件)。
pub fn extract_file_mentions(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for w in text.split_whitespace() {
        if let Some(p) = w.strip_prefix('@') {
            // 遇到标点就截断,如 `@a.rs,然后`
            let end = p.find([',', '，', '。', ';', '；', ')', '）', '、']).unwrap_or(p.len());
            let p = &p[..end];
            if (p.contains('/') || p.contains('.')) && !out.iter().any(|x| x == p) {
                out.push(p.to_string());
            }
        }
    }
    out
}

impl Composer {
    pub fn new(session: Entity<SessionModel>, demo: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        bind_keys(cx);
        let input = cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 8).placeholder("向 Claude 提问,/ 调用命令,@ 引用文件…").submit_on_enter(true));
        let sub = cx.subscribe_in(&input, window, |this: &mut Self, _, ev: &InputEvent, window, cx| match ev {
            InputEvent::PressEnter { shift: false, secondary } => {
                // 设置为「⌘Enter 发送」时,普通 Enter 只换行
                let cmd_send = cx.try_global::<crate::UiPrefs>().is_some_and(|p| p.cmd_enter_send);
                if this.suggestions.is_empty() && cmd_send && !*secondary {
                    this.input.update(cx, |i, cx| i.insert("\n", window, cx));
                } else if this.suggestions.is_empty() {
                    this.submit(window, cx)
                } else {
                    this.accept_suggestion(this.sel, window, cx)
                }
            }
            InputEvent::Change => {
                this.optimize_error = None;
                this.refresh_suggestions(cx)
            }
            _ => {}
        });
        // 调试用:LOOM_POPUP=model|context 启动即打开对应弹层(只用于截图验证)
        let open = match std::env::var("LOOM_POPUP").as_deref() {
            Ok("model") => Some(Pick::Model),
            Ok("context") => Some(Pick::Context),
            Ok("mode") => Some(Pick::Mode),
            _ => None,
        };
        let mut this = Self { session, input, open, outside_closed: None, suggestions: Vec::new(), sel: 0, attachments: Vec::new(), attach_counter: 0, demo, rewrite: None, optimizing: false, optimize_error: None, custom_commands: Vec::new(), custom_agents: Vec::new(), _sub: sub };
        // 调试用:LOOM_ATTACH=图片路径 启动即带一张附件,方便截图检查缩略图
        if let Ok(p) = std::env::var("LOOM_ATTACH") {
            this.add_image_path(std::path::Path::new(&p), cx);
        }
        this
    }

    /// 输入框里有没有未发送的内容(文字或附件);淘汰会话视图前要保护草稿。
    pub fn has_draft(&self, cx: &App) -> bool {
        !self.input.read(cx).value().trim().is_empty() || !self.attachments.is_empty()
    }

    /// 把一个文件引用(如 `src/a.rs#5-10`)以 `@` 提及的形式追加到输入框末尾并聚焦。
    pub fn insert_reference(&mut self, reference: &str, window: &mut Window, cx: &mut Context<Self>) {
        let cur = self.input.read(cx).value().to_string();
        let sep = if cur.is_empty() || cur.ends_with(' ') || cur.ends_with('\n') { "" } else { " " };
        self.set_text(&format!("{cur}{sep}@{reference} "), window, cx);
        self.focus(window, cx);
    }

    /// 聚焦输入框。
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |i, cx| i.focus(window, cx));
    }

    /// 直接设置文本(命令面板「演示场景」使用)。
    pub fn set_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |i, cx| i.set_value(text.to_string(), window, cx));
        // set_value 不会触发 Change 事件,手动刷新补全
        self.refresh_suggestions(cx);
    }

    /// 在光标处插入文字(从设置页「在对话中使用」调用),并刷新补全。
    pub fn insert_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |i, cx| i.insert(text.to_string(), window, cx));
        self.refresh_suggestions(cx);
    }

    /// 注入自定义命令 / Skills 与子 Agent(工作台扫描 `.claude` 目录后调用)。
    ///
    /// 参数:`commands` (`/名称`, 描述) 列表;`agents` 子 Agent 名称。
    pub fn set_extensions(&mut self, commands: Vec<(String, String)>, agents: Vec<String>) {
        self.custom_commands = commands;
        self.custom_agents = agents;
    }

    pub fn refresh_suggestions(&mut self, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value().to_string();
        let mut out = Vec::new();
        if let Some(q) = text.strip_prefix('/') {
            if !q.contains(' ') {
                for (c, d) in BUILTIN_COMMANDS {
                    if c[1..].starts_with(q) {
                        out.push(Suggestion { insert: format!("{c} "), label: c.to_string(), detail: d.to_string() });
                    }
                }
                for (c, d) in &self.custom_commands {
                    if c[1..].starts_with(q) && !out.iter().any(|x: &Suggestion| x.label == *c) {
                        out.push(Suggestion { insert: format!("{c} "), label: c.clone(), detail: d.clone() });
                    }
                }
                if self.demo {
                    for s in loom_demo::scenarios::all() {
                        let cmd = format!("/demo-{}", s.id);
                        if cmd[1..].starts_with(q) {
                            out.push(Suggestion { insert: format!("{cmd} "), label: cmd, detail: s.title.to_string() });
                        }
                    }
                }
            }
        } else if let Some(at) = text.rfind('@') {
            let q = &text[at + 1..];
            if !q.contains(' ') {
                for f in ["src/main.rs", "src/router.rs", "src/routes.rs", "Cargo.toml", "README.md"] {
                    if f.to_lowercase().contains(&q.to_lowercase()) {
                        out.push(Suggestion { insert: format!("{}@{f} ", &text[..at]), label: format!("@{f}"), detail: "文件".into() });
                    }
                }
                let builtin = ["Explore", "Plan", "general-purpose"].map(str::to_string);
                for a in builtin.iter().chain(self.custom_agents.iter()) {
                    if a.to_lowercase().starts_with(&q.to_lowercase()) {
                        out.push(Suggestion { insert: format!("{}@{a} ", &text[..at]), label: format!("@{a}"), detail: "子 Agent".into() });
                    }
                }
            }
        }
        out.truncate(8);
        self.suggestions = out;
        self.sel = 0;
        cx.notify();
    }

    /// 接受第 `ix` 项补全。
    fn accept_suggestion(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(s) = self.suggestions.get(ix).cloned() {
            self.set_text(&s.insert, window, cx);
            self.suggestions.clear();
            self.focus(window, cx);
            cx.notify();
        }
    }

    /// 移动补全高亮(循环)。返回是否消费了按键。
    fn move_sel(&mut self, delta: isize, cx: &mut Context<Self>) -> bool {
        if self.suggestions.is_empty() {
            return false;
        }
        let n = self.suggestions.len() as isize;
        self.sel = ((self.sel as isize + delta).rem_euclid(n)) as usize;
        cx.notify();
        true
    }

    /// 添加图片附件:落盘到临时目录,发送时以路径传给 Claude。
    fn add_image(&mut self, bytes: &[u8], ext: &str, cx: &mut Context<Self>) {
        self.attach_counter += 1;
        let dir = std::env::temp_dir().join("loom-attachments");
        let _ = std::fs::create_dir_all(&dir);
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        let path = dir.join(format!("{stamp}-{}.{ext}", self.attach_counter));
        if std::fs::write(&path, bytes).is_ok() {
            self.attachments.push(Attachment { name: format!("图片 {}", self.attach_counter), path: path.to_string_lossy().to_string() });
            cx.notify();
        }
    }

    /// 添加一个已存在的图片文件。
    fn add_image_path(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        self.attach_counter += 1;
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| format!("图片 {}", self.attach_counter));
        self.attachments.push(Attachment { name, path: path.to_string_lossy().to_string() });
        cx.notify();
    }

    pub(crate) fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value().trim().to_string();
        if text.is_empty() && self.attachments.is_empty() {
            return;
        }
        self.input.update(cx, |i, cx| i.set_value(String::new(), window, cx));
        self.suggestions.clear();
        let images: Vec<String> = self.attachments.drain(..).map(|a| a.path).collect();
        let files = extract_file_mentions(&text);
        let running = self.session.read(cx).state.phase == Phase::Running;
        self.session.update(cx, |m, cx| {
            if running && images.is_empty() {
                m.queue_prompt(text, cx)
            } else {
                m.send_prompt_with(text, images, files, cx)
            }
        });
        cx.notify();
    }

    fn pick_list(&self, kind: Pick, cx: &App) -> Vec<(String, String, bool, String)> {
        let st = &self.session.read(cx).state;
        match kind {
            // 模型与上下文有各自的弹层(composer_pop),这里只剩权限模式的简单列表
            Pick::Model | Pick::Context => Vec::new(),
            Pick::Mode => PermissionMode::ALL.iter().map(|m| (m.label().to_string(), m.cli_value().to_string(), *m == st.mode, m.cli_value().to_string())).collect(),
        }
    }

    fn choose(&mut self, kind: Pick, value: &str, cx: &mut Context<Self>) {
        self.open = None;
        if kind == Pick::Mode
            && let Some(mode) = PermissionMode::from_cli(value)
        {
            self.session.update(cx, |m, cx| m.set_mode(mode, cx));
        }
        cx.notify();
    }

    /// 选择模型并关闭弹层。
    pub(crate) fn choose_model(&mut self, id: &str, cx: &mut Context<Self>) {
        self.open = None;
        self.session.update(cx, |m, cx| m.set_model(id.to_string(), cx));
        cx.notify();
    }

    /// 点击弹层外部:关闭弹层并记下时间。
    fn dismiss_outside(&mut self, cx: &mut Context<Self>) {
        self.open = None;
        self.outside_closed = Some(std::time::Instant::now());
        cx.notify();
    }

    /// 点触发按钮:已打开则关闭,否则打开(刚被外部点击关闭的同一次点击不再重开)。
    fn toggle(&mut self, kind: Pick, cx: &mut Context<Self>) {
        let just_closed = self.outside_closed.take().is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(250));
        self.open = if self.open == Some(kind) || just_closed { None } else { Some(kind) };
        cx.notify();
    }

    /// 设置思考强度并关闭弹层。
    pub(crate) fn choose_effort(&mut self, effort: Effort, cx: &mut Context<Self>) {
        self.open = None;
        self.session.update(cx, |m, cx| m.set_effort(effort, cx));
        cx.notify();
    }

    /// 循环切换思考强度(⌥T),下一轮对话生效。
    pub(crate) fn cycle_effort(&mut self, cx: &mut Context<Self>) {
        let cur = self.session.read(cx).state.effort;
        let ix = Effort::ALL.iter().position(|e| *e == cur).unwrap_or(0);
        self.choose_effort(Effort::ALL[(ix + 1) % Effort::ALL.len()], cx);
    }

    /// 压缩上下文:作为斜杠命令发给 Claude(`/compact`)。
    pub(crate) fn compact(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.open = None;
        self.session.update(cx, |m, cx| m.send_prompt_with("/compact".into(), Vec::new(), Vec::new(), cx));
        cx.notify();
    }

    /// 关闭当前弹层。
    pub(crate) fn close_pop(&mut self, cx: &mut Context<Self>) {
        self.open = None;
        cx.notify();
    }
}

impl Render for Composer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let (phase, mode, model_id, model_name, effort, queued, usage) = {
            let m = self.session.read(cx);
            let name = m.state.models.iter().find(|x| x.id == m.state.model).map(|x| x.name.clone()).unwrap_or_else(|| m.state.model.clone());
            (m.state.phase, m.state.mode, m.state.model.clone(), name, m.state.effort, m.queued.len(), m.state.usage.clone())
        };
        let running = phase == Phase::Running;
        let mode_color = crate::mode_style::mode_color(mode, cx);
        let this = cx.entity();

        let chip = |id: &'static str, label: String, color: Hsla, kind: Pick, lead: Option<AnyElement>, this: Entity<Self>| {
            h_flex().id(id).h(px(24.)).px_2().gap_1().items_center().rounded(px(6.)).cursor_pointer().text_size(px(12.))
                .text_color(color).hover(|d| d.bg(t.list_hover))
                .on_click(move |_, _, cx| { this.update(cx, |c, cx| c.toggle(kind, cx)); })
                .when_some(lead, |d, l| d.child(l))
                .child(label)
                .child(icon(IconName::ChevronDown, 10., color))
        };

        let popup = self.open.map(|kind| match kind {
            Pick::Model => {
                let (models, current) = { let st = &self.session.read(cx).state; (st.models.clone(), st.model.clone()) };
                crate::composer_pop::model_popup(&models, &current, effort, this.clone(), cx)
            }
            Pick::Context => crate::composer_pop::context_popup(&usage, this.clone(), cx),
            Pick::Mode => {
                let items = self.pick_list(kind, cx);
                let this = this.clone();
                v_flex().absolute().bottom(px(44.)).left(px(8.)).min_w(px(200.)).p_1().rounded(px(10.)).border_1().border_color(t.border)
                    .bg(t.popover).shadow_lg().children(items.into_iter().enumerate().map(|(i, (label, detail, on, value))| {
                        let this = this.clone();
                        // 每个模式一个带颜色的图标;当前模式的文字也用它的颜色,右侧打勾
                        let m = PermissionMode::from_cli(&value).unwrap_or_default();
                        let color = crate::mode_style::mode_color(m, cx);
                        h_flex().id(SharedString::from(format!("pick-{i}"))).gap_2().px_2().h(px(30.)).items_center().rounded(px(6.)).cursor_pointer()
                            .hover(|d| d.bg(t.list_hover))
                            .on_click(move |_, _, cx| { let v = value.clone(); this.update(cx, |c, cx| c.choose(kind, &v, cx)); })
                            .child(icon(crate::mode_style::mode_icon(m), 14., color))
                            .child(div().text_size(px(13.)).when(on, |d| d.text_color(color).font_weight(FontWeight::MEDIUM)).child(label))
                            .child(div().flex_1())
                            .child(div().text_size(px(11.)).text_color(t.muted_foreground).child(detail))
                            .child(div().w(px(14.)).child(if on { icon(IconName::Check, 12., color).into_any_element() } else { div().into_any_element() }))
                    }))
            }
        });

        let suggestions = (!self.suggestions.is_empty()).then(|| {
            let this = this.clone();
            v_flex().absolute().bottom(px(112.)).left(px(0.)).w_full().max_h(px(260.)).overflow_hidden().p_1().rounded(px(10.))
                .border_1().border_color(t.border).bg(t.popover).shadow_lg()
                .children(self.suggestions.iter().enumerate().map(|(i, s)| {
                    let this = this.clone();
                    let (label, detail) = (s.label.clone(), s.detail.clone());
                    h_flex().id(SharedString::from(format!("sug-{i}"))).gap_3().px_2().h(px(28.)).items_center().rounded(px(6.)).cursor_pointer()
                        .when(i == self.sel, |d| d.bg(t.list_active))
                        .hover(|d| d.bg(t.list_hover))
                        .on_click(move |_, window, cx| {
                            this.update(cx, |c, cx| c.accept_suggestion(i, window, cx));
                        })
                        .child(div().font_family(crate::ui::MONO).text_size(px(12.)).child(label))
                        .child(div().text_size(px(12.)).text_color(t.muted_foreground).truncate().child(detail))
                }))
        });

        let has_text = !self.input.read(cx).value().trim().is_empty();
        let send_btn = {
            let (s, this) = (self.session.clone(), this.clone());
            crate::composer_send::send_button(running, has_text || !self.attachments.is_empty(), move |window, cx| {
                if running { s.update(cx, |m, cx| m.cancel(cx)); } else { this.update(cx, |c, cx| c.submit(window, cx)); }
            }, cx)
        };
        let optimize_btn = {
            let this = this.clone();
            crate::composer_send::optimize_button(has_text, self.optimizing, move |window, cx| { this.update(cx, |c, cx| c.optimize(window, cx)); }, cx)
        };

        let paste_weak = cx.entity().downgrade();
        // 附件缩略图:点击预览大图,悬停右上角出现删除按钮
        let attach_chips = (!self.attachments.is_empty()).then(|| {
            h_flex().flex_wrap().gap_2().px_3().pt_3().children(self.attachments.iter().enumerate().map(|(i, a)| {
                let this = this.clone();
                crate::image_viewer::thumbnail(&format!("att-{i}-{}", a.path), &a.path, &a.name, 56., Some(Box::new(move |_, cx| {
                    this.update(cx, |c, cx| { if i < c.attachments.len() { c.attachments.remove(i); } cx.notify(); });
                })), cx)
            }))
        });

        div().key_context("Composer").on_action(cx.listener(|this, _: &CycleEffort, _, cx| this.cycle_effort(cx)))
            .w_full().flex().justify_center().px_4().pb_3().pt_1()
            // 补全列表打开时,上下键 / Tab / Esc 归补全列表;否则放行给输入框
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| { if !this.move_sel(-1, cx) { cx.propagate(); } }))
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| { if !this.move_sel(1, cx) { cx.propagate(); } }))
            .capture_action(cx.listener(|this, _: &Indent, window, cx| {
                if this.suggestions.is_empty() { cx.propagate(); } else { this.accept_suggestion(this.sel, window, cx); }
            }))
            .capture_action(cx.listener(|this, _: &crate::workbench::Escape, _, cx| {
                if this.suggestions.is_empty() && this.open.is_none() {
                    // 回合运行中按 Esc 中断(与等待行的「按 Esc 中断」提示一致)
                    let running = this.session.read(cx).state.phase == loom_core::Phase::Running;
                    if running { this.session.update(cx, |m, cx| m.cancel(cx)); } else { cx.propagate(); }
                } else { this.suggestions.clear(); this.open = None; cx.notify(); }
            }))
            // 从文件树拖进来:引用 @相对路径
            .on_drop(cx.listener(|this, d: &crate::file_dnd::DraggedPath, window, cx| {
                let cur = this.input.read(cx).value().to_string();
                let sep = if cur.is_empty() || cur.ends_with(' ') || cur.ends_with('\n') { "" } else { " " };
                this.set_text(&format!("{cur}{sep}@{} ", d.rel), window, cx);
            }))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                for p in paths.paths() {
                    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
                    if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp") {
                        this.add_image_path(p, cx);
                    } else {
                        let cur = this.input.read(cx).value().to_string();
                        let sep = if cur.is_empty() || cur.ends_with(' ') { "" } else { " " };
                        this.set_text(&format!("{cur}{sep}@{} ", p.display()), window, cx);
                    }
                }
            }))
            .child(
            v_flex().relative().w_full().max_w(px(crate::timeline::content_max_w(cx)))
                .when_some(suggestions, |d, s| d.child(s))
                .child(
                    v_flex().w_full().rounded(px(14.)).border_1().border_color(t.border).bg(t.popover).shadow_sm()
                        .when_some(attach_chips, |d, c| d.child(c))
                        .child(div().px_3().pt_2().child(Textarea::new(&self.input).appearance(false).bordered(false).on_paste(move |item, _, cx| {
                            // 粘贴图片:作为附件;其余(文本)走默认逻辑
                            for entry in item.entries() {
                                if let ClipboardEntry::Image(img) = entry {
                                    let ext = match img.format { ImageFormat::Jpeg => "jpg", ImageFormat::Webp => "webp", ImageFormat::Gif => "gif", _ => "png" };
                                    let bytes = img.bytes.clone();
                                    let _ = paste_weak.update(cx, |c, cx| c.add_image(&bytes, ext, cx));
                                    return true;
                                }
                            }
                            false
                        })))
                        .child(
                            h_flex().h(px(36.)).px_2().gap_1().items_center()
                                .child(chip("chip-mode", mode.label().to_string(), mode_color, Pick::Mode, Some(icon(crate::mode_style::mode_icon(mode), 13., mode_color).into_any_element()), this.clone()))
                                .child(chip("chip-model", format!("{model_name} · {}", effort.label()), t.foreground, Pick::Model, Some(crate::brand_icons::brand_icon(&model_id, &model_name, 13., t.foreground)), this.clone()))
                                .when(queued > 0, |d| d.child(pill(format!("排队 {queued}"), t.warning, t.muted)))
                                .child(div().flex_1())
                                .when(usage.context_size > 0, |d| {
                                    let this = this.clone();
                                    d.child(crate::composer_pop::context_ring(&usage, move |cx| { this.update(cx, |c, cx| c.toggle(Pick::Context, cx)); }, cx))
                                })
                                .when_some(self.optimize_error.clone(), |d, e| d.child(div().text_size(px(11.)).text_color(t.danger).truncate().child(e)))
                                .child(optimize_btn)
                                .child(send_btn),
                        ),
                )
                .when_some(popup, |d, p| {
                    let me = this.clone();
                    d.child(p.on_mouse_down_out(move |_, _, cx| { me.update(cx, |c, cx| c.dismiss_outside(cx)); }))
                }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 提取文件引用() {
        let f = extract_file_mentions("看 @src/main.rs 和 @Cargo.toml,还有 @Explore 以及 @src/main.rs");
        assert_eq!(f, vec!["src/main.rs", "Cargo.toml"]);
    }
}
