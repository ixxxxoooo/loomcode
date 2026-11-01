//! 设置页:左侧分类导航 + 右侧表单。所有修改即时生效并写入 `~/.loomcode/settings.json`。

use crate::ui::{MONO, caption, icon, pill};
use crate::workbench::Workbench;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Editor, EditorState, Input, InputState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::path::PathBuf;
use loom_config::ThemeChoice;
use loom_core::*;

/// 设置分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    General,
    Chat,
    Permissions,
    Mcp,
    Hooks,
    Memory,
    Shortcuts,
    About,
}

impl SettingsTab {
    pub const ALL: [SettingsTab; 8] = [
        Self::General,
        Self::Chat,
        Self::Permissions,
        Self::Mcp,
        Self::Hooks,
        Self::Memory,
        Self::Shortcuts,
        Self::About,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::General => "通用",
            Self::Chat => "对话",
            Self::Permissions => "权限",
            Self::Mcp => "MCP 服务器",
            Self::Hooks => "钩子",
            Self::Memory => "记忆",
            Self::Shortcuts => "快捷键",
            Self::About => "关于",
        }
    }

    pub fn icon(self) -> IconName {
        match self {
            Self::General => IconName::SlidersHorizontal,
            Self::Chat => IconName::MessageSquare,
            Self::Permissions => IconName::Shield,
            Self::Mcp => IconName::Plug,
            Self::Hooks => IconName::Webhook,
            Self::Memory => IconName::Brain,
            Self::Shortcuts => IconName::Keyboard,
            Self::About => IconName::Info,
        }
    }
}

/// 记忆页正在编辑的 CLAUDE.md。
pub struct MemoryDoc {
    pub path: PathBuf,
    pub editor: Entity<EditorState>,
    pub saved: bool,
}

/// 快捷键清单(与 `main.rs` 里的绑定保持一致)。
pub const SHORTCUTS: &[(&str, &str)] = &[
    ("⌘K", "命令面板 / 中央搜索"),
    ("⌘N", "新会话"),
    ("⌘B", "显示 / 隐藏左侧栏"),
    ("⌥⌘B", "显示 / 隐藏右侧面板"),
    ("⌘,", "打开设置"),
    ("⇧Tab", "循环切换权限模式"),
    ("Enter", "发送消息"),
    ("⇧Enter", "输入框换行"),
    ("↑ / ↓", "在补全列表中移动"),
    ("Tab", "接受补全"),
    ("Esc", "关闭弹层 / 补全"),
];

/// 一行设置:左标题与说明,右控件。
fn row(title: &str, desc: &str, control: impl IntoElement, cx: &App) -> Div {
    let t = cx.theme();
    h_flex().w_full().py_3().gap_4().items_center().border_b_1().border_color(t.border)
        .child(v_flex().flex_1().min_w_0().gap_0p5()
            .child(div().text_size(px(13.5)).child(title.to_string()))
            .when(!desc.is_empty(), |d| d.child(caption(desc.to_string(), cx))))
        .child(div().flex_none().child(control))
}

/// 分段选择器。
fn segmented<T: Copy + PartialEq + 'static>(
    id: &'static str,
    options: Vec<(T, &'static str)>,
    current: T,
    cx: &mut Context<Workbench>,
    on: impl Fn(&mut Workbench, T, &mut Context<Workbench>) + Clone + 'static,
) -> Div {
    let t = cx.theme().clone();
    h_flex().p(px(2.)).gap_0p5().rounded(px(8.)).bg(t.muted)
        .children(options.into_iter().enumerate().map(|(i, (v, label))| {
            let on = on.clone();
            let active = v == current;
            div().id(SharedString::from(format!("{id}-{i}"))).px_2p5().h(px(24.)).flex().items_center().rounded(px(6.)).cursor_pointer().text_size(px(12.))
                .when(active, |d| d.bg(t.background).shadow_sm().text_color(t.foreground))
                .when(!active, |d| d.text_color(t.muted_foreground).hover(|d| d.text_color(t.foreground)))
                .on_click(cx.listener(move |this, _, _, cx| on(this, v, cx)))
                .child(label)
        }))
}

fn section_title(text: &str, cx: &App) -> Div {
    div().pt_5().pb_1().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(cx.theme().muted_foreground).child(text.to_string())
}

fn page_title(text: &str) -> Div {
    div().text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).pb_1().child(text.to_string())
}

/// 渲染整个设置页。
pub fn render(wb: &mut Workbench, window: &mut Window, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let tab = wb.settings_tab;
    let body = match tab {
        SettingsTab::General => general(wb, cx),
        SettingsTab::Chat => chat(wb, cx),
        SettingsTab::Permissions => permissions(wb, cx),
        SettingsTab::Mcp => mcp(wb, cx),
        SettingsTab::Hooks => hooks(wb, cx),
        SettingsTab::Memory => memory(wb, window, cx),
        SettingsTab::Shortcuts => shortcuts(cx),
        SettingsTab::About => about(wb, cx),
    };
    h_flex().size_full().bg(t.background)
        .child(
            v_flex().w(px(200.)).h_full().flex_none().p_2().gap_0p5().border_r_1().border_color(t.border).bg(t.sidebar)
                .child(div().px_2().py_2().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child("设置"))
                .children(SettingsTab::ALL.into_iter().map(|st| {
                    let on = st == tab;
                    h_flex().id(SharedString::from(format!("st-{}", st.label()))).h(px(30.)).px_2().gap_2().items_center().rounded(px(6.)).cursor_pointer().text_size(px(13.))
                        .when(on, |d| d.bg(t.sidebar_accent))
                        .hover(|d| d.bg(t.sidebar_accent))
                        .on_click(cx.listener(move |this, _, _, cx| { this.settings_tab = st; cx.notify(); }))
                        .child(icon(st.icon(), 14., if on { t.foreground } else { t.muted_foreground }))
                        .child(st.label())
                })),
        )
        .child(
            div().id("settings-scroll").flex_1().min_w_0().h_full().overflow_y_scroll().flex().justify_center().px_6().pb_10()
                .child(v_flex().w_full().max_w(px(680.)).pt_6().child(body)),
        )
        .into_any_element()
}

fn general(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let theme = wb.settings.theme;
    let right_open = wb.settings.right_open;
    let fused = wb.settings.fused_ide;
    v_flex().w_full()
        .child(page_title("通用"))
        .child(section_title("外观", cx))
        .child(row("主题", "纸感浅色 / 深色,或跟随系统", segmented("theme", vec![(ThemeChoice::PaperLight, "浅色"), (ThemeChoice::PaperDark, "深色"), (ThemeChoice::System, "跟随系统")], theme, cx,
            |this, v, cx| this.update_settings(cx, |s| s.theme = v)), cx))
        .child(row("右侧面板默认打开", "启动时是否显示改动 / 文件 / 终端面板", Switch::new("sw-right").checked(right_open)
            .on_click(cx.listener(move |this, checked: &bool, _, cx| { let v = *checked; this.update_settings(cx, |s| s.right_open = v) })), cx))
        .child(row("融合模式", "关闭时 IDE 是独立窗口(⇧⌘E 打开或回到 Agent);开启后在同一窗口原地切换", Switch::new("sw-fused").checked(fused)
            .on_click(cx.listener(move |this, checked: &bool, _, cx| { let v = *checked; this.update_settings(cx, |s| s.fused_ide = v); this.on_fused_changed(cx) })), cx))
        .child(section_title("语言", cx))
        .child(row("界面语言", "目前仅提供简体中文", pill("简体中文", cx.theme().foreground, cx.theme().muted), cx))
        .into_any_element()
}

fn chat(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let s = wb.settings.clone();
    let models: Vec<(String, String)> = default_models().into_iter().map(|m| (m.id, m.name)).collect();
    let cur_model = s.default_model.clone();
    v_flex().w_full()
        .child(page_title("对话"))
        .child(section_title("新会话默认值", cx))
        .child(row("默认模型", "新建会话时使用", h_flex().gap_1().children(models.into_iter().enumerate().map(|(i, (id, name))| {
            let active = id == cur_model;
            let id2 = id.clone();
            Button::new(SharedString::from(format!("dm-{i}"))).small().when(active, |b| b.primary()).when(!active, |b| b.ghost()).label(name)
                .on_click(cx.listener(move |this, _, _, cx| { let v = id2.clone(); this.update_settings(cx, |s| s.default_model = v) }))
        })), cx))
        .child(row("默认权限模式", "也可以在输入框随时切换(⇧Tab)", segmented("dmode", PermissionMode::ALL.iter().map(|m| (*m, m.label())).collect(), s.default_mode, cx,
            |this, v, cx| this.update_settings(cx, |s| s.default_mode = v)), cx))
        .child(row("默认思考强度", "", segmented("deff", Effort::ALL.iter().map(|e| (*e, e.label())).collect(), s.default_effort, cx,
            |this, v, cx| this.update_settings(cx, |s| s.default_effort = v)), cx))
        .child(section_title("显示", cx))
        .child(row("默认展开思考内容", "关闭时思考折叠为「已思考」一行", Switch::new("sw-think").checked(s.expand_thinking)
            .on_click(cx.listener(move |this, checked: &bool, _, cx| { let v = *checked; this.update_settings(cx, |s| s.expand_thinking = v) })), cx))
        .into_any_element()
}

/// 一组规则(allow / deny / ask)。
fn rule_group(wb: &Workbench, kind: &'static str, title: &str, rules: &[String], color: Hsla, cx: &mut Context<Workbench>) -> Div {
    let t = cx.theme().clone();
    let _ = wb;
    v_flex().w_full().gap_1()
        .child(section_title(&format!("{title}({})", rules.len()), cx))
        .when(rules.is_empty(), |d| d.child(caption("暂无规则", cx)))
        .children(rules.iter().enumerate().map(|(i, r)| {
            let rule = r.clone();
            h_flex().w_full().h(px(30.)).px_2().gap_2().items_center().rounded(px(6.)).border_1().border_color(t.border)
                .child(div().size(px(6.)).rounded_full().bg(color))
                .child(div().flex_1().min_w_0().truncate().font_family(MONO).text_size(px(12.5)).child(r.clone()))
                .child(div().id(SharedString::from(format!("rule-del-{kind}-{i}"))).p_1().rounded(px(4.)).cursor_pointer().hover(|d| d.bg(t.list_hover))
                    .on_click(cx.listener(move |this, _, _, cx| this.remove_rule(kind, &rule, cx)))
                    .child(icon(IconName::X, 12., t.muted_foreground)))
        }))
}

fn permissions(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let p = wb.claude_cfg.permissions.clone();
    let scope = if wb.demo { "演示数据,仅在内存中修改" } else { "修改会写入当前项目的 .claude/settings.local.json" };
    v_flex().w_full()
        .child(page_title("权限规则"))
        .child(caption(scope, cx))
        .child(
            h_flex().w_full().gap_2().pt_4().items_center()
                .child(div().flex_1().child(Input::new(&wb.rule_input)))
                .children([("allow", "允许"), ("ask", "询问"), ("deny", "拒绝")].into_iter().map(|(kind, label)| {
                    Button::new(SharedString::from(format!("add-{kind}"))).small().outline().label(label)
                        .on_click(cx.listener(move |this, _, window, cx| this.add_rule(kind, window, cx)))
                })),
        )
        .child(caption("规则示例:Bash(git status)、Bash(npm run:*)、Read(src/**)", cx))
        .child(rule_group(wb, "allow", "始终允许", &p.allow, t.success, cx))
        .child(rule_group(wb, "ask", "每次询问", &p.ask, t.warning, cx))
        .child(rule_group(wb, "deny", "始终拒绝", &p.deny, t.danger, cx))
        .into_any_element()
}

fn mcp(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let servers = wb.claude_cfg.mcp_servers.clone();
    v_flex().w_full()
        .child(page_title("MCP 服务器"))
        .child(caption("来自 ~/.claude/settings.json、项目 .claude/settings.json 与 .mcp.json", cx))
        .child(v_flex().w_full().pt_4().gap_2()
            .when(servers.is_empty(), |d| d.child(caption("没有配置 MCP 服务器", cx)))
            .children(servers.into_iter().map(|s| {
                h_flex().w_full().p_3().gap_3().items_center().rounded(px(10.)).border_1().border_color(t.border)
                    .child(icon(IconName::Plug, 16., if s.enabled { t.primary } else { t.muted_foreground }))
                    .child(v_flex().flex_1().min_w_0().gap_0p5()
                        .child(h_flex().gap_2().items_center()
                            .child(div().text_size(px(13.5)).font_weight(FontWeight::MEDIUM).child(s.name.clone()))
                            .child(pill(s.transport.clone(), t.muted_foreground, t.muted)))
                        .child(div().truncate().font_family(MONO).text_size(px(12.)).text_color(t.muted_foreground).child(s.target.clone())))
                    .child(pill(if s.enabled { "已启用" } else { "已禁用" }, if s.enabled { t.success } else { t.muted_foreground }, t.muted))
            })))
        .into_any_element()
}

fn hooks(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let hooks = wb.claude_cfg.hooks.clone();
    v_flex().w_full()
        .child(page_title("钩子"))
        .child(caption("在工具调用前后自动运行的命令(只读展示)", cx))
        .child(v_flex().w_full().pt_4().gap_2()
            .when(hooks.is_empty(), |d| d.child(caption("没有配置钩子", cx)))
            .children(hooks.into_iter().map(|h| {
                v_flex().w_full().p_3().gap_1().rounded(px(10.)).border_1().border_color(t.border)
                    .child(h_flex().gap_2().items_center()
                        .child(pill(h.event.clone(), t.primary, t.muted))
                        .when(!h.matcher.is_empty(), |d| d.child(div().font_family(MONO).text_size(px(12.)).text_color(t.muted_foreground).child(format!("匹配 {}", h.matcher)))))
                    .child(div().font_family(MONO).text_size(px(12.5)).child(h.command.clone()))
            })))
        .into_any_element()
}

fn memory(wb: &mut Workbench, window: &mut Window, cx: &mut Context<Workbench>) -> AnyElement {
    wb.ensure_memory(window, cx);
    let t = cx.theme().clone();
    let Some(doc) = wb.memory.as_ref() else {
        return v_flex().child(page_title("记忆")).child(caption("先打开一个项目会话", cx)).into_any_element();
    };
    let path = doc.path.display().to_string();
    let saved = doc.saved;
    v_flex().w_full().gap_2()
        .child(page_title("记忆(CLAUDE.md)"))
        .child(h_flex().gap_2().items_center()
            .child(div().flex_1().min_w_0().truncate().font_family(MONO).text_size(px(12.)).text_color(t.muted_foreground).child(path))
            .child(Button::new("memory-save").small().when(!saved, |b| b.primary()).when(saved, |b| b.ghost()).label(if saved { "已保存" } else { "保存" })
                .on_click(cx.listener(|this, _, _, cx| this.save_memory(cx)))))
        .child(div().w_full().h(px(460.)).rounded(px(8.)).border_1().border_color(t.border).overflow_hidden()
            .child(Editor::new(&doc.editor).bordered(false).appearance(false).size_full()))
        .into_any_element()
}

fn shortcuts(cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    v_flex().w_full()
        .child(page_title("快捷键"))
        .child(v_flex().w_full().pt_3()
            .children(SHORTCUTS.iter().map(|(k, d)| {
                h_flex().w_full().py_2().gap_3().items_center().border_b_1().border_color(t.border)
                    .child(div().flex_1().text_size(px(13.)).child(d.to_string()))
                    .child(div().px_2().h(px(22.)).flex().items_center().rounded(px(5.)).bg(t.muted).font_family(MONO).text_size(px(12.)).child(k.to_string()))
            })))
        .into_any_element()
}

fn about(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let claude = wb.claude_info.clone().unwrap_or_else(|| if wb.demo { "演示模式(未连接)".into() } else { "检测中…".into() });
    v_flex().w_full()
        .child(page_title("关于"))
        .child(row("LoomCode", "Rust + gpui-kit 的 Claude Code 工作台", pill(format!("v{}", env!("CARGO_PKG_VERSION")), t.foreground, t.muted), cx))
        .child(row("Claude Code", "本机 claude 命令行", div().font_family(MONO).text_size(px(12.5)).child(claude), cx))
        .child(row("运行模式", "", pill(if wb.demo { "演示" } else { "真实" }, if wb.demo { t.warning } else { t.success }, t.muted), cx))
        .child(row("配置文件", "", div().font_family(MONO).text_size(px(12.)).text_color(t.muted_foreground).child(wb.settings_path.display().to_string()), cx))
        .into_any_element()
}

/// 供 Workbench 创建输入框状态用。
pub fn new_rule_input(window: &mut Window, cx: &mut App) -> Entity<InputState> {
    cx.new(|cx| InputState::new(window, cx).placeholder("输入规则,再点右侧按钮添加"))
}
