//! 输入区:多行输入、斜杠 / @ 补全、模型 / 模式 / 强度选择、发送 / 停止 / 排队。

use crate::model::SessionModel;
use crate::ui::{icon, pill};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Indent, InputEvent, MoveDown, MoveUp, Textarea, TextareaState};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ucode_core::*;

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
    Effort,
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
    session: Entity<SessionModel>,
    input: Entity<TextareaState>,
    open: Option<Pick>,
    suggestions: Vec<Suggestion>,
    /// 补全列表当前高亮项
    sel: usize,
    attachments: Vec<Attachment>,
    attach_counter: usize,
    demo: bool,
    _sub: Subscription,
}

/// 从文本里提取 `@路径` 形式的文件引用(含 `/` 或 `.` 才视为文件)。
pub fn extract_file_mentions(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for w in text.split_whitespace() {
        if let Some(p) = w.strip_prefix('@') {
            let p = p.trim_end_matches([',', '。', ';', ')', ',']);
            if (p.contains('/') || p.contains('.')) && !out.iter().any(|x| x == p) {
                out.push(p.to_string());
            }
        }
    }
    out
}

impl Composer {
    pub fn new(session: Entity<SessionModel>, demo: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 8).placeholder("向 Claude 提问,/ 调用命令,@ 引用文件…").submit_on_enter(true));
        let sub = cx.subscribe_in(&input, window, |this: &mut Self, _, ev: &InputEvent, window, cx| match ev {
            InputEvent::PressEnter { shift: false, .. } => {
                if this.suggestions.is_empty() {
                    this.submit(window, cx)
                } else {
                    this.accept_suggestion(this.sel, window, cx)
                }
            }
            InputEvent::Change => this.refresh_suggestions(cx),
            _ => {}
        });
        Self { session, input, open: None, suggestions: Vec::new(), sel: 0, attachments: Vec::new(), attach_counter: 0, demo, _sub: sub }
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
                if self.demo {
                    for s in ucode_demo::scenarios::all() {
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
                for a in ["Explore", "code-reviewer", "general-purpose"] {
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
        let dir = std::env::temp_dir().join("ucode-attachments");
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

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
            Pick::Model => st.models.iter().map(|m| (m.name.clone(), m.description.clone(), m.id == st.model, m.id.clone())).collect(),
            Pick::Mode => PermissionMode::ALL.iter().map(|m| (m.label().to_string(), m.cli_value().to_string(), *m == st.mode, m.cli_value().to_string())).collect(),
            Pick::Effort => Effort::ALL.iter().map(|e| (e.label().to_string(), e.cli_value().to_string(), *e == st.effort, e.cli_value().to_string())).collect(),
        }
    }

    fn choose(&mut self, kind: Pick, value: &str, cx: &mut Context<Self>) {
        self.open = None;
        self.session.update(cx, |m, cx| match kind {
            Pick::Model => m.set_model(value.to_string(), cx),
            Pick::Mode => {
                if let Some(mode) = PermissionMode::from_cli(value) {
                    m.set_mode(mode, cx)
                }
            }
            Pick::Effort => {
                if let Some(e) = Effort::ALL.into_iter().find(|e| e.cli_value() == value) {
                    m.set_effort(e, cx)
                }
            }
        });
        cx.notify();
    }
}

impl Render for Composer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let (phase, mode, model_name, effort, queued, usage) = {
            let m = self.session.read(cx);
            let name = m.state.models.iter().find(|x| x.id == m.state.model).map(|x| x.name.clone()).unwrap_or_else(|| m.state.model.clone());
            (m.state.phase, m.state.mode, name, m.state.effort, m.queued.len(), m.state.usage.clone())
        };
        let running = phase == Phase::Running;
        let mode_color = match mode {
            PermissionMode::Plan => t.info,
            PermissionMode::AcceptEdits => t.success,
            PermissionMode::BypassPermissions => t.danger,
            PermissionMode::Auto => t.primary,
            _ => t.muted_foreground,
        };
        let this = cx.entity();

        let chip = |id: &'static str, label: String, color: Hsla, kind: Pick, this: Entity<Self>| {
            h_flex().id(id).h(px(24.)).px_2().gap_1().items_center().rounded(px(6.)).cursor_pointer().text_size(px(12.))
                .text_color(color).hover(|d| d.bg(t.list_hover))
                .on_click(move |_, _, cx| { this.update(cx, |c, cx| { c.open = if c.open == Some(kind) { None } else { Some(kind) }; cx.notify(); }); })
                .child(label)
                .child(icon(IconName::ChevronDown, 10., color))
        };

        let popup = self.open.map(|kind| {
            let items = self.pick_list(kind, cx);
            let this = this.clone();
            v_flex().absolute().bottom(px(44.)).left(px(8.)).min_w(px(200.)).p_1().rounded(px(10.)).border_1().border_color(t.border)
                .bg(t.popover).shadow_lg().children(items.into_iter().enumerate().map(|(i, (label, detail, on, value))| {
                    let this = this.clone();
                    h_flex().id(SharedString::from(format!("pick-{i}"))).gap_2().px_2().h(px(28.)).items_center().rounded(px(6.)).cursor_pointer()
                        .hover(|d| d.bg(t.list_hover))
                        .on_click(move |_, _, cx| { let v = value.clone(); this.update(cx, |c, cx| c.choose(kind, &v, cx)); })
                        .child(div().w(px(14.)).child(if on { icon(IconName::Check, 12., t.primary).into_any_element() } else { div().into_any_element() }))
                        .child(div().text_size(px(13.)).child(label))
                        .child(div().flex_1())
                        .child(div().text_size(px(11.)).text_color(t.muted_foreground).child(detail))
                }))
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

        let send_btn = if running {
            let s = self.session.clone();
            Button::new("stop").small().danger().icon(IconName::Square).on_click(move |_, _, cx| { s.update(cx, |m, cx| m.cancel(cx)); })
        } else {
            let this = this.clone();
            Button::new("send").small().primary().icon(IconName::ArrowUp).on_click(move |_, window, cx| { this.update(cx, |c, cx| c.submit(window, cx)); })
        };

        let paste_weak = cx.entity().downgrade();
        let attach_chips = (!self.attachments.is_empty()).then(|| {
            h_flex().flex_wrap().gap_1p5().px_3().pt_2().children(self.attachments.iter().enumerate().map(|(i, a)| {
                let this = this.clone();
                h_flex().gap_1().items_center().pl_2().pr_1().h(px(22.)).rounded(px(6.)).bg(t.muted).border_1().border_color(t.border)
                    .child(icon(IconName::Image, 12., t.muted_foreground))
                    .child(div().text_size(px(12.)).child(a.name.clone()))
                    .child(
                        div().id(SharedString::from(format!("att-x-{i}"))).p(px(2.)).rounded(px(4.)).cursor_pointer().hover(|d| d.bg(t.list_hover))
                            .on_click(move |_, _, cx| { this.update(cx, |c, cx| { if i < c.attachments.len() { c.attachments.remove(i); } cx.notify(); }); })
                            .child(icon(IconName::X, 10., t.muted_foreground)),
                    )
            }))
        });

        div().w_full().flex().justify_center().px_4().pb_3().pt_1()
            // 补全列表打开时,上下键 / Tab / Esc 归补全列表;否则放行给输入框
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| { if !this.move_sel(-1, cx) { cx.propagate(); } }))
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| { if !this.move_sel(1, cx) { cx.propagate(); } }))
            .capture_action(cx.listener(|this, _: &Indent, window, cx| {
                if this.suggestions.is_empty() { cx.propagate(); } else { this.accept_suggestion(this.sel, window, cx); }
            }))
            .capture_action(cx.listener(|this, _: &crate::workbench::Escape, _, cx| {
                if this.suggestions.is_empty() && this.open.is_none() { cx.propagate(); } else { this.suggestions.clear(); this.open = None; cx.notify(); }
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
            v_flex().relative().w_full().max_w(px(crate::timeline::CONTENT_MAX_W))
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
                                .child(chip("chip-mode", mode.label().to_string(), mode_color, Pick::Mode, this.clone()))
                                .child(chip("chip-model", model_name, t.foreground, Pick::Model, this.clone()))
                                .child(chip("chip-effort", format!("思考 {}", effort.label()), t.muted_foreground, Pick::Effort, this.clone()))
                                .when(queued > 0, |d| d.child(pill(format!("排队 {queued}"), t.warning, t.muted)))
                                .child(div().flex_1())
                                .when(usage.context_size > 0, |d| {
                                    let r = usage.context_ratio();
                                    let color = if r > 0.9 { t.danger } else if r > 0.75 { t.warning } else { t.muted_foreground };
                                    d.child(div().text_size(px(11.)).text_color(color).child(format!("上下文 {:.0}%", r * 100.)))
                                })
                                .child(send_btn),
                        ),
                )
                .when_some(popup, |d, p| d.child(p)),
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
