//! macOS 应用菜单与系统级快捷键:退出 / 隐藏 / 隐藏其他 / 最小化 / 缩放 / 全屏 / 关闭窗口 / 会话前进后退。
//!
//! 全局动作(`cx.on_action`)在任何窗口都生效;菜单项会自动显示键位表里对应的快捷键。
//!
//! @author ixxxxoooo

use crate::{term_actions, workbench};
use gpui_kit::*;

actions!(loomcode_app, [QuitApp, HideApp, HideOthers, ShowAll, MinimizeWindow, ZoomWindow, ToggleFullscreen, CloseWindow, NavBack, NavForward]);

/// 对当前激活的窗口执行一个窗口操作(没有激活窗口时忽略)。
///
/// 动作是在窗口分发事件的过程中触发的,此刻窗口被占用,直接更新它会失败,所以推迟到事件处理之后再执行。
///
/// 参数:`f` 窗口操作。
fn on_active_window(cx: &mut App, f: impl FnOnce(&mut Window, &mut App) + 'static) {
    cx.defer(move |cx| {
        // 从菜单触发时窗口未必被标为「激活」(例如应用刚被系统带到前台),退而取第一个窗口
        let Some(handle) = cx.active_window().or_else(|| cx.windows().into_iter().next()) else {
            loom_diag::warn!("窗口操作没有可用窗口");
            return;
        };
        if let Err(e) = handle.update(cx, |_, window, cx| f(window, cx)) {
            loom_diag::warn!("窗口操作失败:{e}");
        }
    });
}

/// 关闭窗口(⌘W):独立窗口(IDE)真正关闭;主窗口关掉后托盘无法重建,所以改为隐藏应用(点 Dock 图标即恢复)。
fn close_window(cx: &mut App) {
    let Some(active) = cx.active_window() else { return };
    let ide = cx.try_global::<crate::WorkbenchRef>().and_then(|r| r.0.upgrade()).and_then(|wb| wb.read(cx).ide_window);
    if ide.is_some_and(|h| h.window_id() == active.window_id()) {
        on_active_window(cx, |window, _| window.remove_window());
    } else {
        cx.hide();
    }
}

/// 注册全局动作、快捷键与菜单栏(启动时调用一次)。
///
/// 退出前要先结束语言服务器并标记正常退出,否则下次启动会被误判为崩溃。
pub fn install(cx: &mut App) {
    cx.on_action(|_: &QuitApp, cx| {
        loom_lsp::hub::kill_all();
        loom_diag::crash::clean_exit("用户退出(⌘Q)");
        cx.quit();
    });
    cx.on_action(|_: &HideApp, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &MinimizeWindow, cx| on_active_window(cx, |w, _| w.minimize_window()));
    cx.on_action(|_: &ZoomWindow, cx| on_active_window(cx, |w, _| w.zoom_window()));
    cx.on_action(|_: &ToggleFullscreen, cx| on_active_window(cx, |w, _| w.toggle_fullscreen()));
    cx.on_action(|_: &CloseWindow, cx| close_window(cx));

    // ⌘W 在编辑器里由更具体的 `IdeView` 绑定(关标签)优先
    cx.bind_keys([
        KeyBinding::new("cmd-q", QuitApp, None),
        KeyBinding::new("cmd-h", HideApp, None),
        KeyBinding::new("cmd-alt-h", HideOthers, None),
        KeyBinding::new("cmd-m", MinimizeWindow, None),
        KeyBinding::new("ctrl-cmd-f", ToggleFullscreen, None),
        KeyBinding::new("cmd-w", CloseWindow, None),
        KeyBinding::new("cmd-[", NavBack, None),
        KeyBinding::new("cmd-]", NavForward, None),
    ]);

    let menu = |name: &str, items: Vec<MenuItem>| Menu { name: name.to_string().into(), items, disabled: false };
    cx.set_menus(vec![
        menu("LoomCode", vec![
            MenuItem::action("设置…", workbench::OpenSettings),
            MenuItem::separator(),
            MenuItem::os_submenu("服务", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action("隐藏 LoomCode", HideApp),
            MenuItem::action("隐藏其他", HideOthers),
            MenuItem::action("全部显示", ShowAll),
            MenuItem::separator(),
            MenuItem::action("退出 LoomCode", QuitApp),
        ]),
        menu("文件", vec![
            MenuItem::action("新建会话", workbench::NewSession),
            MenuItem::action("保存", workbench::SaveFile),
            MenuItem::separator(),
            MenuItem::action("关闭窗口", CloseWindow),
        ]),
        menu("视图", vec![
            MenuItem::action("命令面板", workbench::TogglePalette),
            MenuItem::separator(),
            MenuItem::action("切换侧栏", workbench::ToggleLeft),
            MenuItem::action("切换右栏", workbench::ToggleRight),
            MenuItem::action("切换终端", term_actions::ToggleTerminal),
            MenuItem::action("切换 IDE", workbench::ToggleIde),
            MenuItem::separator(),
            MenuItem::action("进入 / 退出全屏", ToggleFullscreen),
        ]),
        menu("前往", vec![
            MenuItem::action("后退", NavBack),
            MenuItem::action("前进", NavForward),
        ]),
        menu("窗口", vec![
            MenuItem::action("最小化", MinimizeWindow),
            MenuItem::action("缩放", ZoomWindow),
        ]),
    ]);
}
