//! LoomCode:Rust + gpui-kit 的 Claude Code 工作台。
//!
//! 启动参数:
//! - `--demo`(或环境变量 `LOOM_DEMO=1`):演示模式,脚本化后端、沙盒项目;
//! - 其余情况使用真实 `claude`:读取 `~/.claude/projects` 的历史会话,按需惰性启动子进程。

// 主题的语法配色用大段 json! 构造,默认递归上限不够
#![recursion_limit = "512"]

mod ansi;
mod ask_cards;
mod dock_cards;
mod subagent_card;
mod wizard_card;
mod appearance;
mod app_icon;
mod automation_page;
mod brand_icon_data;
mod brand_icons;
mod branch_bar;
mod browser_page;
mod canvas_page;
mod cards;
mod composer;
mod composer_pop;
mod composer_send;
mod diff;
mod mode_style;
mod model;
mod file_dnd;
mod files_workspace;
mod file_icons;
mod file_ops;
mod file_lang;
mod file_preview;
mod file_tree;
mod file_tree_view;
mod git_status;
mod git_actions;
mod git_changes;
mod image_viewer;
mod ide;
mod ide_activity;
mod ide_markdown;
mod ide_minimap;
mod minimap_model;
mod ide_quick;
mod ide_reference;
mod ide_search;
mod ide_status;
mod ide_terminal;
mod ide_watch;
mod ide_chat_tabs;
mod ide_gutter;
mod ide_memory;
mod ide_prefs;
mod ide_brackets;
mod ide_chrome;
mod ide_close;
mod ide_tabs;
mod chat_scroll;
mod nav_history;
mod row_groups;
mod tool_group;
mod tray;
mod turn_digest;
mod turn_footer;
mod window_header;
mod ide_window;
mod layout_rules;
mod palette;
mod palette_search;
mod project_actions;
mod project_add;
mod project_menu;
mod real;
mod rich_text;
mod session_actions;
mod session_fork;
mod split_layout;
mod session_menu;
mod settings;
mod right_memory;
mod right_panel;
mod right_rail;
mod right_tabs;
mod sidebar;
mod smooth_scroll;
mod sidebar_filter;
mod term_actions;
mod term_geom;
mod term_palette;
mod term_panel;
mod term_store;
mod terminal_view;
mod theme;
mod timeline;
mod tool_card;
mod ui;
mod updater;
mod user_message;
mod view_cache;
mod workspace_actions;
mod worktree_actions;
mod workspace_form;
mod window_memory;
mod workbench;

use gpui_kit::component::TitleBar;
use gpui_kit::*;
use workbench::Workbench;

/// 界面偏好的全局副本,供不持有工作台引用的渲染函数读取。
#[derive(Clone, Copy)]
pub struct UiPrefs {
    pub expand_thinking: bool,
    /// ⌘Enter 发送、Enter 换行
    pub cmd_enter_send: bool,
    /// 会话内容最大宽度(pt)
    pub chat_width: f32,
    /// 时间线正文字号倍率(1.0 = 100%)
    pub font_scale: f32,
    /// 等待动画文案风格
    pub spinner: loom_config::SpinnerStyle,
    /// 工具调用详细程度
    pub tool_density: loom_config::ToolDensity,
}
impl UiPrefs {
    /// 由偏好得到界面偏好副本。
    pub fn from_settings(s: &loom_config::AppSettings) -> Self {
        Self {
            expand_thinking: s.expand_thinking,
            cmd_enter_send: s.send_with_cmd_enter,
            chat_width: f32::from(s.appearance.chat_width.clamp(560, 1600)),
            font_scale: f32::from(s.appearance.timeline_font_pct.clamp(85, 150)) / 100.,
            spinner: s.appearance.spinner,
            tool_density: s.appearance.tool_density,
        }
    }
}
impl Default for UiPrefs {
    fn default() -> Self {
        Self::from_settings(&loom_config::AppSettings::default())
    }
}
impl Global for UiPrefs {}

/// 客户端偏好的完整全局副本:编辑器、终端、浏览器、文件检索等不持有工作台引用的地方读取。
#[derive(Clone, Default)]
pub struct Prefs(pub loom_config::AppSettings);
impl Global for Prefs {}

/// 当前偏好(尚未初始化时为默认值)。
pub fn prefs(cx: &App) -> loom_config::AppSettings {
    cx.try_global::<Prefs>().map(|p| p.0.clone()).unwrap_or_default()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let demo = args.iter().any(|a| a == "--demo") || std::env::var("LOOM_DEMO").is_ok_and(|v| v == "1");
    let session_arg = std::env::var("LOOM_SESSION").ok();
    // 更名前的数据目录 ~/.ucode 搬到 ~/.loomcode(只做一次)
    loom_config::legacy::migrate_home();
    // 诊断日志:~/.loomcode/logs/loomcode.log;崩溃信息也写进去
    if let Some(home) = std::env::var_os("HOME") {
        loom_config::diag::init(&std::path::PathBuf::from(home).join(".loomcode/logs"));
    }
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // 同时记录调用栈,便于定位重入等难以复现的崩溃
        let bt = std::backtrace::Backtrace::force_capture();
        loom_config::diag::error(&format!("崩溃:{info}\n{bt}"));
        default_hook(info);
    }));
    loom_config::diag::info(&format!("LoomCode {} 启动{}", env!("CARGO_PKG_VERSION"), if demo { "(演示模式)" } else { "" }));

    application().with_assets(gpui_kit::assets::AllAssets).run(move |cx| {
        init(cx);
        markdown_editor::init(cx);
        theme::apply(theme::ThemeKind::PaperLight, None, cx);
        cx.bind_keys([
            KeyBinding::new("cmd-k", workbench::TogglePalette, None),
            KeyBinding::new("cmd-b", workbench::ToggleLeft, None),
            KeyBinding::new("cmd-alt-b", workbench::ToggleRight, None),
            KeyBinding::new("cmd-,", workbench::OpenSettings, None),
            KeyBinding::new("cmd-n", workbench::NewSession, None),
            KeyBinding::new("cmd-s", workbench::SaveFile, None),
            KeyBinding::new("cmd-shift-e", workbench::ToggleIde, None),
            KeyBinding::new("ctrl-`", term_actions::ToggleTerminal, None),
            KeyBinding::new("ctrl-shift-`", term_actions::NewTerminal, None),
            KeyBinding::new("shift-tab", workbench::CycleMode, Some("Workbench")),
        ]);
        let mut options = TitleBar::window_options();
        // 红绿灯位置与 IDE 窗口统一(顶部行 35px,参考 OhMyAgent)
        window_header::apply_traffic_light(&mut options);
        // 调试用:LOOM_WINDOW_SIZE=宽,高 覆盖初始窗口大小(验证窄窗口布局)
        let (w, h) = std::env::var("LOOM_WINDOW_SIZE").ok().and_then(|v| v.split_once(',').and_then(|(a, b)| Some((a.trim().parse::<f32>().ok()?, b.trim().parse::<f32>().ok()?)))).unwrap_or((1360., 860.));
        // 有记忆就恢复上次的位置与大小;LOOM_WINDOW_SIZE 调试覆盖优先
        options.window_bounds = Some(if std::env::var("LOOM_WINDOW_SIZE").is_ok() { WindowBounds::Windowed(Bounds::centered(None, size(px(w), px(h)), cx)) } else { window_memory::restore("main", (w, h), cx) });
        // 最小宽度放到 640:窗口窄于 780 时侧栏自动收起,不再被 900 的下限挡住
        options.window_min_size = Some(size(px(640.), px(480.)));
        open_window(options, cx, move |window, cx| cx.new(|cx| Workbench::new(demo, session_arg, window, cx)))
            .expect("打开主窗口失败");
        cx.activate(true);
    });
}
