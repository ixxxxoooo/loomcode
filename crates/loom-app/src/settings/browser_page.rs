//! 浏览器页:内置阅读模式浏览器的抓取超时与 User-Agent。
//!
//! @author ixxxxoooo

use super::widgets::{group, note, page_header, row, rows, stepper};
use crate::workbench::Workbench;
use gpui_kit::component::input::Input;
use gpui_kit::component::{Sizable, v_flex};
use gpui_kit::*;

/// 渲染浏览器页。
pub fn render(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let p = wb.settings.browser.clone();
    v_flex().w_full()
        .child(page_header("浏览器", "阅读模式浏览器抓取网页时使用的参数;它不执行脚本,只提取正文", div(), cx))
        .child(group("抓取", rows![
            row("请求超时", "单个页面的最长等待时间", stepper("br-timeout", p.timeout_secs as i64, (5, 120, 5), " 秒", cx,
                |this, v, cx| this.update_settings(cx, |s| s.browser.timeout_secs = v as u32)), cx),
            row("User-Agent", "留空使用内置标识;个别站点会按它返回不同内容", div().w(px(300.)).child(Input::new(wb.field("browser_ua")).small()), cx),
        ], cx))
        .child(note("改动对之后的每次导航生效", cx))
        .into_any_element()
}
