//! 再次打开应用(点程序坞图标、双击应用、快捷键 / 启动器唤醒)时把窗口带回前台。
//!
//! **多个窗口进程当成一个应用**(⇧⌘N 新窗口是另起一个同 bundle id 的进程):
//! 启动器 / 全局快捷键(如 Manico 绑的 ⌥X)按 bundle id 找应用,只认其中一个进程(通常是先启动的主进程),
//! 「显示 / 隐藏」切换只对它做:它在前台就隐藏它,否则给它发「再次打开」。所以:
//! - **收到「再次打开」**(不是程序坞发来的):看**按下前最前面的应用**(本进程用 `NSWorkspace` 的
//!   「应用被激活」通知记下的激活历史,去掉这次「再次打开」本身引起的自己的激活)。
//!   是另一个 LoomCode 窗口进程 = 用户在那个窗口里按了切换键 → 全部隐藏;
//!   否则 → 全部显示,焦点给激活历史里最近使用的 LoomCode 进程。纯函数 [`decide`]。
//! - **全部显示**:发分布式通知 `<bundle id>.showAll`,每个进程自己 `unhideWithoutActivation`、恢复最小化、
//!   `orderFrontRegardless` 抬起主窗口类的窗口(都不激活,免得几个进程互相抢前台;被别的应用盖住的窗口也能出来);
//!   0.2 秒后(等各进程抬完窗口)再激活要拿焦点的那个,让它盖在最上面。
//! - **全部隐藏**:发 `<bundle id>.hideAll`,各进程自己隐藏。
//! - **某个进程被外部隐藏**(启动器在它在前台时直接隐藏它、⌘H):`NSApplicationDidHideNotification` 里同样广播
//!   全部隐藏;我们自己发起的隐藏(红色关闭按钮 / ⌘W 只隐藏本进程、收到广播后的隐藏)先 [`hide_self`] 标记,不再广播。
//! - **程序坞点击**(「再次打开」的发送者是 `com.apple.dock`)与只有一个进程时:照常只把自己带回前台。
//!
//! 为什么要自己处理「再次打开」:gpui 把 `applicationShouldHandleReopen:hasVisibleWindows:` 声明成没有返回值,
//! 而 AppKit 期望一个 BOOL;返回值是随机的,为真时系统才会自己取消隐藏、恢复窗口,
//! 所以「有时能打开、有时点了没反应」(实测:关窗后连续再次打开,约三次里有一次不恢复)。
//! 而且窗口只是被隐藏时 `hasVisibleWindows` 为真,gpui 连自己的 `on_reopen` 回调都不调用。
//! 所以运行时把代理的这个方法换成我们的实现,明确返回 YES / NO。
//!
//! 旧做法的坑(0.1.19 / 0.1.20):各进程在失去前台 / 被隐藏时写「最后离开的时间」,收到「再次打开」时据此猜谁是最近使用、
//! 1.5 秒内刚离开就当作「在它里面按了切换键」。这些通知跨进程送达有延迟、会乱序,实测按一次「全部显示」紧接着下一次被误判成
//! 「全部隐藏」;再加上 `unhide:` 自带激活,几个进程互相抢前台,最后窗口一个都没出来。
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
            mac::watch();
            mac::remove_old_records();
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

/// 只隐藏本进程(红色关闭按钮、⌘W):不连带其他窗口进程。
pub fn hide_self(cx: &mut App) {
    #[cfg(target_os = "macos")]
    mac::mark_self_hide();
    cx.hide();
}

/// 关闭 Agent 窗口(红色关闭按钮、⌘W)。
///
/// 独立 IDE 窗口开着时只把当前窗口藏起来(`orderOut`),IDE 窗口留在屏幕上;
/// 否则隐藏整个应用(`cx.hide()` 会连带隐藏本进程的所有窗口,所以不能在 IDE 开着时用)。
/// 藏起来的窗口之后由「再次打开」/ 程序坞 / 快捷键带回(`activate_window` 会重新显示它)。
pub fn hide_agent_window(cx: &mut App) {
    let ide_open = cx
        .try_global::<crate::WorkbenchRef>()
        .and_then(|r| r.0.upgrade())
        .is_some_and(|wb| wb.read(cx).ide_window.is_some_and(|h| cx.windows().contains(&h)));
    #[cfg(target_os = "macos")]
    if ide_open && mac::hide_key_window() {
        return;
    }
    let _ = ide_open;
    hide_self(cx);
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

/// 收到「再次打开」后怎么做。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decision {
    /// 只把自己带回前台(程序坞点击、只有一个进程)
    Own,
    /// 按下前最前面的就是另一个 LoomCode 窗口进程:全部隐藏
    HideAll,
    /// 全部显示,焦点给 `focus`(进程号,可能是自己)
    ShowAll { focus: i32 },
}

/// 激活历史里自己的最后一条若在这么久之内,视为这次「再次打开」本身引起的激活,不算「按下前最前面的应用」。
const FRESH_MS: u64 = 2000;

/// 激活历史最多保留的条数。
#[cfg(target_os = "macos")]
const HISTORY_MAX: usize = 32;

/// 决定收到「再次打开」时怎么做(纯函数)。
///
/// 参数:`own` 本进程号;`ours` 所有同应用进程号(含自己);`history` 应用激活历史 `(进程号, 毫秒)`,旧 → 新;
/// `from_dock` 是否程序坞发来的;`now` 当前毫秒。
fn decide(own: i32, ours: &[i32], history: &[(i32, u64)], from_dock: bool, now: u64) -> Decision {
    if from_dock || ours.iter().all(|&p| p == own) {
        return Decision::Own;
    }
    let mut h = history;
    if let Some(&(pid, ms)) = h.last()
        && pid == own
        && now.saturating_sub(ms) <= FRESH_MS
    {
        h = &h[..h.len() - 1];
    }
    match h.last() {
        Some(&(prev, _)) if prev != own && ours.contains(&prev) => Decision::HideAll,
        _ => {
            let focus = h.iter().rev().find(|(p, _)| ours.contains(p)).map_or(own, |&(p, _)| p);
            Decision::ShowAll { focus }
        }
    }
}

/// 当前 Unix 毫秒。
#[cfg(target_os = "macos")]
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
    use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
    use std::sync::{Mutex, OnceLock};

    unsafe extern "C" {
        fn object_getClass(obj: *const c_void) -> *const c_void;
        fn class_replaceMethod(cls: *const c_void, name: *const c_void, imp: *const c_void, types: *const c_char) -> *const c_void;
    }

    /// 通知界面「把主窗口带回前台」的通道(代理方法在主线程调用,界面侧的任务接收)。
    static TX: OnceLock<async_channel::Sender<()>> = OnceLock::new();

    /// 应用激活历史 `(进程号, 毫秒)`,旧 → 新(所有应用,不只本应用)。
    static HISTORY: Mutex<Vec<(i32, u64)>> = Mutex::new(Vec::new());

    /// 这个时刻之前本进程被隐藏不算「外部隐藏」(不广播全部隐藏)。Unix 毫秒。
    static SELF_HIDE_UNTIL: AtomicU64 = AtomicU64::new(0);

    /// 「全部显示」之后要拿焦点的进程(延迟激活时读取)。
    static FOCUS_PID: AtomicI32 = AtomicI32::new(0);

    /// 等其他进程抬完窗口再激活焦点进程的延迟(秒)。
    const FOCUS_DELAY: f64 = 0.2;

    /// 「全部显示」「全部隐藏」两条跨进程通知(名字前面加 bundle id,开发版与正式版互不干扰)。
    const SHOW_ALL: &str = "showAll";
    const HIDE_ALL: &str = "hideAll";

    /// 本进程号。
    fn own_pid() -> i32 {
        std::process::id() as i32
    }

    /// 本进程的 bundle id;不在 `.app` 包里运行时为 `None`(此时不做跨进程处理)。
    fn own_bundle() -> Option<String> {
        NSRunningApplication::currentApplication().bundleIdentifier().map(|s| s.to_string())
    }

    /// 标记接下来的一次隐藏是本进程自己发起的(不广播给其他窗口进程)。
    pub fn mark_self_hide() {
        SELF_HIDE_UNTIL.store(super::now_ms() + 1000, Ordering::Relaxed);
    }

    /// 应用与它的代理(gpui 设置的);没有代理时为 `None`。
    fn app_delegate() -> Option<(Retained<NSApplication>, Retained<AnyObject>)> {
        let mtm = MainThreadMarker::new()?;
        let app = NSApplication::sharedApplication(mtm);
        let delegate: Option<Retained<AnyObject>> = unsafe { msg_send![&*app, delegate] };
        Some((app, delegate?))
    }

    /// 给代理的类加 / 换一个方法。
    unsafe fn add_method(delegate: &AnyObject, sel: Sel, imp: *const c_void, types: &std::ffi::CStr) {
        unsafe {
            let cls = object_getClass(delegate as *const AnyObject as *const c_void);
            class_replaceMethod(cls, std::mem::transmute::<Sel, *const c_void>(sel), imp, types.as_ptr());
        }
    }

    /// 把应用代理的「再次打开」方法换成 [`should_handle_reopen`]。返回是否成功(没有代理时失败)。
    pub fn hook_delegate(tx: async_channel::Sender<()>) -> bool {
        let Some((_, delegate)) = app_delegate() else {
            loom_diag::warn!("应用代理还没设置,无法接管「再次打开」");
            return false;
        };
        let _ = TX.set(tx);
        // BOOL 在 arm64 上是 bool('B'),x86_64 上是 signed char('c')
        let types = if cfg!(target_arch = "aarch64") { c"B@:@B" } else { c"c@:@c" };
        let imp = should_handle_reopen as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject, Bool) -> Bool as *const c_void;
        unsafe { add_method(&delegate, sel!(applicationShouldHandleReopen:hasVisibleWindows:), imp, types) };
        loom_diag::info!("已接管「再次打开」通知");
        true
    }

    /// 新的 `applicationShouldHandleReopen:hasVisibleWindows:`。
    extern "C" fn should_handle_reopen(_this: *mut AnyObject, _sel: Sel, _app: *mut AnyObject, _has_visible: Bool) -> Bool {
        let own = own_pid();
        let sender = reopen_sender();
        let from_dock = sender.as_deref() == Some("com.apple.dock");
        let ours = our_pids();
        let history = HISTORY.lock().map(|h| h.clone()).unwrap_or_default();
        let now = super::now_ms();
        let decision = super::decide(own, &ours, &history, from_dock, now);
        let recent: Vec<String> = history.iter().rev().take(4).map(|(p, ms)| format!("{p}@-{}ms", now.saturating_sub(*ms))).collect();
        loom_diag::info!("再次打开:发送者={} 本应用进程={ours:?} 最近激活=[{}] → {decision:?}", sender.as_deref().unwrap_or("?"), recent.join(", "));
        match decision {
            super::Decision::Own => {
                unhide_and_restore();
                if let Some(tx) = TX.get() {
                    let _ = tx.try_send(());
                }
                Bool::YES
            }
            super::Decision::HideAll => {
                broadcast(HIDE_ALL);
                mark_self_hide();
                if let Some((app, _)) = app_delegate() {
                    app.hide(None);
                }
                Bool::NO
            }
            super::Decision::ShowAll { focus } => {
                raise_windows();
                broadcast(SHOW_ALL);
                FOCUS_PID.store(focus, Ordering::Relaxed);
                if let Some((_, delegate)) = app_delegate() {
                    let _: () = unsafe { msg_send![&*delegate, performSelector: sel!(loomFocus:), withObject: Option::<&AnyObject>::None, afterDelay: FOCUS_DELAY] };
                }
                Bool::NO
            }
        }
    }

    /// 这次「再次打开」苹果事件的发送者 bundle id(取不到为 `None`)。
    fn reopen_sender() -> Option<String> {
        let mgr_cls = AnyClass::get(c"NSAppleEventManager")?;
        unsafe {
            let mgr: Option<Retained<AnyObject>> = msg_send![mgr_cls, sharedAppleEventManager];
            let ev: Option<Retained<AnyObject>> = msg_send![&*mgr?, currentAppleEvent];
            // keySenderPIDAttr = 'spid'
            let desc: Option<Retained<AnyObject>> = msg_send![&*ev?, attributeDescriptorForKeyword: 0x7370_6964u32];
            let pid: i32 = msg_send![&*desc?, int32Value];
            if pid <= 0 {
                return None;
            }
            NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?.bundleIdentifier().map(|s| s.to_string())
        }
    }

    /// 所有同 bundle id、还在运行的进程号(含自己)。
    fn our_pids() -> Vec<i32> {
        let own = own_pid();
        let Some(bundle) = own_bundle() else { return vec![own] };
        let running = NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(&bundle));
        let mut pids: Vec<i32> = running.iter().filter(|a| !a.isTerminated()).map(|a| a.processIdentifier()).collect();
        if !pids.contains(&own) {
            pids.push(own);
        }
        pids
    }

    /// 延迟到点:激活要拿焦点的进程。
    extern "C" fn did_focus(_this: *mut AnyObject, _sel: Sel, _arg: *mut AnyObject) {
        let focus = FOCUS_PID.load(Ordering::Relaxed);
        if focus == own_pid() || focus == 0 {
            raise_windows();
            if let Some(tx) = TX.get() {
                let _ = tx.try_send(());
            }
            return;
        }
        let Some(other) = NSRunningApplication::runningApplicationWithProcessIdentifier(focus) else { return };
        let Some((app, _)) = app_delegate() else { return };
        // macOS 14 起激活改为「协作式」:先把激活权让给它,它才能真正到前台
        let responds: bool = unsafe { msg_send![&*app, respondsToSelector: sel!(yieldActivationToApplication:)] };
        if responds {
            let _: () = unsafe { msg_send![&*app, yieldActivationToApplication: &*other] };
        }
        let ok = other.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows);
        loom_diag::info!("再次打开:焦点给窗口进程 pid {focus}(激活{})", if ok { "成功" } else { "失败" });
    }

    /// 跨进程通知的完整名字。
    fn note_name(kind: &str) -> Option<Retained<NSString>> {
        own_bundle().map(|b| NSString::from_str(&format!("{b}.{kind}")))
    }

    /// 通知其他同应用窗口进程;`object` 带上本进程号,收到的进程据此跳过自己发的。
    fn broadcast(kind: &str) {
        let (Some(cls), Some(name)) = (AnyClass::get(c"NSDistributedNotificationCenter"), note_name(kind)) else { return };
        let sender = NSString::from_str(&own_pid().to_string());
        unsafe {
            let center: Retained<AnyObject> = msg_send![cls, defaultCenter];
            let _: () = msg_send![&*center, postNotificationName: &*name, object: &*sender, userInfo: Option::<&AnyObject>::None, deliverImmediately: true];
        }
    }

    /// 跨进程通知是不是本进程自己发的。
    fn from_self(note: *mut AnyObject) -> bool {
        let Some(note) = (unsafe { note.as_ref() }) else { return true };
        let object: Option<Retained<NSString>> = unsafe { msg_send![note, object] };
        object.is_some_and(|o| o.to_string() == own_pid().to_string())
    }

    extern "C" fn did_show_all(_this: *mut AnyObject, _sel: Sel, note: *mut AnyObject) {
        if !from_self(note) {
            raise_windows();
        }
    }

    extern "C" fn did_hide_all(_this: *mut AnyObject, _sel: Sel, note: *mut AnyObject) {
        if from_self(note) {
            return;
        }
        mark_self_hide();
        if let Some((app, _)) = app_delegate() {
            app.hide(None);
        }
    }

    /// 本进程被隐藏:不是自己发起的(启动器、⌘H)就让其他窗口进程一起隐藏。
    extern "C" fn did_hide(_this: *mut AnyObject, _sel: Sel, _note: *mut AnyObject) {
        if super::now_ms() < SELF_HIDE_UNTIL.load(Ordering::Relaxed) {
            return;
        }
        if our_pids().len() > 1 {
            loom_diag::info!("本窗口进程被外部隐藏,其他窗口进程一起隐藏");
            broadcast(HIDE_ALL);
        }
    }

    /// 某个应用被激活(`NSWorkspaceDidActivateApplicationNotification`):记进激活历史。
    extern "C" fn did_activate_app(_this: *mut AnyObject, _sel: Sel, note: *mut AnyObject) {
        let Some(note) = (unsafe { note.as_ref() }) else { return };
        let pid: Option<i32> = unsafe {
            let info: Option<Retained<AnyObject>> = msg_send![note, userInfo];
            let key = NSString::from_str("NSWorkspaceApplicationKey");
            info.and_then(|info| {
                let app: Option<Retained<AnyObject>> = msg_send![&*info, objectForKey: &*key];
                app.map(|a| msg_send![&*a, processIdentifier])
            })
        };
        if let Some(pid) = pid {
            push_history(pid);
        }
    }

    /// 追加一条激活记录(与最后一条相同的进程只更新时间)。
    fn push_history(pid: i32) {
        let Ok(mut h) = HISTORY.lock() else { return };
        let now = super::now_ms();
        if let Some(last) = h.last_mut()
            && last.0 == pid
        {
            last.1 = now;
            return;
        }
        h.push((pid, now));
        let over = h.len().saturating_sub(super::HISTORY_MAX);
        h.drain(..over);
    }

    /// 注册各种观察:本进程被隐藏、任意应用被激活、两条跨进程通知;以及延迟激活用的方法。
    /// 不用代理的 `applicationDidHide:` 等:代理是 gpui 设置的,设置之后再加的方法 AppKit 不会自动接上。
    pub fn watch() {
        let Some((app, delegate)) = app_delegate() else { return };
        type Imp = extern "C" fn(*mut AnyObject, Sel, *mut AnyObject);
        let methods: [(Sel, Imp); 5] = [
            (sel!(loomDidHide:), did_hide),
            (sel!(loomDidActivateApp:), did_activate_app),
            (sel!(loomShowAll:), did_show_all),
            (sel!(loomHideAll:), did_hide_all),
            (sel!(loomFocus:), did_focus),
        ];
        for (sel, imp) in methods {
            unsafe { add_method(&delegate, sel, imp as *const c_void, c"v@:@") };
        }
        unsafe {
            // 本进程被隐藏
            if let Some(cls) = AnyClass::get(c"NSNotificationCenter") {
                let center: Retained<AnyObject> = msg_send![cls, defaultCenter];
                let name = NSString::from_str("NSApplicationDidHideNotification");
                let _: () = msg_send![&*center, addObserver: &*delegate, selector: sel!(loomDidHide:), name: &*name, object: &*app];
            }
            // 任意应用被激活(按键前最前面的是谁)
            if let Some(cls) = AnyClass::get(c"NSWorkspace") {
                let ws: Retained<AnyObject> = msg_send![cls, sharedWorkspace];
                let center: Retained<AnyObject> = msg_send![&*ws, notificationCenter];
                let name = NSString::from_str("NSWorkspaceDidActivateApplicationNotification");
                let _: () = msg_send![&*center, addObserver: &*delegate, selector: sel!(loomDidActivateApp:), name: &*name, object: Option::<&AnyObject>::None];
                // 先记下此刻最前面的应用
                let front: Option<Retained<AnyObject>> = msg_send![&*ws, frontmostApplication];
                if let Some(front) = front {
                    let pid: i32 = msg_send![&*front, processIdentifier];
                    push_history(pid);
                }
            }
            // 跨进程「全部显示 / 全部隐藏」;4 = NSNotificationSuspensionBehaviorDeliverImmediately(被隐藏的应用也立刻收到)
            if let Some(cls) = AnyClass::get(c"NSDistributedNotificationCenter") {
                let center: Retained<AnyObject> = msg_send![cls, defaultCenter];
                for (kind, sel) in [(SHOW_ALL, sel!(loomShowAll:)), (HIDE_ALL, sel!(loomHideAll:))] {
                    let Some(name) = note_name(kind) else { continue };
                    let _: () = msg_send![&*center, addObserver: &*delegate, selector: sel, name: &*name, object: Option::<&AnyObject>::None, suspensionBehavior: 4usize];
                }
            }
        }
    }

    /// 删掉旧版本留下的使用记录文件(`last-used` / `last-hidden` / `used-<pid>`)。
    pub fn remove_old_records() {
        let Some(home) = std::env::var_os("HOME") else { return };
        let dir = std::path::PathBuf::from(home).join(".loomcode").join("run");
        let Ok(entries) = std::fs::read_dir(&dir) else { return };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name == "last-used" || name == "last-hidden" || name.starts_with("used-") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }

    /// 取消隐藏(不激活)、恢复最小化,并把主窗口类的窗口抬到所有应用的窗口之上(不抢焦点)。
    fn raise_windows() {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let app = NSApplication::sharedApplication(mtm);
        app.unhideWithoutActivation();
        for_each_window(&app, |w| unsafe {
            let mini: bool = msg_send![w, isMiniaturized];
            if mini {
                let _: () = msg_send![w, deminiaturize: Option::<&AnyObject>::None];
            }
            // 只抬主窗口类的窗口,面板 / 提示浮窗不动
            let can: bool = msg_send![w, canBecomeMainWindow];
            if can {
                let _: () = msg_send![w, orderFrontRegardless];
            }
        });
    }

    /// 只把当前的键盘窗口藏起来(不隐藏应用,其他窗口不受影响)。没有键盘窗口返回 `false`。
    pub fn hide_key_window() -> bool {
        let Some(mtm) = MainThreadMarker::new() else { return false };
        let app = NSApplication::sharedApplication(mtm);
        let w: Option<Retained<AnyObject>> = unsafe { msg_send![&*app, keyWindow] };
        let Some(w) = w else { return false };
        unsafe {
            let _: () = msg_send![&*w, orderOut: Option::<&AnyObject>::None];
        }
        true
    }

    /// 对应用的每个窗口执行 `f`。
    fn for_each_window(app: &NSApplication, mut f: impl FnMut(&AnyObject)) {
        let windows: Option<Retained<AnyObject>> = unsafe { msg_send![app, windows] };
        let Some(windows) = windows else { return };
        let count: usize = unsafe { msg_send![&*windows, count] };
        for i in 0..count {
            let w: Option<Retained<AnyObject>> = unsafe { msg_send![&*windows, objectAtIndex: i] };
            if let Some(w) = w {
                f(&w);
            }
        }
    }

    /// 取消应用隐藏,并恢复所有被最小化到程序坞的窗口(`makeKeyAndOrderFront` 不会恢复最小化的窗口)。
    pub fn unhide_and_restore() {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let app = NSApplication::sharedApplication(mtm);
        app.unhide(None);
        for_each_window(&app, |w| unsafe {
            let mini: bool = msg_send![w, isMiniaturized];
            if mini {
                let _: () = msg_send![w, deminiaturize: Option::<&AnyObject>::None];
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: i32 = 100;
    const B: i32 = 200;
    /// 别的应用(浏览器之类)
    const X: i32 = 900;
    const NOW: u64 = 100_000;

    #[::core::prelude::v1::test]
    fn 只有一个进程或程序坞点击时只管自己() {
        assert_eq!(decide(A, &[A], &[(X, 1)], false, NOW), Decision::Own);
        assert_eq!(decide(A, &[A, B], &[(B, 1)], true, NOW), Decision::Own);
    }

    #[::core::prelude::v1::test]
    fn 在另一个窗口里按切换键全部隐藏() {
        // 用户在 B 里按 ⌥X,启动器只认 A:A 被激活后收到「再次打开」
        assert_eq!(decide(A, &[A, B], &[(X, 1), (B, 50_000), (A, NOW - 100)], false, NOW), Decision::HideAll);
        // A 自己的激活通知还没到
        assert_eq!(decide(A, &[A, B], &[(X, 1), (B, 50_000)], false, NOW), Decision::HideAll);
    }

    #[::core::prelude::v1::test]
    fn 在别的应用里按键全部显示且焦点给最近使用的() {
        // 最近用的是 B,之后切到浏览器
        assert_eq!(decide(A, &[A, B], &[(A, 10), (B, 20), (X, 30), (A, NOW - 50)], false, NOW), Decision::ShowAll { focus: B });
        // 最近用的是 A
        assert_eq!(decide(A, &[A, B], &[(B, 10), (A, 20), (X, 30)], false, NOW), Decision::ShowAll { focus: A });
        // 全部隐藏后(系统把前台交给了别的应用)再按:全部显示
        assert_eq!(decide(A, &[A, B], &[(A, 10), (B, 20), (X, 25)], false, NOW), Decision::ShowAll { focus: B });
        // 历史里一个本应用进程都没有:焦点给自己
        assert_eq!(decide(A, &[A, B], &[(X, 30)], false, NOW), Decision::ShowAll { focus: A });
    }

    #[::core::prelude::v1::test]
    fn 已退出的进程不算() {
        // 300 已退出(不在 ours 里):不当成本应用进程
        assert_eq!(decide(A, &[A, B], &[(B, 10), (300, 20)], false, NOW), Decision::ShowAll { focus: B });
    }

    #[::core::prelude::v1::test]
    fn 自己很早以前的激活不被当成这次引起的() {
        // 自己一直在前台:不隐藏,照常显示
        assert_eq!(decide(A, &[A, B], &[(B, 10), (A, 20)], false, NOW), Decision::ShowAll { focus: A });
    }
}
