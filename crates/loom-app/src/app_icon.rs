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

    /// 经 UserNotifications 发一条通知;返回是否已提交(失败时调用方回退到 osascript)。
    pub fn notify(title: &str, body: &str, thread: &str) -> bool {
        use block2::RcBlock;
        use objc2::runtime::Bool;
        use objc2_foundation::{NSError, NSString};
        use objc2_user_notifications::{UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest, UNNotificationSound, UNUserNotificationCenter};
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let center = UNUserNotificationCenter::currentNotificationCenter();
        // 授权请求幂等:已决定过不会再弹窗
        let done = RcBlock::new(|granted: Bool, err: *mut NSError| {
            if !granted.as_bool() {
                // SAFETY: 系统回调传入的 NSError 指针要么为空,要么在回调期间有效
                let why = unsafe { err.as_ref() }.map(|e| e.localizedDescription().to_string()).unwrap_or_default();
                loom_diag::info!("系统通知权限未授予(可在 系统设置 → 通知 里开启){}", if why.is_empty() { String::new() } else { format!(":{why}") });
            }
        });
        center.requestAuthorizationWithOptions_completionHandler(UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound, &done);
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(title));
        content.setBody(&NSString::from_str(body));
        if !thread.is_empty() {
            content.setThreadIdentifier(&NSString::from_str(thread));
        }
        content.setSound(Some(&UNNotificationSound::defaultSound()));
        let id = NSString::from_str(&format!("loom-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(&id, &content, None);
        let added = RcBlock::new(|err: *mut NSError| {
            if !err.is_null() {
                loom_diag::warn!("系统通知提交失败(可能未授权通知权限)");
            }
        });
        center.addNotificationRequest_withCompletionHandler(&request, Some(&added));
        true
    }
}

/// 从 PNG 字节创建 NSImage(托盘用)。
#[cfg(target_os = "macos")]
pub(crate) use mac::image as ns_image;

/// App 图标 PNG 字节(浅色 / 深色版),给「关于」窗口显示。
pub fn icon_png(dark: bool) -> &'static [u8] {
    #[cfg(target_os = "macos")]
    {
        if dark { ICON_DARK } else { ICON_LIGHT }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = dark;
        &[]
    }
}

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

/// 发系统通知。
///
/// 从 `.app` 包运行时走 UserNotifications:通知归属本应用,图标取应用包的图标(首次会请求通知授权)。
/// 不在包里运行(裸二进制、`cargo run`)时 UserNotifications 不可用,回退到 osascript(通知会记在「脚本编辑器」名下)。
///
/// 参数:`title` 标题;`body` 正文;`thread` 分组标识(同一会话的通知折叠在一起,空串不分组)。
pub fn notify(title: &str, body: &str, thread: &str) {
    #[cfg(target_os = "macos")]
    if in_app_bundle() && mac::notify(title, body, thread) {
        return;
    }
    let _ = thread;
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!("display notification \"{}\" with title \"LoomCode\" subtitle \"{}\"", esc(body), esc(title));
    let _ = std::process::Command::new("osascript").arg("-e").arg(script).spawn();
}

/// 是否从 `.app` 包里运行(UserNotifications 只在有包标识时可用)。
fn in_app_bundle() -> bool {
    std::env::current_exe().ok().is_some_and(|exe| exe.ancestors().any(|p| p.extension().is_some_and(|e| e == "app")))
}
