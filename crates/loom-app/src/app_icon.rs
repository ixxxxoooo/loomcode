//! 应用图标(仅 macOS):Dock 图标跟随界面主题切换浅色 / 深色版(打包的 `.icns` 只有浅色版,运行中由这里覆盖)。
//! 菜单栏托盘在 `tray.rs`;图标源文件与生成脚本在仓库根目录的 `assets/icon/`。
//!
//! @author ixxxxoooo

/// 浅色 App 图标(1024×1024 PNG)。
#[cfg(target_os = "macos")]
const ICON_LIGHT: &[u8] = include_bytes!("../../../assets/icon/app-light.png");
/// 深色 App 图标(1024×1024 PNG)。
#[cfg(target_os = "macos")]
const ICON_DARK: &[u8] = include_bytes!("../../../assets/icon/app-dark.png");

#[cfg(target_os = "macos")]
mod mac {
    use objc2::rc::Retained;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;
    use std::cell::Cell;

    thread_local! {
        /// 当前 Dock 图标是否深色,避免重复设置。
        static DOCK_DARK: Cell<Option<bool>> = const { Cell::new(None) };
    }

    /// 从 PNG 字节创建 NSImage。参数:`bytes` PNG 数据。返回值:图片,数据无效时为 `None`。
    pub fn image(bytes: &[u8]) -> Option<Retained<NSImage>> {
        use objc2::AnyThread;
        let data = NSData::with_bytes(bytes);
        NSImage::initWithData(NSImage::alloc(), &data)
    }

    /// 设置 Dock 图标。参数:`dark` 是否用深色版。
    pub fn set_dock_icon(dark: bool) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        if DOCK_DARK.get() == Some(dark) {
            return;
        }
        if let Some(img) = image(if dark { super::ICON_DARK } else { super::ICON_LIGHT }) {
            let app = NSApplication::sharedApplication(mtm);
            unsafe { app.setApplicationIconImage(Some(&img)) };
            DOCK_DARK.set(Some(dark));
        }
    }
}

/// 从 PNG 字节创建 NSImage(托盘用)。
#[cfg(target_os = "macos")]
pub(crate) use mac::image as ns_image;

/// 按主题切换 Dock 图标。参数:`dark` 当前主题是否深色。
pub fn set_dock_icon(dark: bool) {
    #[cfg(target_os = "macos")]
    mac::set_dock_icon(dark);
    #[cfg(not(target_os = "macos"))]
    let _ = dark;
}

/// 按设置显示或移除菜单栏托盘图标(启动时与设置里切换时都走这里)。
pub fn set_tray_visible(visible: bool) {
    crate::tray::set_visible(visible);
}

/// 发系统通知(经 osascript,不需要通知权限声明)。
///
/// 参数:`title` 标题;`body` 正文。
pub fn notify(title: &str, body: &str) {
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!("display notification \"{}\" with title \"LoomCode\" subtitle \"{}\"", esc(body), esc(title));
    let _ = std::process::Command::new("osascript").arg("-e").arg(script).spawn();
}
