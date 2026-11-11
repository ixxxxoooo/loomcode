//! 再次打开应用(点程序坞图标、双击应用、快捷键 / 启动器唤醒)时把窗口带回前台。
//!
//! **多个窗口进程**(⇧⌘N 新窗口是另起一个同 bundle id 的进程):启动器 / 全局快捷键 / `open` 按 bundle id
//! 找应用,只认其中一个进程(通常是先启动的主进程)。「显示 / 隐藏」切换类快捷键判断的是「这个进程在不在最前面」,
//! 所以在另一个窗口进程里按快捷键,它只会把主进程拉到前面,永远隐藏不了当前窗口;被隐藏的另一个进程也收不到「再次打开」。
//! 因此每个进程失去前台或被隐藏时(`NSApplicationDidResignActiveNotification` / `DidHideNotification`,
//! ⌘H、启动器快捷键、程序坞菜单都算)把「怎么了、何时」记进**自己的**文件 `~/.loomcode/run/used-<pid>`
//! (每个进程只写自己的,免得多个进程抢写同一个文件、通知先后不定留下旧记录);
//! 收到「再次打开」的进程按 [`decide`] 处理:在所有同应用进程里找「最近使用」的——此刻在前台的算「现在」,
//! 否则取它最后一次离开的时间(自己也算,取自己的离开时间)。最近使用的是另一个窗口进程且它在前台(或刚被这次快捷键抢走前台)
//! = 在那个窗口里按了切换键,把它和自己一起隐藏;否则**把所有进程的窗口都显示到前台**:发一条分布式通知
//! (`<bundle id>.showAll`),每个进程自己取消隐藏、恢复最小化、`orderFrontRegardless` 抬起全部窗口
//! (不靠激活,所以被别的应用盖住的窗口也能出来),最近使用的那个再激活拿焦点、盖在最上面。
//! (旧版只有一个 `last-used` 文件、只记「最后离开的」:A → B 切换后 A 记一条、B 变前台不记,在 B 里按唤醒键,
//! 记录还是 A,A 就把自己弹出来,而不是处理 B。)
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
            mac::prune_records();
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

/// 进程 `pid` 的使用记录文件:`~/.loomcode/run/used-<pid>`。
fn used_path(pid: i32) -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".loomcode").join("run").join(format!("used-{pid}")))
}

/// 窗口进程最近一次被记录的事件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Usage {
    /// 失去前台(切到别的应用,或被别的进程抢走前台)
    Resign,
    /// 被隐藏
    Hide,
}

/// 一条使用记录(某个进程最后一次离开前台的方式与时间)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Record {
    usage: Usage,
    /// 记录时间(Unix 毫秒)
    ms: u64,
}

impl Record {
    /// 序列化:`<resign|hide> <毫秒>`。
    fn format(&self) -> String {
        let u = match self.usage { Usage::Resign => "resign", Usage::Hide => "hide" };
        format!("{u} {}", self.ms)
    }

    /// 解析;格式不对返回 `None`。
    fn parse(text: &str) -> Option<Self> {
        let mut it = text.split_whitespace();
        let usage = match it.next()? { "resign" => Usage::Resign, "hide" => Usage::Hide, _ => return None };
        let ms = it.next()?.parse().ok()?;
        Some(Self { usage, ms })
    }
}

/// 另一个同应用进程此刻的状态(由调用方从系统与它的记录文件查询)。
#[derive(Debug, Clone, Copy)]
struct Peer {
    pid: i32,
    hidden: bool,
    active: bool,
    /// 它最后一次离开前台的记录(没有记录为 `None`)
    rec: Option<Record>,
}

/// 收到「再次打开」后怎么做。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decision {
    /// 在另一个窗口进程里按了切换键:把它和自己一起隐藏
    HideAll(i32),
    /// 把所有进程的窗口都显示到前台;`front` 是最近使用、要拿焦点的另一个进程(`None` = 自己最近使用)
    ShowAll { front: Option<i32> },
}

/// 「刚被这次快捷键抢走前台」的时间窗:另一个进程失去前台后这么久之内收到的「再次打开」视为在它里面按的。
const TOGGLE_WINDOW_MS: u64 = 1500;

/// 决定收到「再次打开」时怎么做(纯函数)。
///
/// 参数:`own` 本进程最后一次离开前台的记录(本进程此刻多半正被系统激活,所以不看它的前台状态);
/// `peers` 其他同应用进程;`now_ms` 当前时间。
/// 「最近使用」:在前台的进程 = 现在;否则取它的离开时间。最近使用的是自己(或同时刻)则自己拿焦点。
fn decide(own: Option<Record>, peers: &[Peer], now_ms: u64) -> Decision {
    let key = |p: &Peer| if p.active { now_ms } else { p.rec.map_or(0, |r| r.ms) };
    let Some(best) = peers.iter().max_by_key(|p| key(p)) else { return Decision::ShowAll { front: None } };
    if key(best) <= own.map_or(0, |r| r.ms) {
        return Decision::ShowAll { front: None };
    }
    let just_left = matches!(best.rec, Some(Record { usage: Usage::Resign, ms }) if now_ms.saturating_sub(ms) <= TOGGLE_WINDOW_MS);
    if !best.hidden && (best.active || just_left) {
        Decision::HideAll(best.pid)
    } else {
        Decision::ShowAll { front: Some(best.pid) }
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
    /// 先让所有同应用窗口进程把窗口都显示出来;最近使用的是另一个进程时由它拿焦点,自己不再激活(返回 NO)。
    extern "C" fn should_handle_reopen(_this: *mut AnyObject, _sel: Sel, _app: *mut AnyObject, _has_visible: Bool) -> Bool {
        if handle_with_siblings() {
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
        let pid = std::process::id() as i32;
        let Some(path) = super::used_path(pid) else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&path, super::Record { usage, ms: super::now_ms() }.format());
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
        // 「全部显示」的跨进程通知
        if let (Some(dcls), Some(bundle)) = (AnyClass::get(c"NSDistributedNotificationCenter"), own_bundle()) {
            let sel = sel!(loomShowAll:);
            unsafe {
                class_replaceMethod(cls, std::mem::transmute::<Sel, *const c_void>(sel), did_show_all as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject) as *const c_void, c"v@:@".as_ptr());
                let dcenter: Retained<AnyObject> = msg_send![dcls, defaultCenter];
                let name = NSString::from_str(&show_all_name(&bundle));
                // 4 = NSNotificationSuspensionBehaviorDeliverImmediately(被隐藏的应用也立刻收到)
                let _: () = msg_send![&*dcenter, addObserver: &*delegate, selector: sel, name: &*name, object: Option::<&AnyObject>::None, suspensionBehavior: 4usize];
            }
        }
    }

    /// 「全部显示」通知的名字(带 bundle id,开发版与正式版互不干扰)。
    fn show_all_name(bundle: &str) -> String {
        format!("{bundle}.showAll")
    }

    extern "C" fn did_show_all(_this: *mut AnyObject, _sel: Sel, _note: *mut AnyObject) {
        raise_all_windows();
    }

    /// 取消隐藏、恢复最小化,并把本进程所有「真窗口」抬到所有应用的窗口之上(不激活应用,不抢焦点)。
    fn raise_all_windows() {
        unhide_and_restore();
        let Some(mtm) = MainThreadMarker::new() else { return };
        let app = NSApplication::sharedApplication(mtm);
        let windows: Option<Retained<AnyObject>> = unsafe { msg_send![&*app, windows] };
        let Some(windows) = windows else { return };
        let count: usize = unsafe { msg_send![&*windows, count] };
        for i in 0..count {
            let w: Option<Retained<AnyObject>> = unsafe { msg_send![&*windows, objectAtIndex: i] };
            let Some(w) = w else { continue };
            // 只抬主窗口类的窗口,面板 / 提示浮窗不动
            let can: bool = unsafe { msg_send![&*w, canBecomeMainWindow] };
            if can {
                let _: () = unsafe { msg_send![&*w, orderFrontRegardless] };
            }
        }
    }

    /// 通知所有同应用窗口进程(含自己)把窗口全部显示出来。
    fn broadcast_show_all(bundle: &str) {
        let Some(dcls) = AnyClass::get(c"NSDistributedNotificationCenter") else { return };
        let name = NSString::from_str(&show_all_name(bundle));
        unsafe {
            let dcenter: Retained<AnyObject> = msg_send![dcls, defaultCenter];
            let _: () = msg_send![&*dcenter, postNotificationName: &*name, object: Option::<&AnyObject>::None, userInfo: Option::<&AnyObject>::None, deliverImmediately: true];
        }
    }

    /// 读某个进程的使用记录。
    fn read_record(pid: i32) -> Option<super::Record> {
        super::used_path(pid).and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| super::Record::parse(&t))
    }

    /// 清掉已不存在的进程留下的使用记录(启动时调用一次,免得越积越多)。
    pub fn prune_records() {
        let Some(dir) = super::used_path(0).and_then(|p| p.parent().map(|d| d.to_path_buf())) else { return };
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some(pid) = name.strip_prefix("used-").and_then(|n| n.parse::<i32>().ok()) else { continue };
            let alive = NSRunningApplication::runningApplicationWithProcessIdentifier(pid).is_some_and(|a| !a.isTerminated());
            if !alive {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }

    /// 处理「再次打开」:在另一个窗口进程里按了切换键就一起隐藏;否则把所有进程的窗口都显示到前台。
    /// 返回是否已由另一个进程接手焦点(`true` 时本进程不再自己激活)。
    fn handle_with_siblings() -> bool {
        let Some(own) = own_bundle() else { return false };
        let own_pid = std::process::id() as i32;
        // 所有同 bundle id 的其他进程(直接问系统,不靠记录里的 pid,所以不怕 pid 被复用)
        let running = NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(&own));
        let others: Vec<Retained<NSRunningApplication>> =
            running.iter().filter(|a| a.processIdentifier() != own_pid && !a.isTerminated()).collect();
        if others.is_empty() {
            return false;
        }
        let peers: Vec<super::Peer> = others
            .iter()
            .map(|a| {
                let pid = a.processIdentifier();
                super::Peer { pid, hidden: a.isHidden(), active: a.isActive(), rec: read_record(pid) }
            })
            .collect();
        let now = super::now_ms();
        let decision = super::decide(read_record(own_pid), &peers, now);
        let Some(mtm) = MainThreadMarker::new() else { return false };
        let app = NSApplication::sharedApplication(mtm);
        // 本进程替别的进程处理期间,自己的失去前台 / 隐藏不记录
        QUIET_UNTIL.store(now + super::TOGGLE_WINDOW_MS, Ordering::Relaxed);
        match decision {
            super::Decision::HideAll(pid) => {
                loom_diag::info!("再次打开:在另一个窗口进程(pid {pid})里按了切换键,一起隐藏");
                for o in &others {
                    o.hide();
                }
                app.hide(None);
                true
            }
            super::Decision::ShowAll { front } => {
                loom_diag::info!("再次打开:显示全部窗口(共 {} 个进程),焦点给 {front:?}", others.len() + 1);
                // 自己与其他进程都抬起窗口;其他进程经分布式通知各自处理
                raise_all_windows();
                broadcast_show_all(&own);
                let Some(other) = front.and_then(|pid| others.iter().find(|a| a.processIdentifier() == pid)) else { return false };
                // macOS 14 起激活改为「协作式」:先把激活权让给它,它才能真正到前台
                let responds: bool = unsafe { msg_send![&*app, respondsToSelector: sel!(yieldActivationToApplication:)] };
                if responds {
                    let _: () = unsafe { msg_send![&*app, yieldActivationToApplication: &**other] };
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

    fn rec(usage: Usage, ms: u64) -> Option<Record> {
        Some(Record { usage, ms })
    }

    fn peer(pid: i32, hidden: bool, active: bool, rec: Option<Record>) -> Peer {
        Peer { pid, hidden, active, rec }
    }

    #[::core::prelude::v1::test]
    fn 使用记录的读写() {
        let r = Record { usage: Usage::Resign, ms: 1_700_000_000_123 };
        assert_eq!(Record::parse(&r.format()), Some(r));
        assert_eq!(Record::parse("hide 5\n"), Some(Record { usage: Usage::Hide, ms: 5 }));
        assert_eq!(Record::parse("hide"), None);
        assert_eq!(Record::parse("x 1"), None);
    }

    const ME: Decision = Decision::ShowAll { front: None };

    #[::core::prelude::v1::test]
    fn 收到再次打开时的决定() {
        // 没有其他进程:自己拿焦点
        assert_eq!(decide(None, &[], 0), ME);
        // 其他进程从没用过(没记录、不在前台):自己拿焦点
        assert_eq!(decide(rec(Usage::Hide, 5), &[peer(7, true, false, None)], 10_000), ME);
        // 在另一个窗口里按了切换键:它还在前台,或刚刚失去前台 → 一起隐藏
        assert_eq!(decide(None, &[peer(7, false, true, rec(Usage::Resign, 1))], 10_000), Decision::HideAll(7));
        assert_eq!(decide(None, &[peer(7, false, false, rec(Usage::Resign, 10_000))], 10_800), Decision::HideAll(7));
        // 它已被隐藏 → 全部显示,焦点给它(即使刚刚失去前台)
        assert_eq!(decide(None, &[peer(7, true, false, rec(Usage::Hide, 10_000))], 10_100), Decision::ShowAll { front: Some(7) });
        assert_eq!(decide(None, &[peer(7, true, false, rec(Usage::Resign, 10_000))], 10_100), Decision::ShowAll { front: Some(7) });
        // 很久以前切到别的应用(没隐藏)→ 全部显示,焦点给最近使用的那个窗口
        assert_eq!(decide(None, &[peer(7, false, false, rec(Usage::Resign, 10_000))], 60_000), Decision::ShowAll { front: Some(7) });
    }

    #[::core::prelude::v1::test]
    fn 最近使用的是自己时自己拿焦点() {
        // 自己比对方更晚离开前台:最近使用的是自己
        assert_eq!(decide(rec(Usage::Resign, 20_000), &[peer(7, false, false, rec(Usage::Resign, 10_000))], 60_000), ME);
        assert_eq!(decide(rec(Usage::Hide, 20_000), &[peer(7, true, false, rec(Usage::Hide, 10_000))], 60_000), ME);
    }

    /// 旧版的漏洞:A → B 切换后 A 的离开时间更晚于 B 的上一次记录,但 B 此刻在前台,
    /// 在 B 里按唤醒键(通知送到 A)应当处理 B,而不是把 A 弹出来。
    #[::core::prelude::v1::test]
    fn 在前台的另一个进程总是最近使用() {
        let own = rec(Usage::Resign, 50_000);
        assert_eq!(decide(own, &[peer(7, false, true, rec(Usage::Resign, 1_000))], 60_000), Decision::HideAll(7));
    }

    #[::core::prelude::v1::test]
    fn 多个其他进程取最近使用的那个() {
        let peers = [
            peer(7, true, false, rec(Usage::Hide, 10_000)),
            peer(8, true, false, rec(Usage::Hide, 30_000)),
            peer(9, true, false, rec(Usage::Hide, 20_000)),
        ];
        assert_eq!(decide(rec(Usage::Hide, 5_000), &peers, 90_000), Decision::ShowAll { front: Some(8) });
        assert_eq!(decide(rec(Usage::Hide, 40_000), &peers, 90_000), ME);
    }
}
