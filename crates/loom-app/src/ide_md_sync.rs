//! Markdown 分栏(左源码 / 右预览)时的同步滚动:按「滚动比例」对齐两边,
//! 谁被用户滚动了(偏移变了而另一边没变)就让另一边跟到同样的比例;程序设置的偏移会记下来,不会反过来再触发对方。
//!
//! @author ixxxxoooo

use crate::ide_markdown::MdDocView;
use gpui_kit::component::RopeExt as _;
use gpui_kit::*;

/// 偏移变化小于这个像素数视为没动(忽略取整误差)。
const EPS: f32 = 1.;

/// 滚动比例(0..=1):偏移 / 最大偏移。
///
/// 参数:`y` 当前向下滚过的距离(≥0);`max` 最大可滚距离。返回:比例;没有可滚空间时为 0。
pub fn ratio(y: f32, max: f32) -> f32 {
    if max <= 1. { 0. } else { (y / max).clamp(0., 1.) }
}

impl MdDocView {
    /// 源码编辑器的 (已滚距离, 最大可滚距离):最大值按「行数 × 行高 − 视口高」估算(自动换行时是近似值)。
    fn source_scroll(&self, cx: &App) -> Option<(f32, f32)> {
        let ed = self.editor.read(cx);
        let lh = f32::from(ed.line_height()?);
        let view_h = f32::from(ed.text_bounds()?.size.height);
        let lines = ed.text().lines_len() as f32;
        Some((-f32::from(ed.scroll_offset().y), (lines * lh - view_h).max(0.)))
    }

    /// 在分栏模式下同步两边的滚动(渲染时调用)。
    pub(crate) fn sync_scroll(&mut self, cx: &mut Context<Self>) {
        let Some((ys, max_s)) = self.source_scroll(cx) else { return };
        let yp = -f32::from(self.scroll.offset().y);
        let max_p = f32::from(self.scroll.max_offset().y);
        let (ds, dp) = ((ys - self.sync_last.0).abs() > EPS, (yp - self.sync_last.1).abs() > EPS);
        if ds && !dp {
            let target = ratio(ys, max_s) * max_p;
            self.scroll.set_offset(point(self.scroll.offset().x, px(-target)));
            self.sync_last = (ys, target);
        } else if dp && !ds {
            let target = ratio(yp, max_p) * max_s;
            let x = self.editor.read(cx).scroll_offset().x;
            self.editor.update(cx, |e, cx| e.set_scroll_offset(point(x, px(-target)), cx));
            self.sync_last = (target, yp);
        } else {
            self.sync_last = (ys, yp);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 比例在范围内且无滚动空间为零() {
        assert_eq!(ratio(50., 100.), 0.5);
        assert_eq!(ratio(150., 100.), 1.);
        assert_eq!(ratio(-5., 100.), 0.);
        assert_eq!(ratio(10., 0.), 0.);
    }
}
