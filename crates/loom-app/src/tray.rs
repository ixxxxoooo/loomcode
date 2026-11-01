//! 菜单栏托盘(参考 OhMyAgent):模板图标 + 右下角的彩色状态点,菜单里是状态摘要、
//! 打开 / 新对话 / 最近会话(带状态点)/ 设置 / 检查更新 / 退出。
//!
//! 状态点:运行中橙色实心、等待你处理琥珀色空心环,两者都「呼吸」(透明度随时间起伏,约 1.6 秒一个来回);
//! 有未读结果时蓝色实心静止;都没有就不显示。界面线程每 66 毫秒调用 [`tick`] 驱动呼吸,没有运行中会话时什么也不做。
//! 菜单里点选项通过 [`commands`] 通道交给工作台处理。
//!
//! @author ixxxxoooo

use std::sync::OnceLock;

/// 托盘菜单发给工作台的命令。
#[derive(Debug, Clone, PartialEq)]
pub enum TrayCommand {
    /// 显示主窗口
    Show,
    /// 新建会话
    NewSession,
    /// 打开设置
    OpenSettings,
    /// 打开某个会话(会话 id)
    OpenSession(String),
    /// 全部标为已读
    MarkAllRead,
    /// 检查更新
    CheckUpdate,
}

/// 会话在托盘里的状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayState {
    Idle,
    Running,
    /// 等待用户(权限 / 提问 / 计划)
    Waiting,
    /// 有未读结果
    Unread,
}

/// 菜单里的一个会话。
#[derive(Debug, Clone, PartialEq)]
pub struct TraySession {
    pub id: String,
    pub title: String,
    pub project: String,
    pub state: TrayState,
}

/// 托盘需要的全部数据。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TraySnapshot {
    pub sessions: Vec<TraySession>,
}

/// 命令通道(菜单点击在主线程发,工作台里的任务接收)。
static CHANNEL: OnceLock<(async_channel::Sender<TrayCommand>, async_channel::Receiver<TrayCommand>)> = OnceLock::new();

fn channel() -> &'static (async_channel::Sender<TrayCommand>, async_channel::Receiver<TrayCommand>) {
    CHANNEL.get_or_init(async_channel::unbounded)
}

/// 取命令接收端(工作台启动时调用一次,循环处理)。
pub fn commands() -> async_channel::Receiver<TrayCommand> {
    channel().1.clone()
}

/// 菜单栏状态点该显示什么:等待处理优先,其次运行中,再次未读;都没有为 `Idle`。
pub fn indicator(snap: &TraySnapshot) -> TrayState {
    let has = |s: TrayState| snap.sessions.iter().any(|x| x.state == s);
    [TrayState::Waiting, TrayState::Running, TrayState::Unread].into_iter().find(|s| has(*s)).unwrap_or(TrayState::Idle)
}

/// 状态摘要:`1 个会话运行中 · 2 个等待处理 · 3 个未读完成`;都没有返回空串。
pub fn summary(snap: &TraySnapshot) -> String {
    let count = |s: TrayState| snap.sessions.iter().filter(|x| x.state == s).count();
    let mut parts = Vec::new();
    for (state, label) in [(TrayState::Running, "运行中"), (TrayState::Waiting, "等待处理"), (TrayState::Unread, "未读完成")] {
        let n = count(state);
        if n > 0 {
            parts.push(format!("{n} 个会话{label}"));
        }
    }
    parts.join(" · ")
}

/// 菜单里的会话标题:最多 14 个字符,超出加省略号(完整标题放进悬浮提示)。
pub fn menu_title(title: &str) -> String {
    const MAX: usize = 14;
    if title.chars().count() <= MAX { title.to_string() } else { format!("{}…", title.chars().take(MAX).collect::<String>()) }
}

/// 呼吸透明度:约 1.6 秒一个来回,在 0.3 到 1.0 之间平滑起伏。
///
/// 参数:`secs` 动画已进行的秒数。
pub fn breath_alpha(secs: f64) -> f64 {
    0.65 + 0.35 * (secs * std::f64::consts::TAU / 1.6).cos()
}

/// 放置或移除菜单栏托盘图标(仅 macOS;其他平台什么也不做)。
pub fn set_visible(visible: bool) {
    #[cfg(target_os = "macos")]
    if visible { mac::install() } else { mac::remove() }
    #[cfg(not(target_os = "macos"))]
    let _ = visible;
}

/// 更新托盘数据(状态点、提示与菜单内容);内容没变不重建。
pub fn set_snapshot(snap: TraySnapshot) {
    #[cfg(target_os = "macos")]
    mac::apply(snap);
    #[cfg(not(target_os = "macos"))]
    let _ = snap;
}

/// 推进呼吸动画一帧(界面线程每 66 毫秒调一次)。
pub fn tick() {
    #[cfg(target_os = "macos")]
    mac::tick();
}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
    use objc2_app_kit::{NSApplication, NSImage, NSImageView, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength, NSView};
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
    use std::cell::RefCell;
    use std::time::Instant;

    const TRAY: &[u8] = include_bytes!("../../../assets/icon/tray.png");
    const DOT_IDLE: &[u8] = include_bytes!("../../../assets/icon/tray-idle.png");
    const DOT_RUNNING: &[u8] = include_bytes!("../../../assets/icon/tray-running.png");
    const DOT_WAITING: &[u8] = include_bytes!("../../../assets/icon/tray-waiting.png");
    const DOT_UNREAD: &[u8] = include_bytes!("../../../assets/icon/tray-unread.png");

    define_class!(
        /// 托盘菜单项的动作接收者:把点击转成 [`TrayCommand`]。
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "LoomCodeTrayTarget"]
        struct TrayTarget;

        impl TrayTarget {
            #[unsafe(method(showApp:))]
            fn show_app(&self, _sender: Option<&AnyObject>) {
                let app = NSApplication::sharedApplication(self.mtm());
                app.unhide(None);
                #[allow(deprecated)]
                app.activateIgnoringOtherApps(true);
                send(TrayCommand::Show);
            }

            #[unsafe(method(newSession:))]
            fn new_session(&self, _sender: Option<&AnyObject>) {
                let app = NSApplication::sharedApplication(self.mtm());
                #[allow(deprecated)]
                app.activateIgnoringOtherApps(true);
                send(TrayCommand::NewSession);
            }

            #[unsafe(method(openSettings:))]
            fn open_settings(&self, _sender: Option<&AnyObject>) {
                let app = NSApplication::sharedApplication(self.mtm());
                #[allow(deprecated)]
                app.activateIgnoringOtherApps(true);
                send(TrayCommand::OpenSettings);
            }

            #[unsafe(method(openSession:))]
            fn open_session(&self, sender: Option<&AnyObject>) {
                // 会话 id 放在菜单项的 representedObject 里
                let id: Option<Retained<NSString>> = sender.and_then(|s| unsafe { msg_send![s, representedObject] });
                if let Some(id) = id {
                    let app = NSApplication::sharedApplication(self.mtm());
                    #[allow(deprecated)]
                    app.activateIgnoringOtherApps(true);
                    send(TrayCommand::OpenSession(id.to_string()));
                }
            }

            #[unsafe(method(markAllRead:))]
            fn mark_all_read(&self, _sender: Option<&AnyObject>) {
                send(TrayCommand::MarkAllRead);
            }

            #[unsafe(method(checkUpdate:))]
            fn check_update(&self, _sender: Option<&AnyObject>) {
                let app = NSApplication::sharedApplication(self.mtm());
                #[allow(deprecated)]
                app.activateIgnoringOtherApps(true);
                send(TrayCommand::CheckUpdate);
            }
        }
    );

    fn send(cmd: TrayCommand) {
        let _ = channel().0.try_send(cmd);
    }

    /// 托盘持有的全部对象:释放后图标会从菜单栏消失。
    struct Tray {
        item: Retained<NSStatusItem>,
        target: Retained<TrayTarget>,
        dot: Retained<NSImageView>,
        started: Instant,
    }

    thread_local! {
        static TRAY_STATE: RefCell<Option<Tray>> = const { RefCell::new(None) };
        /// 最近一次的数据:托盘还没创建(或被关掉又打开)时用它恢复。
        static LAST: RefCell<TraySnapshot> = RefCell::new(TraySnapshot::default());
    }

    fn dot_image(state: TrayState) -> Option<Retained<NSImage>> {
        let bytes = match state {
            TrayState::Idle => DOT_IDLE,
            TrayState::Running => DOT_RUNNING,
            TrayState::Waiting => DOT_WAITING,
            TrayState::Unread => DOT_UNREAD,
        };
        let img = crate::app_icon::ns_image(bytes)?;
        img.setSize(NSSize::new(10., 10.));
        Some(img)
    }

    fn menu_item(mtm: MainThreadMarker, title: &str, action: Option<objc2::runtime::Sel>, key: &str, target: Option<&AnyObject>) -> Retained<NSMenuItem> {
        let item = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &NSString::from_str(title), action, &NSString::from_str(key)) };
        unsafe { item.setTarget(target) };
        item
    }

    fn disabled_item(mtm: MainThreadMarker, title: &str) -> Retained<NSMenuItem> {
        let item = menu_item(mtm, title, None, "", None);
        item.setEnabled(false);
        item
    }

    /// 按数据构造菜单。
    fn build_menu(mtm: MainThreadMarker, target: &TrayTarget, snap: &TraySnapshot) -> Retained<NSMenu> {
        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);
        let t: &AnyObject = target;
        let text = summary(snap);
        if !text.is_empty() {
            menu.addItem(&disabled_item(mtm, &text));
            if snap.sessions.iter().any(|s| s.state == TrayState::Unread) {
                menu.addItem(&menu_item(mtm, "全部标为已读", Some(sel!(markAllRead:)), "", Some(t)));
            }
            menu.addItem(&NSMenuItem::separatorItem(mtm));
        }
        menu.addItem(&menu_item(mtm, "打开 LoomCode", Some(sel!(showApp:)), "", Some(t)));
        menu.addItem(&menu_item(mtm, "新对话", Some(sel!(newSession:)), "n", Some(t)));
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        menu.addItem(&disabled_item(mtm, "最近会话"));
        if snap.sessions.is_empty() {
            menu.addItem(&disabled_item(mtm, "暂无会话"));
        }
        for s in &snap.sessions {
            let item = menu_item(mtm, &menu_title(&s.title), Some(sel!(openSession:)), "", Some(t));
            let id = NSString::from_str(&s.id);
            unsafe {
                let obj: &AnyObject = &id;
                item.setRepresentedObject(Some(obj));
            }
            item.setImage(dot_image(s.state).as_deref());
            let tip = if s.project.is_empty() { s.title.clone() } else { format!("{} — {}", s.title, s.project) };
            item.setToolTip(Some(&NSString::from_str(&tip)));
            menu.addItem(&item);
        }
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        menu.addItem(&menu_item(mtm, "设置…", Some(sel!(openSettings:)), ",", Some(t)));
        menu.addItem(&menu_item(mtm, "检查更新…", Some(sel!(checkUpdate:)), "", Some(t)));
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        // 目标为空:沿响应链交给 NSApplication,走正常的退出流程
        menu.addItem(&menu_item(mtm, "退出 LoomCode", Some(sel!(terminate:)), "q", None));
        menu
    }

    /// 在菜单栏放置托盘图标(重复调用无副作用)。
    pub fn install() {
        let Some(mtm) = MainThreadMarker::new() else { return };
        if TRAY_STATE.with_borrow(|t| t.is_some()) {
            return;
        }
        let Some(icon) = crate::app_icon::ns_image(TRAY) else { return };
        icon.setSize(NSSize::new(18., 18.));
        icon.setTemplate(true);
        let target: Retained<TrayTarget> = unsafe { msg_send![TrayTarget::alloc(mtm), init] };
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        let dot = NSImageView::new(mtm);
        if let Some(button) = item.button(mtm) {
            button.setImage(Some(&icon));
            button.setToolTip(Some(&NSString::from_str("LoomCode")));
            let view: &NSView = &dot;
            button.addSubview(view);
        }
        dot.setHidden(true);
        TRAY_STATE.set(Some(Tray { item, target, dot, started: Instant::now() }));
        let snap = LAST.with_borrow(|s| s.clone());
        apply_now(&snap);
    }

    /// 移除菜单栏图标(设置里关闭「菜单栏图标」时)。
    pub fn remove() {
        if let Some(t) = TRAY_STATE.take() {
            NSStatusBar::systemStatusBar().removeStatusItem(&t.item);
        }
    }

    /// 记下数据并(托盘存在时)刷新;内容没变不重建菜单。
    pub fn apply(snap: TraySnapshot) {
        let same = LAST.with_borrow(|last| *last == snap);
        if same {
            return;
        }
        LAST.set(snap.clone());
        apply_now(&snap);
    }

    fn apply_now(snap: &TraySnapshot) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        TRAY_STATE.with_borrow_mut(|tray| {
            let Some(t) = tray.as_mut() else { return };
            t.item.setMenu(Some(&build_menu(mtm, &t.target, snap)));
            let state = indicator(snap);
            let sum = summary(snap);
            if let Some(button) = t.item.button(mtm) {
                button.setToolTip(Some(&NSString::from_str(&if sum.is_empty() { "LoomCode".to_string() } else { format!("LoomCode · {sum}") })));
                // 状态点贴在按钮右下角
                // (按钮的坐标系可能是翻转的:翻转时 y 向下,「底部」要换算)
                let b = button.bounds().size;
                let (w, h) = (b.width.max(24.), b.height.max(22.));
                let view: &NSView = &button;
                let y = if view.isFlipped() { h - 11. } else { 3. };
                t.dot.setFrame(NSRect::new(NSPoint::new(w - 12., y), NSSize::new(8., 8.)));
            }
            match state {
                TrayState::Idle => t.dot.setHidden(true),
                s => {
                    t.dot.setImage(dot_image(s).as_deref());
                    t.dot.setAlphaValue(1.0);
                    t.dot.setHidden(false);
                    t.started = Instant::now();
                }
            }
        });
    }

    /// 推进呼吸动画一帧:只有运行中 / 等待处理才起伏,未读与空闲静止。
    pub fn tick() {
        let animating = LAST.with_borrow(|s| matches!(indicator(s), TrayState::Running | TrayState::Waiting));
        if !animating {
            return;
        }
        TRAY_STATE.with_borrow(|tray| {
            if let Some(t) = tray {
                t.dot.setAlphaValue(breath_alpha(t.started.elapsed().as_secs_f64()));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(id: &str, state: TrayState) -> TraySession {
        TraySession { id: id.into(), title: id.into(), project: "p".into(), state }
    }

    #[::core::prelude::v1::test]
    fn 状态点优先级() {
        assert_eq!(indicator(&TraySnapshot::default()), TrayState::Idle);
        assert_eq!(indicator(&TraySnapshot { sessions: vec![s("a", TrayState::Unread), s("b", TrayState::Running)] }), TrayState::Running);
        assert_eq!(indicator(&TraySnapshot { sessions: vec![s("a", TrayState::Running), s("b", TrayState::Waiting)] }), TrayState::Waiting);
        assert_eq!(indicator(&TraySnapshot { sessions: vec![s("a", TrayState::Idle)] }), TrayState::Idle);
    }

    #[::core::prelude::v1::test]
    fn 摘要文案() {
        assert_eq!(summary(&TraySnapshot::default()), "");
        let snap = TraySnapshot { sessions: vec![s("a", TrayState::Running), s("b", TrayState::Running), s("c", TrayState::Unread)] };
        assert_eq!(summary(&snap), "2 个会话运行中 · 1 个会话未读完成");
    }

    #[::core::prelude::v1::test]
    fn 菜单标题截断() {
        assert_eq!(menu_title("短标题"), "短标题");
        assert_eq!(menu_title("这是一个非常非常非常非常长的会话标题"), "这是一个非常非常非常非常长的会…");
    }

    #[::core::prelude::v1::test]
    fn 呼吸在范围内起伏() {
        for i in 0..40 {
            let a = breath_alpha(i as f64 * 0.05);
            assert!((0.3..=1.0).contains(&a), "透明度 {a} 越界");
        }
        assert!((breath_alpha(0.0) - 1.0).abs() < 1e-9);
        assert!((breath_alpha(0.8) - 0.3).abs() < 1e-9);
    }
}
