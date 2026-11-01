//! 权限模式的图标与颜色:菜单里每一项、输入框外的模式标签用同一套,一眼分清当前是哪种模式。
//!
//! @author ixxxxoooo

use gpui_kit::assets::IconName;
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;
use loom_core::PermissionMode;

/// 模式对应的图标。
pub fn mode_icon(mode: PermissionMode) -> IconName {
    match mode {
        PermissionMode::Default => IconName::Shield,
        PermissionMode::AcceptEdits => IconName::FilePen,
        PermissionMode::Plan => IconName::ClipboardList,
        PermissionMode::Auto => IconName::Sparkles,
        PermissionMode::DontAsk => IconName::ShieldBan,
        PermissionMode::BypassPermissions => IconName::ShieldAlert,
    }
}

/// 模式对应的颜色:默认灰、接受编辑绿、计划蓝、自动主题色、不再询问黄、绕过权限红。
pub fn mode_color(mode: PermissionMode, cx: &App) -> Hsla {
    let t = cx.theme();
    match mode {
        PermissionMode::Default => t.muted_foreground,
        PermissionMode::AcceptEdits => t.success,
        PermissionMode::Plan => t.info,
        PermissionMode::Auto => t.primary,
        PermissionMode::DontAsk => t.warning,
        PermissionMode::BypassPermissions => t.danger,
    }
}
