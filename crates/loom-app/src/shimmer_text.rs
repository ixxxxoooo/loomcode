//! 流光文字:按外观偏好在「经典」(单色高亮带扫过)与「流光溢彩」(七彩渐变流转)之间切换。
//!
//! @author ixxxxoooo

use crate::UiPrefs;
use gpui_kit::component::shimmer::ShimmerText;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use loom_config::ShimmerKind;
use std::time::Duration;

/// 流光溢彩一个循环的时长(毫秒)。
const AURORA_CYCLE_MS: u64 = 5000;
/// 流光溢彩的刷新帧率上限(逐字重排文字,限频以免空闲时频繁重建)。
const AURORA_FPS: f32 = 20.;
/// 一行文字在调色板上跨越的圈数:越小整行越接近同一种颜色,过渡越柔和。
const AURORA_TURNS_PER_LINE: f32 = 0.45;
/// 极光调色板(色相 °):青 → 蓝 → 紫 → 粉 → 紫 → 蓝,首尾相接可无缝循环,避开黄绿红这类刺眼色。
const AURORA_HUES: [f32; 6] = [178., 212., 262., 318., 262., 212.];

/// 调色板上的一点:`pos` 取 0..1 循环,在相邻色标间做余弦平滑插值,返回低饱和的柔和色。
///
/// 参数:`pos` 位置(自动取小数部分);`dark` 是否深色主题。
/// 返回:对应颜色。
pub fn aurora_color(pos: f32, dark: bool) -> Hsla {
    let n = AURORA_HUES.len();
    let x = pos.rem_euclid(1.) * n as f32;
    let (i, f) = (x as usize % n, x.fract());
    let k = (1. - (f * std::f32::consts::PI).cos()) / 2.;
    let (h0, h1) = (AURORA_HUES[i], AURORA_HUES[(i + 1) % n]);
    let hue = (h0 + (h1 - h0) * k) / 360.;
    hsla(hue, if dark { 0.62 } else { 0.55 }, if dark { 0.74 } else { 0.52 }, 1.)
}

/// 给每个字符分配流光溢彩的高亮区间。
///
/// 参数:`text` 文本;`phase` 动画进度 0..1;`dark` 是否深色主题。
/// 返回:`(字节区间, 高亮样式)` 列表,每个字符一项。
pub fn aurora_highlights(text: &str, phase: f32, dark: bool) -> Vec<(std::ops::Range<usize>, HighlightStyle)> {
    let n = text.chars().count().max(1) as f32;
    text.char_indices().enumerate().map(|(i, (at, ch))| {
        let hue = phase - AURORA_TURNS_PER_LINE * (i as f32 / n);
        let style = HighlightStyle { color: Some(aurora_color(hue, dark)), ..Default::default() };
        (at..at + ch.len_utf8(), style)
    }).collect()
}

/// 带流光的文字,样式取自外观偏好。
///
/// 参数:`text` 文字;`id` 动画元素 id(同屏唯一);`cx` 应用上下文。
/// 返回:可直接放进布局的元素(字号、颜色由外层继承)。
pub fn shimmer_label(text: impl Into<SharedString>, id: impl Into<SharedString>, cx: &App) -> AnyElement {
    let (text, id): (SharedString, SharedString) = (text.into(), id.into());
    let kind = cx.try_global::<UiPrefs>().map_or(ShimmerKind::Classic, |p| p.shimmer);
    match kind {
        ShimmerKind::Classic => ShimmerText::new(text).id(id).into_any_element(),
        ShimmerKind::Aurora => {
            if cx.reduce_motion() {
                return div().child(text).into_any_element();
            }
            let dark = cx.theme().is_dark();
            div().min_w_0().with_animation(
                id,
                Animation::new(Duration::from_millis(AURORA_CYCLE_MS)).repeat_synced().with_max_fps(AURORA_FPS),
                move |d, t| d.child(StyledText::new(text.clone()).with_highlights(aurora_highlights(&text, t, dark))),
            ).into_any_element()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 每个字符一个区间且覆盖全文() {
        let text = "思考中… ok";
        let hl = aurora_highlights(text, 0.3, false);
        assert_eq!(hl.len(), text.chars().count());
        assert_eq!(hl.first().unwrap().0.start, 0);
        assert_eq!(hl.last().unwrap().0.end, text.len());
        assert!(hl.windows(2).all(|w| w[0].0.end == w[1].0.start));
    }

    #[::core::prelude::v1::test]
    fn 相邻字符颜色不同且随进度流转() {
        let a = aurora_highlights("abcd", 0.0, true);
        assert_ne!(a[0].1.color, a[1].1.color);
        let b = aurora_highlights("abcd", 0.5, true);
        assert_ne!(a[0].1.color, b[0].1.color);
    }

    #[::core::prelude::v1::test]
    fn 空串不panic() {
        assert!(aurora_highlights("", 0.5, false).is_empty());
    }
}
