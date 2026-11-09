//! 再次打开应用(点程序坞图标、双击应用、快捷键 / 启动器唤醒)时把窗口带回前台。
//!
//! **多个窗口进程**(⇧⌘N 新窗口是另起一个同 bundle id 的进程):启动器 / 全局快捷键 / `open` 按 bundle id
//! 唤醒时,系统只把「再次打开」交给其中一个进程(通常是先启动的主进程),被隐藏的另一个进程永远收不到。
//! 所以每个进程被隐藏时(⌘H、启动器快捷键、程序坞菜单都算,监听 `NSApplicationDidHideNotification`)把自己的
//! pid 记进 `~/.loomcode/run/last-hidden`;任一进程收到「再次打开」时,若最近被隐藏的是同一应用的**另一个**进程
//! 且它仍隐藏着,就替它取消隐藏并激活,自己不动(见 `mac::redirect_to_hidden_sibling`)。
//!
//! 为什么要自己处理:gpui 把 `applicationShouldHandleReopen:hasVisibleWindows:` 声明成没有返回值,
//! 而 AppKit 期望一个 BOOL;返回值是随机的,为真时系统才会自己取消隐藏、恢复窗口,
//! 所以「有时能打开、有时点了没反应」(实测:关窗后连续再次打开,约三次里有一次不恢复)。
//! 而且窗口只是被隐藏时 `hasVisibleWindows` 为真,gpui 连自己的 `on_reopen` 回调都不调用。
//! 所以运行时把代理的这个方法换成我们的实现:明确返回 YES、显式取消隐藏并恢复最小化的窗口,
//! 再通知界面激活主窗口(主窗口已不存在时重建一个)。
//!
//! @author ixxxxoooo

use gpui_kit::*;

/// 接管系统的「再次打开」通知(启动时调用一次)。
///
/// 参数:`demo` 是否演示模式;`session` 启动时指定的会话(重建主窗口时沿用)。
pub fn install(cx: &mut App, demo: bool, session: Option<String>) {
    #[cfg(target_os = "macos")]
    {
        let (tx, rx) = async_channel::unbounded::<()>();
        if mac::hook_delegate(tx) {
            mac::watch_hide();
            cx.spawn(async move |cx| {
                while rx.recv().await.is_ok() {
                    let session = session.clone();
                    cx.update(|cx| bring_back(cx, demo, session));
                }
            })
            .detach();
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (cx, demo, session);
}


/// 把主窗口带回前台:取消隐藏、恢复最小化,没有主窗口就重建。
///
/// 参数:`demo` 是否演示模式;`session` 启动时指定的会话(重建主窗口时沿用)。
fn bring_back(cx: &mut App, demo: bool, session: Option<String>) {
    loom_diag::info!("再次打开应用:把窗口带回前台");
    #[cfg(target_os = "macos")]
    mac::unhide_and_restore();
    // 记下的主窗口;已被销毁时下面的 update 会失败,走重建
    let main = cx.try_global::<crate::MainWindow>().map(|m| m.0);
    match main {
        Some(handle) => {
            if handle.update(cx, |_, window, _| window.activate_window()).is_err() {
                loom_diag::warn!("主窗口激活失败,重建");
                crate::open_main_window(demo, session, cx);
            }
        }
        None => {
            loom_diag::warn!("再次打开时没有主窗口,重建");
            crate::open_main_window(demo, session, cx);
        }
    }
    cx.activate(true);
}

/// 「最近被隐藏的窗口进程」记录文件:`~/.loomcode/run/last-hidden`。
fn last_hidden_path() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".loomcode").join("run").join("last-hidden"))
}

/// 记录内容的格式:`<pid> <bundle id>`。
fn format_hidden(pid: i32, bundle: &str) -> String {
    format!("{pid} {bundle}")
}

/// 解析记录;格式不对返回 `None`。
fn parse_hidden(text: &str) -> Option<(i32, String)> {
    let (pid, bundle) = text.trim().split_once(' ')?;
    Some((pid.parse().ok()?, bundle.to_string()))
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool, Sel};
    use objc2::{MainThreadMarker, msg_send, sel};
    use objc2_app_kit::{NSApplication, NSApplicationActivationOptions, NSRunningApplication};
    use objc2_foundation::NSString;
    use std::ffi::{c_char, c_void};
    use std::sync::OnceLock;

    unsafe extern "C" {
        fn object_getClass(obj: *const c_void) -> *const c_void;
        fn class_replaceMethod(cls: *const c_void, name: *const c_void, imp: *const c_void, types: *const c_char) -> *const c_void;
    }

    /// 通知界面「再次打开」的通道(代理方法在主线程调用,界面侧的任务接收)。
    static TX: OnceLock<async_channel::Sender<()>> = OnceLock::new();

    /// 新的 `applicationShouldHandleReopen:hasVisibleWindows:`:先取消隐藏并恢复最小化的窗口,通知界面激活,明确返回 YES。
    /// 最近被隐藏的是同一应用的另一个窗口进程时改为唤醒它,自己什么也不做(返回 NO)。
    extern "C" fn should_handle_reopen(_this: *mut AnyObject, _sel: Sel, _app: *mut AnyObject, _has_visible: Bool) -> Bool {
        if redirect_to_hidden_sibling() {
            return Bool::NO;
        }
        unhide_and_restore();
        if let Some(tx) = TX.get() {
            let _ = tx.try_send(());
        }
        Bool::YES
    }

    /// 把应用代理的「再次打开」方法换成 [`should_handle_reopen`]。返回是否成功(没有代理时失败)。
    pub fn hook_delegate(tx: async_channel::Sender<()>) -> bool {
        let Some(mtm) = MainThreadMarker::new() else { return false };
        let app = NSApplication::sharedApplication(mtm);
        let delegate: Option<Retained<AnyObject>> = unsafe { msg_send![&*app, delegate] };
        let Some(delegate) = delegate else {
            loom_diag::warn!("应用代理还没设置,无法接管「再次打开」");
            return false;
        };
        let _ = TX.set(tx);
        let cls = unsafe { object_getClass(&*delegate as *const AnyObject as *const c_void) };
        // BOOL 在 arm64 上是 bool('B'),x86_64 上是 signed char('c')
        let types: &[u8] = if cfg!(target_arch = "aarch64") { b"B@:@B\0" } else { b"c@:@c\0" };
        let sel = sel!(applicationShouldHandleReopen:hasVisibleWindows:);
        unsafe {
            class_replaceMethod(cls, std::mem::transmute::<Sel, *const c_void>(sel), should_handle_reopen as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject, Bool) -> Bool as *const c_void, types.as_ptr() as *const c_char);
        }
        loom_diag::info!("已接管「再次打开」通知");
        true
    }

    /// 本进程的 bundle id;不在 `.app` 包里运行时为 `None`(此时不做跨进程唤醒)。
    fn own_bundle() -> Option<String> {
        NSRunningApplication::currentApplication().bundleIdentifier().map(|s| s.to_string())
    }

    /// 应用被隐藏时的通知回调:记下自己的 pid 与 bundle id。
    extern "C" fn did_hide(_this: *mut AnyObject, _sel: Sel, _note: *mut AnyObject) {
        let (Some(path), Some(bundle)) = (super::last_hidden_path(), own_bundle()) else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&path, super::format_hidden(std::process::id() as i32, &bundle));
    }

    /// 监听本应用被隐藏(`NSApplicationDidHideNotification`):给应用代理加一个 `loomDidHide:` 方法并注册为观察者。
    /// 不用代理的 `applicationDidHide:`:代理是 gpui 设置的,设置之后再加的方法 AppKit 不会自动接上。
    pub fn watch_hide() {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let app = NSApplication::sharedApplication(mtm);
        let delegate: Option<Retained<AnyObject>> = unsafe { msg_send![&*app, delegate] };
        let Some(delegate) = delegate else { return };
        let Some(center_cls) = AnyClass::get(c"NSNotificationCenter") else { return };
        let cls = unsafe { object_getClass(&*delegate as *const AnyObject as *const c_void) };
        let sel = sel!(loomDidHide:);
        unsafe {
            class_replaceMethod(cls, std::mem::transmute::<Sel, *const c_void>(sel), did_hide as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject) as *const c_void, c"v@:@".as_ptr());
            let center: Retained<AnyObject> = msg_send![center_cls, defaultCenter];
            let name = NSString::from_str("NSApplicationDidHideNotification");
            let _: () = msg_send![&*center, addObserver: &*delegate, selector: sel, name: &*name, object: &*app];
        }
    }

    /// 最近被隐藏的若是同一应用的另一个进程且它仍隐藏着:取消它的隐藏并激活它。返回是否这样做了。
    fn redirect_to_hidden_sibling() -> bool {
        let Some(own) = own_bundle() else { return false };
        let Some((pid, bundle)) = super::last_hidden_path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| super::parse_hidden(&t)) else { return false };
        if pid == std::process::id() as i32 || bundle != own {
            return false;
        }
        let Some(other) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) else { return false };
        // pid 可能已被别的程序复用:再核对一次 bundle id
        if other.isTerminated() || !other.isHidden() || other.bundleIdentifier().map(|s| s.to_string()).as_deref() != Some(own.as_str()) {
            return false;
        }
        loom_diag::info!("再次打开:唤醒最近被隐藏的另一个窗口进程(pid {pid})");
        other.unhide();
        // macOS 14 起激活改为「协作式」:先把激活权让给它,它才能真正到前台
        if let Some(mtm) = MainThreadMarker::new() {
            let app = NSApplication::sharedApplication(mtm);
            let responds: bool = unsafe { msg_send![&*app, respondsToSelector: sel!(yieldActivationToApplication:)] };
            if responds {
                let _: () = unsafe { msg_send![&*app, yieldActivationToApplication: &*other] };
            }
        }
        other.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows);
        true
    }

    /// 取消应用隐藏,并恢复所有被最小化到程序坞的窗口(`makeKeyAndOrderFront` 不会恢复最小化的窗口)。
    pub fn unhide_and_restore() {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let app = NSApplication::sharedApplication(mtm);
        app.unhide(None);
        let windows: Option<Retained<AnyObject>> = unsafe { msg_send![&*app, windows] };
        let Some(windows) = windows else { return };
        let count: usize = unsafe { msg_send![&*windows, count] };
        for i in 0..count {
            let w: Option<Retained<AnyObject>> = unsafe { msg_send![&*windows, objectAtIndex: i] };
            let Some(w) = w else { continue };
            let mini: bool = unsafe { msg_send![&*w, isMiniaturized] };
            if mini {
                let _: () = unsafe { msg_send![&*w, deminiaturize: Option::<&AnyObject>::None] };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 隐藏记录的读写() {
        assert_eq!(parse_hidden(&format_hidden(4321, "dev.loomcode.app")), Some((4321, "dev.loomcode.app".into())));
        assert_eq!(parse_hidden("4321 dev.loomcode.app\n"), Some((4321, "dev.loomcode.app".into())));
        assert_eq!(parse_hidden("abc dev"), None);
        assert_eq!(parse_hidden(""), None);
    }
}
