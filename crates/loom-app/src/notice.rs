//! 通知中心(对标 VS Code):IDE 模式下,提示从右下角弹出,同时记入历史,状态栏的铃铛可回看;
//! 应用更新等没有窗口上下文的事件也记在这里。Agent 窗口不用通知中心,保持原来的顶部提示。
//!
//! 判断「是不是 IDE 窗口」靠 `NoticeStore.ide_windows`:`IdeWindow` 每次渲染登记自己所在的窗口,
//! 融合模式下 Agent 视图渲染时撤销登记(同一个窗口在两种布局间切换)。
//!
//! @author ixxxxoooo

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::notification::Notification;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::collections::HashSet;
use std::rc::Rc;

/// 历史最多保留多少条。
const MAX_HISTORY: usize = 100;

/// 通知级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Info,
    Success,
    Warning,
    Error,
}

/// 通知上的动作按钮(如「下载更新」)。
#[derive(Clone)]
pub struct NoticeAction {
    pub label: SharedString,
    pub run: Rc<dyn Fn(&mut Window, &mut App)>,
}

/// 一条历史通知。
#[derive(Clone)]
pub struct Notice {
    pub id: u64,
    pub kind: Kind,
    pub message: SharedString,
    /// Unix 秒
    pub at: i64,
    pub read: bool,
    pub action: Option<NoticeAction>,
}

/// 历史与窗口登记。
#[derive(Default)]
pub struct NoticeStore {
    pub items: Vec<Notice>,
    next_id: u64,
    ide_windows: HashSet<WindowId>,
}

impl NoticeStore {
    /// 记一条,返回它的编号。超过上限丢最旧的。
    fn add(&mut self, kind: Kind, message: SharedString, action: Option<NoticeAction>) -> u64 {
        self.next_id += 1;
        self.items.push(Notice { id: self.next_id, kind, message, at: now(), read: false, action });
        if self.items.len() > MAX_HISTORY {
            self.items.remove(0);
        }
        self.next_id
    }

    /// 未读条数。
    pub fn unread(&self) -> usize {
        self.items.iter().filter(|n| !n.read).count()
    }

    pub fn mark_all_read(&mut self) {
        self.items.iter_mut().for_each(|n| n.read = true);
    }

    pub fn remove(&mut self, id: u64) {
        self.items.retain(|n| n.id != id);
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }
}

struct Handle(Entity<NoticeStore>);
impl Global for Handle {}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// 启动时创建全局通知仓库。
pub fn init(cx: &mut App) {
    let store = cx.new(|_| NoticeStore::default());
    cx.set_global(Handle(store));
}

/// 通知仓库实体(界面用它订阅变化、读历史);没初始化时为 `None`。
pub fn store(cx: &App) -> Option<Entity<NoticeStore>> {
    cx.try_global::<Handle>().map(|h| h.0.clone())
}

/// 登记 / 撤销「这个窗口当前显示的是 IDE」。不通知观察者(渲染里调用,不能引起重绘循环)。
pub fn mark_ide(window: &Window, on: bool, cx: &mut App) {
    let Some(store) = store(cx) else { return };
    let id = window.window_handle().window_id();
    store.update(cx, |s, _| {
        if on { s.ide_windows.insert(id); } else { s.ide_windows.remove(&id); }
    });
}

/// 现在有没有窗口正显示 IDE(有的话 `post` 能在其右下角弹出)。
pub fn has_ide_window(cx: &App) -> bool {
    store(cx).is_some_and(|s| !s.read(cx).ide_windows.is_empty())
}

/// 这个窗口当前是不是 IDE 布局。
pub fn is_ide(window: &Window, cx: &App) -> bool {
    store(cx).is_some_and(|s| s.read(cx).ide_windows.contains(&window.window_handle().window_id()))
}

fn build(kind: Kind, message: SharedString) -> Notification {
    match kind {
        Kind::Info => Notification::info(message),
        Kind::Success => Notification::success(message),
        Kind::Warning => Notification::warning(message),
        Kind::Error => Notification::error(message),
    }
}

/// 弹出提示:IDE 窗口里右下角弹出并记入历史;Agent 窗口保持原来的提示。
pub fn show(window: &mut Window, cx: &mut App, kind: Kind, message: impl Into<SharedString>) {
    let message = message.into();
    if is_ide(window, cx) {
        if let Some(s) = store(cx) {
            s.update(cx, |s, cx| {
                s.add(kind, message.clone(), None);
                cx.notify();
            });
        }
        window.push_notification(build(kind, message).placement(Anchor::BottomRight), cx);
    } else {
        window.push_notification(build(kind, message), cx);
    }
}

pub fn info(window: &mut Window, cx: &mut App, message: impl Into<SharedString>) {
    show(window, cx, Kind::Info, message)
}
pub fn success(window: &mut Window, cx: &mut App, message: impl Into<SharedString>) {
    show(window, cx, Kind::Success, message)
}
pub fn warning(window: &mut Window, cx: &mut App, message: impl Into<SharedString>) {
    show(window, cx, Kind::Warning, message)
}
pub fn error(window: &mut Window, cx: &mut App, message: impl Into<SharedString>) {
    show(window, cx, Kind::Error, message)
}

/// 没有窗口上下文时(应用更新等)发一条通知:一定记入历史;若有 IDE 窗口开着,在其右下角弹出。
/// `action` 为可选的动作按钮(文字 + 回调)。Agent 窗口不显示,也不会有铃铛——历史留到切到 IDE 时可看。
pub fn post(cx: &mut App, kind: Kind, message: impl Into<SharedString>, action: Option<NoticeAction>) {
    let message = message.into();
    let Some(s) = store(cx) else { return };
    s.update(cx, |s, cx| {
        s.add(kind, message.clone(), action.clone());
        cx.notify();
    });
    // 推迟到当前更新结束后:调用方可能正在更新工作台,而弹出提示要碰窗口根视图
    cx.defer(move |cx| {
        let Some(s) = store(cx) else { return };
        let ide: HashSet<WindowId> = s.read(cx).ide_windows.clone();
        let target = cx.windows().into_iter().find(|w| ide.contains(&w.window_id()));
        let Some(handle) = target else { return };
        let _ = handle.update(cx, |_, window, cx| {
            let mut n = build(kind, message.clone()).placement(Anchor::BottomRight);
            if let Some(a) = action.clone() {
                n = n.action(move |_, _, _| {
                    let run = a.run.clone();
                    Button::new("notice-action").primary().label(a.label.clone()).on_click(move |_, window, cx| run(window, cx))
                });
            }
            window.push_notification(n, cx);
        });
    });
}

/// 级别对应的图标与颜色。
pub fn kind_style(kind: Kind, cx: &App) -> (gpui_kit::assets::IconName, Hsla) {
    use gpui_kit::assets::IconName;
    use gpui_kit::component::ActiveTheme;
    let t = cx.theme();
    match kind {
        Kind::Info => (IconName::Info, t.info),
        Kind::Success => (IconName::CircleCheck, t.success),
        Kind::Warning => (IconName::TriangleAlert, t.warning),
        Kind::Error => (IconName::CircleX, t.danger),
    }
}

/// 通知面板(状态栏铃铛弹出):标题行(全部清除)+ 倒序的通知列表。
///
/// 参数:`store` 通知仓库;`on_close` 点外部关闭;返回面板元素(调用方负责绝对定位)。
pub fn panel(store: &Entity<NoticeStore>, on_close: impl Fn(&mut Window, &mut App) + 'static, cx: &App) -> Div {
    use crate::ui::icon;
    use gpui_kit::assets::IconName;
    use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
    let t = cx.theme();
    let items: Vec<Notice> = store.read(cx).items.iter().rev().cloned().collect();
    let empty = items.is_empty();
    let (clear_store, hover) = (store.clone(), t.list_hover);
    let header = h_flex().h(px(32.)).px_3().items_center().border_b_1().border_color(t.border)
        .child(div().flex_1().text_size(px(12.)).font_weight(FontWeight::MEDIUM).child("通知"))
        .child(
            div().id("notice-clear").h(px(20.)).px_1p5().flex().items_center().rounded(px(5.)).cursor_pointer()
                .text_size(px(11.)).text_color(t.muted_foreground).hover(move |d| d.bg(hover))
                .on_click(move |_, _, cx| { clear_store.update(cx, |s, cx| { s.clear(); cx.notify(); }); })
                .child("全部清除"),
        );
    let rows = items.into_iter().map(|n| {
        let (ic, color) = kind_style(n.kind, cx);
        let del = store.clone();
        let id = n.id;
        v_flex().w_full().px_3().py_2().gap_1().border_b_1().border_color(t.border.opacity(0.5))
            .child(
                h_flex().gap_2().items_start()
                    .child(div().mt(px(2.)).child(icon(ic, 13., color)))
                    .child(div().flex_1().min_w_0().text_size(px(12.)).text_color(t.foreground).child(n.message.clone()))
                    .child(
                        div().id(SharedString::from(format!("notice-del-{id}"))).flex_none().size(px(18.)).flex().items_center().justify_center().rounded(px(4.)).cursor_pointer()
                            .hover(move |d| d.bg(hover))
                            .on_click(move |_, _, cx| { del.update(cx, |s, cx| { s.remove(id); cx.notify(); }); })
                            .child(icon(IconName::X, 11., t.muted_foreground)),
                    ),
            )
            .child(
                h_flex().pl(px(21.)).gap_2().items_center()
                    .child(div().text_size(px(11.)).text_color(t.muted_foreground).child(loom_core::git_log::relative_time(n.at, now())))
                    .when_some(n.action.clone(), |d, a| {
                        let run = a.run.clone();
                        d.child(
                            div().id(SharedString::from(format!("notice-act-{id}"))).h(px(20.)).px_2().flex().items_center().rounded(px(5.)).cursor_pointer()
                                .text_size(px(11.)).text_color(t.primary_foreground).bg(t.primary)
                                .on_click(move |_, window, cx| run(window, cx))
                                .child(a.label.clone()),
                        )
                    }),
            )
    });
    v_flex().w(px(360.)).max_h(px(420.)).overflow_hidden().rounded(px(10.)).border_1().border_color(t.border).bg(t.popover).shadow_lg()
        .occlude()
        .on_mouse_down_out(move |_, window, cx| on_close(window, cx))
        .child(header)
        .when(empty, |d| d.child(div().h(px(72.)).flex().items_center().justify_center().text_size(px(12.)).text_color(t.muted_foreground).child("暂无通知")))
        .child(v_flex().id("notice-list").flex_1().min_h_0().overflow_y_scroll().children(rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 未读计数与已读清除() {
        let mut s = NoticeStore::default();
        let a = s.add(Kind::Info, "a".into(), None);
        s.add(Kind::Error, "b".into(), None);
        assert_eq!(s.unread(), 2);
        s.mark_all_read();
        assert_eq!(s.unread(), 0);
        s.remove(a);
        assert_eq!(s.items.len(), 1);
        s.clear();
        assert!(s.items.is_empty());
    }

    #[::core::prelude::v1::test]
    fn 历史超过上限丢最旧() {
        let mut s = NoticeStore::default();
        for i in 0..MAX_HISTORY + 5 {
            s.add(Kind::Info, format!("{i}").into(), None);
        }
        assert_eq!(s.items.len(), MAX_HISTORY);
        assert_eq!(s.items[0].message.as_ref(), "5");
    }
}
