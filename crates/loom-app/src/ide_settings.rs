//! 独立 IDE 窗口里的设置页:设置属于工作台(`settings::render` 读写 `Workbench`),
//! 这个视图在 IDE 窗口里把它整窗渲染出来。「在哪个窗口打开设置就在哪个窗口」,Agent 窗口不受影响。
//!
//! @author ixxxxoooo

use crate::workbench::Workbench;
use gpui_kit::*;

/// 设置页宿主。
pub struct SettingsHost {
    wb: Entity<Workbench>,
    _sub: Subscription,
}

impl SettingsHost {
    pub fn new(wb: Entity<Workbench>, cx: &mut Context<Self>) -> Self {
        // 设置页读的是工作台的状态:工作台变了就重绘
        let sub = cx.observe(&wb, |_, _, cx| cx.notify());
        Self { wb, _sub: sub }
    }
}

impl Render for SettingsHost {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let wb = self.wb.clone();
        wb.update(cx, |wb, wcx| crate::settings::render(wb, window, wcx))
    }
}
