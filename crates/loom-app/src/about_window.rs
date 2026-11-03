//! 「关于 LoomCode」小窗(对标 Cursor / 系统「关于」面板):图标、名称、版本与构建时间、
//! 「检查更新…」(随更新状态变成「下载更新 / 重启更新」)、「复制版本信息」与版权行。
//! 入口是应用菜单「关于 LoomCode」;已打开时只聚焦。
//!
//! @author ixxxxoooo

use crate::updater::UpdateState;
use crate::workbench::Workbench;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::TitleBar;
use gpui_kit::component::{ActiveTheme, Disableable as _, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::sync::Arc;

/// 窗口尺寸(逻辑像素)。
const SIZE: (f32, f32) = (400., 380.);

/// 当前已打开的「关于」窗口。
#[derive(Default)]
struct AboutHandle(Option<AnyWindowHandle>);
impl Global for AboutHandle {}

/// 是不是「关于」窗口(⌘W 关窗口时用:这个窗口要真正关闭,而不是隐藏整个应用)。
pub fn is_about(cx: &App, id: WindowId) -> bool {
    cx.try_global::<AboutHandle>().and_then(|h| h.0).is_some_and(|h| h.window_id() == id)
}

/// 版本号字符串,如 `0.1.3 (构建 128)`;开发版构建号为 0 时省略。
pub fn version_line() -> String {
    let build = crate::updater::current_build();
    if build > 0 { format!("{} (构建 {build})", env!("CARGO_PKG_VERSION")) } else { env!("CARGO_PKG_VERSION").to_string() }
}

/// 构建时间的显示:`2026年10月27日 08:33 (1 周前)`;取不到时返回空串。
pub fn build_time_line(now: i64) -> String {
    let Some(t) = option_env!("LOOM_BUILD_TIME").and_then(|v| v.parse::<i64>().ok()).filter(|t| *t > 0) else { return String::new() };
    let stamp = loom_core::git_log::format_datetime(t, loom_diag::local_tz_offset());
    format!("{} ({})", cn_date(&stamp), week_relative(t, now))
}

/// `2026-10-27 08:33` → `2026年10月27日 08:33`。
fn cn_date(stamp: &str) -> String {
    let Some((d, t)) = stamp.split_once(' ') else { return stamp.to_string() };
    let mut it = d.split('-');
    match (it.next(), it.next().and_then(|m| m.parse::<u32>().ok()), it.next().and_then(|m| m.parse::<u32>().ok())) {
        (Some(y), Some(m), Some(day)) => format!("{y}年{m}月{day}日 {t}"),
        _ => stamp.to_string(),
    }
}

/// 相对时间:一周内沿用分钟 / 小时 / 天,更久换成「N 周前 / N 个月前 / N 年前」。
fn week_relative(t: i64, now: i64) -> String {
    let d = (now - t).max(0);
    match d {
        0..=604_799 => loom_core::git_log::relative_time(t, now),
        604_800..=2_591_999 => format!("{} 周前", d / 604_800),
        _ => loom_core::git_log::relative_time(t, now),
    }
}

/// 「复制版本信息」的内容。
pub fn version_info(wb: &Workbench) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let mut out = format!("LoomCode: {}\n", version_line());
    let hash = option_env!("LOOM_GIT_HASH").unwrap_or("");
    if !hash.is_empty() {
        out.push_str(&format!("提交: {hash}\n"));
    }
    let time = build_time_line(now);
    if !time.is_empty() {
        out.push_str(&format!("构建时间: {time}\n"));
    }
    out.push_str(&format!("运行模式: {}\n", if wb.demo { "演示" } else { "真实" }));
    if let Some(c) = &wb.claude_info {
        out.push_str(&format!("Claude Code: {c}\n"));
    }
    out.push_str(&format!("系统: {} {}\n", std::env::consts::OS, std::env::consts::ARCH));
    out
}

/// 打开(或聚焦)「关于」窗口。
pub fn open(cx: &mut App) {
    if let Some(h) = cx.try_global::<AboutHandle>().and_then(|h| h.0)
        && h.update(cx, |_, w, _| w.activate_window()).is_ok()
    {
        return;
    }
    let Some(wb) = cx.try_global::<crate::WorkbenchRef>().and_then(|r| r.0.upgrade()) else { return };
    let mut options = TitleBar::window_options();
    options.window_bounds = Some(WindowBounds::Windowed(Bounds::centered(None, size(px(SIZE.0), px(SIZE.1)), cx)));
    options.is_resizable = false;
    options.is_minimizable = false;
    match gpui_kit::open_window(options, cx, move |_, cx| cx.new(|cx| AboutView::new(wb, cx))) {
        Ok((handle, _)) => {
            cx.set_global(AboutHandle(Some(handle)));
            cx.activate(true);
        }
        Err(e) => loom_diag::error!("打开关于窗口失败:{e}"),
    }
}

struct AboutView {
    wb: Entity<Workbench>,
    icon: [Arc<Image>; 2],
    copied: bool,
    _sub: Subscription,
}

impl AboutView {
    fn new(wb: Entity<Workbench>, cx: &mut Context<Self>) -> Self {
        // 更新状态变化时刷新按钮
        let sub = cx.observe(&wb, |_, _, cx| cx.notify());
        let png = |dark| Arc::new(Image::from_bytes(ImageFormat::Png, crate::app_icon::icon_png(dark).to_vec()));
        Self { wb, icon: [png(false), png(true)], copied: false, _sub: sub }
    }

    /// 复制版本信息,按钮上「已复制」停留一会儿。
    fn copy_info(&mut self, cx: &mut Context<Self>) {
        let text = version_info(self.wb.read(cx));
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.copied = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(1500)).await;
            let _ = this.update(cx, |this, cx| { this.copied = false; cx.notify(); });
        })
        .detach();
    }
}

impl Render for AboutView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
        let (state, status) = { let w = self.wb.read(cx); (w.update.clone(), w.update_text()) };
        let wb = self.wb.clone();
        let busy = matches!(state, UpdateState::Checking | UpdateState::Downloading(_));
        let highlight = matches!(state, UpdateState::Available(_) | UpdateState::Ready(_));
        let label = match &state {
            UpdateState::Checking => "检查中…".to_string(),
            UpdateState::Available(i) => format!("下载更新 v{}", i.version),
            UpdateState::Downloading(_) => "下载中…".to_string(),
            UpdateState::Ready(_) => "重启更新".to_string(),
            _ => "检查更新…".to_string(),
        };
        let mut update_btn = Button::new("about-update").label(label).disabled(busy).on_click(move |_, _, cx| {
            wb.update(cx, |w, cx| match w.update {
                UpdateState::Available(_) => w.download_update(cx),
                UpdateState::Ready(_) => w.install_update(cx),
                _ => w.check_update(cx),
            });
        });
        update_btn = if highlight { update_btn.primary() } else { update_btn.outline() };
        let copy_btn = Button::new("about-copy").label(if self.copied { "已复制" } else { "复制版本信息" });
        let copy_btn = if highlight { copy_btn.outline() } else { copy_btn.primary() }
            .on_click(cx.listener(|this, _, _, cx| this.copy_info(cx)));
        let time = build_time_line(now);
        // 红绿灯下方的空白可拖动窗口
        let drag = crate::window_header::draggable(div().id("about-drag").w_full().h(px(36.)).flex_none());
        v_flex().size_full().bg(t.background).text_color(t.foreground).items_center()
            .child(drag)
            .child(img(self.icon[t.is_dark() as usize].clone()).size(px(112.)).flex_none())
            .child(div().mt_3().text_size(px(26.)).font_weight(FontWeight::BOLD).child("LoomCode"))
            .child(div().mt_2().text_size(px(15.)).text_color(t.muted_foreground).child(format!("版本 {}", version_line())))
            .when(!time.is_empty(), |d| d.child(div().mt_1().text_size(px(12.)).text_color(t.muted_foreground.opacity(0.85)).child(time)))
            .child(div().mt_4().flex().gap_2().child(update_btn).child(copy_btn))
            .child(div().mt_2().h(px(16.)).text_size(px(12.)).text_color(if matches!(state, UpdateState::Failed(_)) { t.danger } else { t.muted_foreground }).max_w(px(340.)).truncate()
                .child(if matches!(state, UpdateState::Idle) { String::new() } else { status }))
            .child(div().flex_1())
            .child(div().pb_4().text_size(px(11.)).text_color(t.muted_foreground).child("Copyright © 2026 ixxxxoooo. All rights reserved."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 日期改成中文格式() {
        assert_eq!(cn_date("2026-10-27 08:33"), "2026年10月27日 08:33");
        assert_eq!(cn_date("乱七八糟"), "乱七八糟");
    }

    #[::core::prelude::v1::test]
    fn 相对时间超过一周按周算() {
        assert_eq!(week_relative(0, 3 * 86_400), "3 天前");
        assert_eq!(week_relative(0, 8 * 86_400), "1 周前");
        assert_eq!(week_relative(0, 21 * 86_400), "3 周前");
        assert_eq!(week_relative(0, 40 * 86_400), "1 个月前");
    }
}
