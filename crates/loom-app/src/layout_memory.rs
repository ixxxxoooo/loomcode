//! 窗口布局的记忆:左右栏开关、所在页面、IDE 是否开着及其内部布局、侧栏项目展开,
//! 写进 `state.json` 的 `last.layout`,下次启动恢复(仅真实模式恢复,与会话恢复一致)。
//!
//! 做法:不在每个开关处埋点,而是在渲染里对比「当前布局快照」与「已记下的」,变了才写盘,
//! 所以以后新增的开关只要进快照就自动被记住。
//!
//! @author ixxxxoooo

use crate::ide_window::IdeWindow;
use crate::workbench::{Page, RightTab, Workbench};
use gpui_kit::*;
use loom_config::{IdeLayout, LayoutMemory};

impl Page {
    /// 持久化用的键。
    pub fn key(self) -> &'static str {
        match self {
            Page::Chat => "chat",
            Page::Settings => "settings",
            Page::Canvas => "canvas",
            Page::Automation => "automation",
            Page::Browser => "browser",
            Page::Customize => "customize",
        }
    }

    /// 由键还原;未知键返回 `None`。
    pub fn from_key(key: &str) -> Option<Page> {
        [Page::Chat, Page::Settings, Page::Canvas, Page::Automation, Page::Browser, Page::Customize].into_iter().find(|p| p.key() == key)
    }
}

impl Workbench {
    /// 当前布局快照;`prev` 里没法现取的部分(IDE 内部布局、尚未加载完的项目展开)沿用它。
    fn layout_snapshot(&self, prev: &LayoutMemory, cx: &App) -> LayoutMemory {
        // 窗口变窄被自动收起时,记收起前的状态,免得下次在宽窗口里启动还是收起的
        let left = if self.layout_narrow { self.left_before_narrow } else { self.left_open };
        let right = if self.right_narrow { self.right_before_narrow } else { self.right_open };
        let ide = self.ide || self.ide_window.is_some_and(|h| cx.windows().contains(&h));
        // 项目列表还没从历史里读完时不能记:这时一个都没有,会把真正的展开状态抹掉
        let expanded = if self.layout_ready {
            Some(self.projects.iter().filter(|p| p.expanded).map(|p| p.path.clone()).collect())
        } else {
            prev.expanded_projects.clone()
        };
        LayoutMemory {
            left_open: Some(left),
            right_open: Some(right),
            page: Some(self.page.key().to_string()),
            ide,
            expanded_projects: expanded,
            show_archived: self.show_archived,
            ide_layout: prev.ide_layout.clone(),
        }
    }

    /// 渲染时调用:布局变了就记下并写盘(没变什么也不做)。
    pub fn sync_layout(&mut self, cx: &mut Context<Self>) {
        if crate::secondary::is() {
            return;
        }
        let snap = self.layout_snapshot(&self.ui_state.last.layout, cx);
        if snap != self.ui_state.last.layout {
            self.ui_state.last.layout = snap;
            self.persist_ui_state();
        }
    }

    /// IDE 窗口上报它的内部布局(由 `IdeWindow` 在渲染之后推迟调用)。
    pub fn set_ide_layout(&mut self, layout: IdeLayout) {
        if self.ui_state.last.layout.ide_layout != layout {
            self.ui_state.last.layout.ide_layout = layout;
            self.persist_ui_state();
        }
    }

    /// 启动时恢复布局(只在 `new` 里、真实模式调用)。
    /// IDE 是否开着、项目展开要等历史加载完才能做,先挂起。
    pub fn restore_layout(&mut self) {
        let l = self.ui_state.last.layout.clone();
        if let Some(v) = l.left_open {
            self.left_open = v;
            self.left_before_narrow = v;
        }
        if let Some(v) = l.right_open {
            self.right_open = v;
            self.right_before_narrow = v;
        }
        self.show_archived = l.show_archived;
        // 设置页是临时去处,不当作「上次所在页面」恢复;其余页面原样回去
        if let Some(page) = l.page.as_deref().and_then(Page::from_key).filter(|p| *p != Page::Settings) {
            self.page = page;
        }
        self.pending_expanded = l.expanded_projects;
        self.pending_ide = l.ide;
    }
}

impl IdeWindow {
    /// 当前 IDE 内部布局快照(窗口窄时被自动收起的,记收起前的状态)。
    pub(crate) fn layout_snapshot(&self, cx: &App) -> IdeLayout {
        IdeLayout {
            left_open: Some(if self.left_narrow { self.left_before } else { self.left_open }),
            chat_open: Some(if self.chat_narrow { self.chat_before } else { self.chat_open }),
            search_open: self.search_open,
            term_open: self.term_open,
            chat_wide: self.chat_wide,
            left_tab: Some(self.panel.read(cx).tab.key().to_string()),
            left_w: Some(self.left_w.round()),
            chat_w: Some(self.chat_w.round()),
            term_h: Some(self.term_h.round()),
        }
    }

    /// 按记忆恢复内部布局(创建之后立刻调用)。环境变量调试开关优先。
    pub(crate) fn restore_layout(&mut self, l: &IdeLayout, cx: &mut Context<Self>) {
        if let Some(v) = l.left_open {
            self.left_open = v;
            self.left_before = v;
        }
        if let Some(v) = l.chat_open {
            self.chat_open = v;
            self.chat_before = v;
        }
        self.search_open = l.search_open;
        // 栏宽 / 终端高度:夹到各自的允许范围内,坏值退回默认
        let pick = |v: Option<f32>, lo: f32, hi: f32, d: f32| v.filter(|x| x.is_finite()).map_or(d, |x| x.clamp(lo, hi));
        self.left_w = pick(l.left_w, 160., 520., 260.);
        self.chat_w = pick(l.chat_w, crate::layout_rules::COMPOSER_MIN_W, 900., 420.);
        self.term_h = pick(l.term_h, 120., 600., 260.);
        if std::env::var_os("LOOM_IDE_TERM").is_none() {
            self.term_open = l.term_open;
        }
        if std::env::var_os("LOOM_CHAT_WIDE").is_none() {
            self.chat_wide = l.chat_wide;
        }
        // 左栏只有「文件」与「改动」两种分段
        if let Some(tab) = l.left_tab.as_deref().and_then(RightTab::from_key).filter(|t| matches!(t, RightTab::Files | RightTab::Changes)) {
            self.panel.update(cx, |p, cx| p.set_tab(tab, cx));
        }
    }

    /// 横向分栏拖动回调:记下新的左栏 / 对话栏宽度(不可见的栏宽度不可信,忽略),停下后再触发保存。
    pub(crate) fn on_split_resized(&mut self, sizes: &[Pixels], cx: &mut Context<Self>) {
        let [left, _, chat] = sizes else { return };
        let (left, chat) = (f32::from(*left), f32::from(*chat));
        if self.left_open && left > 1. {
            self.left_w = left.clamp(160., 520.);
        }
        if self.chat_open && chat > 1. {
            self.chat_w = chat.clamp(crate::layout_rules::COMPOSER_MIN_W, 900.);
        }
        self.schedule_size_sync(cx);
    }

    /// 上下分栏拖动回调:记下终端面板高度。
    pub(crate) fn on_vsplit_resized(&mut self, sizes: &[Pixels], cx: &mut Context<Self>) {
        let [_, term] = sizes else { return };
        let term = f32::from(*term);
        if self.term_open && term > 1. {
            self.term_h = term.clamp(120., 600.);
        }
        self.schedule_size_sync(cx);
    }

    /// 拖动停下 0.4 秒后重绘一次,渲染里的 `sync_layout` 会发现宽高变了并落盘。
    fn schedule_size_sync(&mut self, cx: &mut Context<Self>) {
        if self.size_save_pending {
            return;
        }
        self.size_save_pending = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(400)).await;
            let _ = this.update(cx, |this, cx| {
                this.size_save_pending = false;
                cx.notify();
            });
        })
        .detach();
    }

    /// 渲染里调用:内部布局变了就推迟到渲染之后告诉工作台去记(渲染期间不能更新工作台)。
    pub(crate) fn sync_layout(&self, cx: &mut Context<Self>) {
        let snap = self.layout_snapshot(cx);
        if self.wb.read(cx).ui_state.last.layout.ide_layout != snap {
            let wb = self.wb.clone();
            cx.defer(move |cx| wb.update(cx, |wb, _| wb.set_ide_layout(snap)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 页面键往返() {
        for p in [Page::Chat, Page::Settings, Page::Canvas, Page::Automation, Page::Browser, Page::Customize] {
            assert_eq!(Page::from_key(p.key()), Some(p));
        }
        assert_eq!(Page::from_key("nope"), None);
    }
}
