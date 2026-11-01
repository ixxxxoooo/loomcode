//! LoomCode:Rust + gpui-kit 的 Claude Code 工作台。
//!
//! 启动参数:
//! - `--demo`(或环境变量 `LOOM_DEMO=1`):演示模式,脚本化后端、沙盒项目;
//! - 其余情况使用真实 `claude`:读取 `~/.claude/projects` 的历史会话,按需惰性启动子进程。

mod ansi;
mod app_icon;
mod automation_page;
mod branch_bar;
mod browser_page;
mod canvas_page;
mod cards;
mod composer;
mod composer_pop;
mod diff;
mod model;
mod file_icons;
mod file_ops;
mod file_lang;
mod file_tree;
mod git_status;
mod git_actions;
mod git_changes;
mod ide;
mod ide_activity;
mod ide_minimap;
mod ide_quick;
mod ide_reference;
mod ide_search;
mod ide_status;
mod ide_terminal;
mod ide_watch;
mod ide_chat_header;
mod ide_gutter;
mod ide_brackets;
mod ide_chrome;
mod ide_close;
mod ide_tabs;
mod chat_scroll;
mod nav_history;
mod row_groups;
mod tool_group;
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
mod right_panel;
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
mod view_cache;
mod workspace_actions;
mod worktree_actions;
mod workspace_form;
mod workbench;

use gpui_kit::component::TitleBar;
use gpui_kit::*;
use workbench::Workbench;

/// 界面偏好的全局副本,供不持有工作台引用的渲染函数读取。
#[derive(Clone, Copy, Default)]
pub struct UiPrefs {
    pub expand_thinking: bool,
}
impl Global for UiPrefs {}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let demo = args.iter().any(|a| a == "--demo") || std::env::var("LOOM_DEMO").is_ok_and(|v| v == "1");
    let session_arg = std::env::var("LOOM_SESSION").ok();
    // 更名前的数据目录 ~/.ucode 搬到 ~/.loomcode(只做一次)
    loom_config::legacy::migrate_home();

    application().with_assets(gpui_kit::assets::AllAssets).run(move |cx| {
        init(cx);
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
        options.window_bounds = Some(WindowBounds::Windowed(Bounds::centered(None, size(px(w), px(h)), cx)));
        // 最小宽度放到 640:窗口窄于 780 时侧栏自动收起,不再被 900 的下限挡住
        options.window_min_size = Some(size(px(640.), px(480.)));
        open_window(options, cx, move |window, cx| cx.new(|cx| Workbench::new(demo, session_arg, window, cx)))
            .expect("打开主窗口失败");
        app_icon::install_tray();
        cx.activate(true);
    });
}
