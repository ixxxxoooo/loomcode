//! UCode:Rust + gpui-kit 的 Claude Code 工作台。
//!
//! 启动参数:
//! - `--demo`(或环境变量 `UCODE_DEMO=1`):演示模式,脚本化后端、沙盒项目;
//! - 其余情况使用真实 `claude`(尚在接入中,缺省回退到演示模式并提示)。

mod ansi;
mod cards;
mod composer;
mod diff;
mod model;
mod file_tree;
mod palette;
mod rich_text;
mod settings;
mod right_panel;
mod sidebar;
mod theme;
mod timeline;
mod tool_card;
mod ui;
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
    let demo = args.iter().any(|a| a == "--demo") || std::env::var("UCODE_DEMO").is_ok_and(|v| v == "1");
    let session_arg = std::env::var("UCODE_SESSION").ok();

    application().with_assets(gpui_kit::assets::AllAssets).run(move |cx| {
        init(cx);
        theme::apply(theme::ThemeKind::PaperLight, None, cx);
        cx.bind_keys([
            KeyBinding::new("cmd-k", workbench::TogglePalette, None),
            KeyBinding::new("cmd-b", workbench::ToggleLeft, None),
            KeyBinding::new("cmd-alt-b", workbench::ToggleRight, None),
            KeyBinding::new("cmd-,", workbench::OpenSettings, None),
            KeyBinding::new("cmd-n", workbench::NewSession, None),
            KeyBinding::new("shift-tab", workbench::CycleMode, Some("Workbench")),
        ]);
        let mut options = TitleBar::window_options();
        options.window_bounds = Some(WindowBounds::Windowed(Bounds::centered(None, size(px(1360.), px(860.)), cx)));
        options.window_min_size = Some(size(px(900.), px(560.)));
        open_window(options, cx, move |window, cx| cx.new(|cx| Workbench::new(demo, session_arg, window, cx)))
            .expect("打开主窗口失败");
        cx.activate(true);
    });
}
