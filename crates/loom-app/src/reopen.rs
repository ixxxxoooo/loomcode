//! 再次打开应用(点程序坞图标、双击应用、快捷键 / 启动器唤醒)时把窗口带回前台。
//!
//! **多个窗口进程**(⇧⌘N 新窗口是另起一个同 bundle id 的进程):启动器 / 全局快捷键 / `open` 按 bundle id
//! 找应用,只认其中一个进程(通常是先启动的主进程)。「显示 / 隐藏」切换类快捷键判断的是「这个进程在不在最前面」,
//! 所以在另一个窗口进程里按快捷键,它只会把主进程拉到前面,永远隐藏不了当前窗口;被隐藏的另一个进程也收不到「再次打开」。
//! 因此每个进程失去前台或被隐藏时(`NSApplicationDidResignActiveNotification` / `DidHideNotification`,
//! ⌘H、启动器快捷键、程序坞菜单都算)把「谁、怎么了、何时」记进 `~/.loomcode/run/last-used`;
//! 收到「再次打开」的进程按 [`decide`] 处理:最近使用的是另一个窗口进程且它此刻在前台(或刚被这次快捷键抢走前台)
//! = 在那个窗口里按了切换键,把它和自己一起隐藏;否则最近使用的是另一个窗口进程就唤醒它;都不是才唤醒自己。
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
            mac::watch_usage();
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

/// 「最近使用的窗口进程」记录文件:`~/.loomcode/run/last-used`。
fn last_used_path() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".loomcode").join("run").join("last-used"))
}

/// 窗口进程最近一次被记录的事件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Usage {
    /// 失去前台(切到别的应用,或被别的进程抢走前台)
    Resign,
    /// 被隐藏
    Hide,
}

/// 一条使用记录。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Record {
    pid: i32,
    bundle: String,
    usage: Usage,
    /// 记录时间(Unix 毫秒)
    ms: u64,
}

impl Record {
    /// 序列化:`<pid> <bundle id> <resign|hide> <毫秒>`。
    fn format(&self) -> String {
        let u = match self.usage { Usage::Resign => "resign", Usage::Hide => "hide" };
        format!("{} {} {u} {}", self.pid, self.bundle, self.ms)
    }

    /// 解析;格式不对返回 `None`。
    fn parse(text: &str) -> Option<Self> {
        let mut it = text.split_whitespace();
        let pid = it.next()?.parse().ok()?;
        let bundle = it.next()?.to_string();
        let usage = match it.next()? { "resign" => Usage::Resign, "hide" => Usage::Hide, _ => return None };
        let ms = it.next()?.parse().ok()?;
        Some(Self { pid, bundle, usage, ms })
    }
}

/// 记录里那个进程此刻的状态(由调用方从系统查询)。
#[derive(Debug, Clone, Copy)]
struct Sibling {
    hidden: bool,
    active: bool,
}

/// 收到「再次打开」后怎么做。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decision {
    /// 照常把自己的窗口带回前台
    Normal,
    /// 在另一个窗口进程里按了切换键:把它和自己一起隐藏
    HideAll(i32),
    /// 唤醒另一个窗口进程(取消隐藏并激活),自己不动
    Bring(i32),
}

/// 「刚被这次快捷键抢走前台」的时间窗:另一个进程失去前台后这么久之内收到的「再次打开」视为在它里面按的。
const TOGGLE_WINDOW_MS: u64 = 1500;

/// 决定收到「再次打开」时怎么做(纯函数)。
///
/// 参数:`own_pid` / `own_bundle` 本进程;`rec` 最近使用记录;`sibling` 记录里那个进程的状态(已退出或不是同一应用为 `None`);`now_ms` 当前时间。
fn decide(own_pid: i32, own_bundle: &str, rec: Option<&Record>, sibling: Option<Sibling>, now_ms: u64) -> Decision {
    let Some(rec) = rec else { return Decision::Normal };
    if rec.pid == own_pid || rec.bundle != own_bundle {
        return Decision::Normal;
    }
    let Some(sib) = sibling else { return Decision::Normal };
    let just_left = rec.usage == Usage::Resign && now_ms.saturating_sub(rec.ms) <= TOGGLE_WINDOW_MS;
    if !sib.hidden && (sib.active || just_left) {
        Decision::HideAll(rec.pid)
    } else {
        Decision::Bring(rec.pid)
    }
}

/// 当前 Unix 毫秒。
fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
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
    use std::sync::atomic::{AtomicU64, Ordering};

    unsafe extern "C" {
        fn object_getClass(obj: *const c_void) -> *const c_void;
        fn class_replaceMethod(cls: *const c_void, name: *const c_void, imp: *const c_void, types: *const c_char) -> *const c_void;
    }

    /// 通知界面「再次打开」的通道(代理方法在主线程调用,界面侧的任务接收)。
    static TX: OnceLock<async_channel::Sender<()>> = OnceLock::new();

    /// 新的 `applicationShouldHandleReopen:hasVisibleWindows:`:先取消隐藏并恢复最小化的窗口,通知界面激活,明确返回 YES。
    /// 最近被隐藏的是同一应用的另一个窗口进程时改为唤醒它,自己什么也不做(返回 NO)。
    extern "C" fn should_handle_reopen(_this: *mut AnyObject, _sel: Sel, _app: *mut AnyObject, _has_visible: Bool) -> Bool {
        if redirect_to_sibling() {
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

    /// 本进程替别的窗口进程处理「再次打开」后的一小段时间里,自己的失去前台 / 隐藏不记录
    /// (那是被我们自己让出去的,记下来会盖掉真正的「最近使用」)。值为截止时间(Unix 毫秒)。
    static QUIET_UNTIL: AtomicU64 = AtomicU64::new(0);

    /// 写一条使用记录(安静期内忽略)。
    fn note(usage: super::Usage) {
        if super::now_ms() < QUIET_UNTIL.load(Ordering::Relaxed) {
            return;
        }
        let (Some(path), Some(bundle)) = (super::last_used_path(), own_bundle()) else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let rec = super::Record { pid: std::process::id() as i32, bundle, usage, ms: super::now_ms() };
        let _ = std::fs::write(&path, rec.format());
    }

    extern "C" fn did_hide(_this: *mut AnyObject, _sel: Sel, _note: *mut AnyObject) {
        note(super::Usage::Hide);
    }

    extern "C" fn did_resign(_this: *mut AnyObject, _sel: Sel, _note: *mut AnyObject) {
        note(super::Usage::Resign);
    }

    /// 监听本应用被隐藏 / 失去前台:给应用代理加 `loomDidHide:` / `loomDidResign:` 方法并注册为通知观察者。
    /// 不用代理的 `applicationDidHide:` 等:代理是 gpui 设置的,设置之后再加的方法 AppKit 不会自动接上。
    pub fn watch_usage() {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let app = NSApplication::sharedApplication(mtm);
        let delegate: Option<Retained<AnyObject>> = unsafe { msg_send![&*app, delegate] };
        let Some(delegate) = delegate else { return };
        let Some(center_cls) = AnyClass::get(c"NSNotificationCenter") else { return };
        let cls = unsafe { object_getClass(&*delegate as *const AnyObject as *const c_void) };
        let center: Retained<AnyObject> = unsafe { msg_send![center_cls, defaultCenter] };
        let hooks: [(Sel, extern "C" fn(*mut AnyObject, Sel, *mut AnyObject), &str); 2] = [
            (sel!(loomDidHide:), did_hide, "NSApplicationDidHideNotification"),
            (sel!(loomDidResign:), did_resign, "NSApplicationDidResignActiveNotification"),
        ];
        for (sel, imp, name) in hooks {
            unsafe {
                class_replaceMethod(cls, std::mem::transmute::<Sel, *const c_void>(sel), imp as *const c_void, c"v@:@".as_ptr());
                let name = NSString::from_str(name);
                let _: () = msg_send![&*center, addObserver: &*delegate, selector: sel, name: &*name, object: &*app];
            }
        }
    }

    /// 按最近使用记录处理「再次打开」:隐藏另一个窗口进程(连同自己)或唤醒它。返回是否替别的进程处理了。
    fn redirect_to_sibling() -> bool {
        let Some(own) = own_bundle() else { return false };
        let rec = super::last_used_path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| super::Record::parse(&t));
        let own_pid = std::process::id() as i32;
        // 记录里的进程:pid 可能已被别的程序复用,再核对一次 bundle id
        let other = rec.as_ref().filter(|r| r.pid != own_pid).and_then(|r| NSRunningApplication::runningApplicationWithProcessIdentifier(r.pid))
            .filter(|o| !o.isTerminated() && o.bundleIdentifier().map(|s| s.to_string()).as_deref() == Some(own.as_str()));
        let sib = other.as_ref().map(|o| super::Sibling { hidden: o.isHidden(), active: o.isActive() });
        let now = super::now_ms();
        let decision = super::decide(own_pid, &own, rec.as_ref(), sib, now);
        let (Some(other), Some(mtm)) = (other, MainThreadMarker::new()) else { return false };
        let app = NSApplication::sharedApplication(mtm);
        match decision {
            super::Decision::Normal => false,
            super::Decision::HideAll(pid) => {
                loom_diag::info!("再次打开:在另一个窗口进程(pid {pid})里按了切换键,一起隐藏");
                QUIET_UNTIL.store(now + super::TOGGLE_WINDOW_MS, Ordering::Relaxed);
                other.hide();
                app.hide(None);
                true
            }
            super::Decision::Bring(pid) => {
                loom_diag::info!("再次打开:唤醒最近使用的另一个窗口进程(pid {pid})");
                QUIET_UNTIL.store(now + super::TOGGLE_WINDOW_MS, Ordering::Relaxed);
                other.unhide();
                // macOS 14 起激活改为「协作式」:先把激活权让给它,它才能真正到前台
                let responds: bool = unsafe { msg_send![&*app, respondsToSelector: sel!(yieldActivationToApplication:)] };
                if responds {
                    let _: () = unsafe { msg_send![&*app, yieldActivationToApplication: &*other] };
                }
                other.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows);
                true
            }
        }
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

    const B: &str = "dev.loomcode.app";

    fn rec(pid: i32, usage: Usage, ms: u64) -> Record {
        Record { pid, bundle: B.into(), usage, ms }
    }

    #[::core::prelude::v1::test]
    fn 使用记录的读写() {
        let r = rec(4321, Usage::Resign, 1_700_000_000_123);
        assert_eq!(Record::parse(&r.format()), Some(r));
        assert_eq!(Record::parse(&format!("{}\n", rec(1, Usage::Hide, 5).format())), Some(rec(1, Usage::Hide, 5)));
        assert_eq!(Record::parse("4321 dev.loomcode.app"), None);
        assert_eq!(Record::parse("x y hide 1"), None);
    }

    #[::core::prelude::v1::test]
    fn 收到再次打开时的决定() {
        let me = 100;
        let sib = |hidden, active| Some(Sibling { hidden, active });
        // 没有记录 / 记录是自己 / 不同应用 / 那个进程已退出:照常
        assert_eq!(decide(me, B, None, None, 0), Decision::Normal);
        assert_eq!(decide(me, B, Some(&rec(me, Usage::Hide, 0)), sib(true, false), 0), Decision::Normal);
        assert_eq!(decide(me, "other", Some(&rec(7, Usage::Hide, 0)), sib(true, false), 0), Decision::Normal);
        assert_eq!(decide(me, B, Some(&rec(7, Usage::Hide, 0)), None, 0), Decision::Normal);
        // 在另一个窗口里按了切换键:它还在前台,或刚刚失去前台 → 一起隐藏
        assert_eq!(decide(me, B, Some(&rec(7, Usage::Resign, 10_000)), sib(false, true), 10_000), Decision::HideAll(7));
        assert_eq!(decide(me, B, Some(&rec(7, Usage::Resign, 10_000)), sib(false, false), 10_800), Decision::HideAll(7));
        // 它已被隐藏 → 唤醒它(即使刚刚失去前台)
        assert_eq!(decide(me, B, Some(&rec(7, Usage::Hide, 10_000)), sib(true, false), 10_100), Decision::Bring(7));
        assert_eq!(decide(me, B, Some(&rec(7, Usage::Resign, 10_000)), sib(true, false), 10_100), Decision::Bring(7));
        // 很久以前切到别的应用(没隐藏)→ 唤醒最近使用的那个窗口
        assert_eq!(decide(me, B, Some(&rec(7, Usage::Resign, 10_000)), sib(false, false), 60_000), Decision::Bring(7));
    }
}
