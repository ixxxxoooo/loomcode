//! 应用图标与菜单栏托盘(仅 macOS)。
//!
//! - Dock 图标跟随界面主题切换浅色 / 深色版(打包的 `.icns` 只有浅色版,运行中由这里覆盖);
//! - 菜单栏托盘:模板图标(系统按菜单栏明暗自动着色),菜单里有「显示 LoomCode」与「退出」。
//!
//! 图标源文件与生成脚本在仓库根目录的 `assets/icon/`。
//!
//! @author ixxxxoooo

/// 浅色 App 图标(1024×1024 PNG)。
#[cfg(target_os = "macos")]
const ICON_LIGHT: &[u8] = include_bytes!("../../../assets/icon/app-light.png");
/// 深色 App 图标(1024×1024 PNG)。
#[cfg(target_os = "macos")]
const ICON_DARK: &[u8] = include_bytes!("../../../assets/icon/app-dark.png");
/// 菜单栏模板图标(36×36,即 18pt@2x,黑色 + 透明)。
#[cfg(target_os = "macos")]
const TRAY: &[u8] = include_bytes!("../../../assets/icon/tray.png");

#[cfg(target_os = "macos")]
mod mac {
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
    use objc2_app_kit::{NSApplication, NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
    use objc2_foundation::{NSData, NSSize, NSString};
    use std::cell::{Cell, RefCell};

    define_class!(
        /// 托盘菜单项的动作接收者。
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "LoomCodeTrayTarget"]
        struct TrayTarget;

        impl TrayTarget {
            /// 「显示 LoomCode」:取消隐藏并把应用切到前台。
            #[unsafe(method(showApp:))]
            fn show_app(&self, _sender: Option<&AnyObject>) {
                let app = NSApplication::sharedApplication(self.mtm());
                app.unhide(None);
                #[allow(deprecated)]
                app.activateIgnoringOtherApps(true);
            }
        }
    );

    thread_local! {
        /// 托盘对象要一直持有,释放后图标会从菜单栏消失。
        static TRAY: RefCell<Option<(Retained<NSStatusItem>, Retained<TrayTarget>)>> = const { RefCell::new(None) };
        /// 当前 Dock 图标是否深色,避免重复设置。
        static DOCK_DARK: Cell<Option<bool>> = const { Cell::new(None) };
    }

    /// 从 PNG 字节创建 NSImage。参数:`bytes` PNG 数据。返回值:图片,数据无效时为 `None`。
    fn image(bytes: &[u8]) -> Option<Retained<NSImage>> {
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

    /// 创建菜单项。参数:标题、动作、快捷键、目标(`None` 走响应链)。返回值:菜单项。
    fn menu_item(mtm: MainThreadMarker, title: &str, action: objc2::runtime::Sel, key: &str, target: Option<&AnyObject>) -> Retained<NSMenuItem> {
        let item = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &NSString::from_str(title), Some(action), &NSString::from_str(key)) };
        unsafe { item.setTarget(target) };
        item
    }

    /// 在菜单栏放置托盘图标(重复调用无副作用)。
    pub fn install_tray() {
        let Some(mtm) = MainThreadMarker::new() else { return };
        if TRAY.with_borrow(|t| t.is_some()) {
            return;
        }
        let Some(icon) = image(super::TRAY) else { return };
        icon.setSize(NSSize::new(18., 18.));
        icon.setTemplate(true);

        let target: Retained<TrayTarget> = unsafe { msg_send![TrayTarget::alloc(mtm), init] };
        let menu = NSMenu::new(mtm);
        menu.addItem(&menu_item(mtm, "显示 LoomCode", sel!(showApp:), "", Some(&target)));
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        // 目标为空:沿响应链交给 NSApplication,走正常的退出流程
        menu.addItem(&menu_item(mtm, "退出 LoomCode", sel!(terminate:), "q", None));

        let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        if let Some(button) = item.button(mtm) {
            button.setImage(Some(&icon));
            button.setToolTip(Some(&NSString::from_str("LoomCode")));
        }
        item.setMenu(Some(&menu));
        TRAY.set(Some((item, target)));
    }

    /// 移除菜单栏图标(设置里关闭「菜单栏图标」时)。
    pub fn remove_tray() {
        if let Some((item, _)) = TRAY.take() {
            NSStatusBar::systemStatusBar().removeStatusItem(&item);
        }
    }
}

/// 按主题切换 Dock 图标。参数:`dark` 当前主题是否深色。
pub fn set_dock_icon(dark: bool) {
    #[cfg(target_os = "macos")]
    mac::set_dock_icon(dark);
    #[cfg(not(target_os = "macos"))]
    let _ = dark;
}

/// 放置菜单栏托盘图标(仅 macOS;其他平台什么也不做)。
/// 按设置显示或移除菜单栏图标(启动时与设置里切换时都走这里)。
pub fn set_tray_visible(visible: bool) {
    #[cfg(target_os = "macos")]
    if visible { mac::install_tray() } else { mac::remove_tray() }
    #[cfg(not(target_os = "macos"))]
    let _ = visible;
}

/// 发系统通知(经 osascript,不需要通知权限声明)。
///
/// 参数:`title` 标题;`body` 正文。
pub fn notify(title: &str, body: &str) {
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!("display notification \"{}\" with title \"LoomCode\" subtitle \"{}\"", esc(body), esc(title));
    let _ = std::process::Command::new("osascript").arg("-e").arg(script).spawn();
}
