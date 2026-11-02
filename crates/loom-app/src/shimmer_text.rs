//! 流光文字:按外观偏好在「经典」(单色高亮带扫过)与「流光溢彩」(七彩渐变流转)之间切换。
//!
//! @author ixxxxoooo

use crate::UiPrefs;
use gpui_kit::component::shimmer::ShimmerText;
use gpui_kit::component::{ActiveTheme as _, Colorize as _};
use gpui_kit::*;
use loom_config::ShimmerKind;
use std::time::Duration;

/// 流光溢彩一个循环的时长(毫秒)。
const AURORA_CYCLE_MS: u64 = 5000;
/// 刷新帧率上限(逐字重排文字,限频以免空闲时频繁重建)。
const AURORA_FPS: f32 = 20.;
/// 一行文字在调色板上跨越的圈数:越小整行越接近同一种颜色。
const AURORA_TURNS_PER_LINE: f32 = 0.45;
/// 与文字原色的混合比例:越小越含蓄。
const AURORA_MIX: f32 = 0.55;
/// 调色板色相(°):只在蓝 → 靛 → 淡紫之间往返,首尾相接可无缝循环。
const AURORA_HUES: [f32; 6] = [208., 226., 248., 270., 248., 226.];

/// 调色板上的一点:`pos` 取 0..1 循环,色标间余弦平滑插值,返回低饱和的冷色。
///
/// 参数:`pos` 位置(自动取小数部分);`dark` 是否深色主题。
/// 返回:对应颜色。
pub fn aurora_color(pos: f32, dark: bool) -> Hsla {
    let n = AURORA_HUES.len();
    let x = pos.rem_euclid(1.) * n as f32;
    let (i, f) = (x as usize % n, x.fract());
    let k = (1. - (f * std::f32::consts::PI).cos()) / 2.;
    let (h0, h1) = (AURORA_HUES[i], AURORA_HUES[(i + 1) % n]);
    hsla((h0 + (h1 - h0) * k) / 360., if dark { 0.5 } else { 0.45 }, if dark { 0.74 } else { 0.46 }, 1.)
}

/// 给每个字符分配流光溢彩的高亮区间:调色板颜色与文字原色混合后逐字上色。
///
/// 参数:`text` 文本;`phase` 动画进度 0..1;`base` 文字原色;`dark` 是否深色主题。
/// 返回:`(字节区间, 高亮样式)` 列表,每个字符一项。
pub fn aurora_highlights(text: &str, phase: f32, base: Hsla, dark: bool) -> Vec<(std::ops::Range<usize>, HighlightStyle)> {
    let n = text.chars().count().max(1) as f32;
    text.char_indices().enumerate().map(|(i, (at, ch))| {
        let pos = phase - AURORA_TURNS_PER_LINE * (i as f32 / n);
        let color = base.mix_oklab(aurora_color(pos, dark), AURORA_MIX);
        (at..at + ch.len_utf8(), HighlightStyle { color: Some(color), ..Default::default() })
    }).collect()
}

/// 流光溢彩文字:读取继承来的文字颜色作为底色,逐字向调色板颜色靠拢。
#[derive(IntoElement)]
struct AuroraText {
    text: SharedString,
    id: SharedString,
}

impl RenderOnce for AuroraText {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let base = window.text_style().color;
        let dark = cx.theme().is_dark();
        let text = self.text;
        div().min_w_0().with_animation(
            self.id,
            Animation::new(Duration::from_millis(AURORA_CYCLE_MS)).repeat_synced().with_max_fps(AURORA_FPS),
            move |d, t| {
                d.child(StyledText::new(text.clone()).with_highlights(aurora_highlights(&text, t, base, dark)))
            },
        )
    }
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
        ShimmerKind::Aurora if cx.reduce_motion() => div().child(text).into_any_element(),
        ShimmerKind::Aurora => AuroraText { text, id }.into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: Hsla = Hsla { h: 0., s: 0., l: 0.5, a: 1. };

    #[::core::prelude::v1::test]
    fn 每个字符一个区间且覆盖全文() {
        let text = "思考中… ok";
        let hl = aurora_highlights(text, 0.3, BASE, false);
        assert_eq!(hl.len(), text.chars().count());
        assert_eq!(hl.last().unwrap().0.end, text.len());
        assert!(hl.windows(2).all(|w| w[0].0.end == w[1].0.start));
    }

    #[::core::prelude::v1::test]
    fn 颜色随进度流转且色相限制在冷色段() {
        let a = aurora_highlights("abcd", 0.0, BASE, true);
        let b = aurora_highlights("abcd", 0.5, BASE, true);
        assert_ne!(a[0].1.color, b[0].1.color);
        assert!((0..=100).all(|i| { let h = aurora_color(i as f32 / 100., false).h * 360.; (205.0..=272.0).contains(&h) }));
    }

    #[::core::prelude::v1::test]
    fn 空串不panic() {
        assert!(aurora_highlights("", 0.5, BASE, false).is_empty());
    }
}
