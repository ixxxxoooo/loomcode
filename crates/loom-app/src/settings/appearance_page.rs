//! 外观页的扩展区块:会话与设置页内容宽度、时间线字号、滚动条、等待动画。
//!
//! @author ixxxxoooo

use super::widgets::{group, note, row, rows, segmented, stepper};
use crate::appearance::SPINNERS;
use crate::workbench::Workbench;
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_config::{ScrollbarStyle, ShimmerKind, ToolDensity};

/// 外观页在主题 / 强调色之后的全部区块。
pub fn sections(wb: &Workbench, cx: &mut Context<Workbench>) -> Vec<AnyElement> {
    let a = wb.settings.appearance.clone();
    let t = cx.theme().clone();
    let scale = f32::from(a.timeline_font_pct) / 100.;
    let spinners = SPINNERS.iter().enumerate().map(|(i, def)| {
        let (style, on) = (def.style, a.spinner == def.style);
        h_flex().id(SharedString::from(format!("spin-{i}"))).w_full().px_3().py_2().gap_3().items_center().cursor_pointer()
            .when(on, |d| d.bg(t.primary.opacity(0.1))).hover(|d| d.bg(t.muted))
            .on_click(cx.listener(move |this, _, _, cx| this.update_settings(cx, |s| s.appearance.spinner = style)))
            .child(div().w(px(20.)).flex().justify_center().text_size(px(15.)).text_color(t.primary).child(crate::appearance::glyph_anim(&format!("spin-demo-{i}"), style, t.primary, 15.)))
            .child(v_flex().flex_1().gap_0p5()
                .child(div().text_size(px(13.)).child(def.name))
                .child(div().text_size(px(12.)).text_color(t.muted_foreground).child(crate::appearance::sample(def))))
            .when(on, |d| d.child(crate::ui::icon(IconName::Check, 14., t.primary)))
            .into_any_element()
    }).collect::<Vec<_>>();
    vec![
        group("Agent 会话", rows![
            row("内容宽度", "会话时间线与输入框的最大宽度;窄屏或分屏时会自动收窄", segmented("w-chat",
                vec![(680u16, "紧凑"), (820, "标准"), (920, "较宽"), (1080, "宽"), (1600, "铺满")], a.chat_width, cx,
                |this, v, cx| this.update_settings(cx, |s| s.appearance.chat_width = v)), cx),
            // 设置行与示例预览放在同一个条目里,中间不画分隔线
            v_flex().w_full()
                .child(row("工具调用详细程度", a.tool_density.describe(), segmented("tool-density",
                    ToolDensity::ALL.iter().map(|d| (*d, d.label())).collect(), a.tool_density, cx,
                    |this, v, cx| this.update_settings(cx, |s| s.appearance.tool_density = v)), cx))
                .child(super::density_preview::density_preview(a.tool_density, cx)),
        ], cx).into_any_element(),
        group("设置页宽度", rows![
            row("内容宽度", "统一控制所有设置页右侧内容的最大宽度;窗口较窄时自动收窄", segmented("w-settings",
                vec![(640u16, "紧凑"), (760, "标准"), (900, "宽"), (1600, "铺满")], a.settings_width, cx,
                |this, v, cx| this.update_settings(cx, |s| s.appearance.settings_width = v)), cx),
        ], cx).into_any_element(),
        group("字号", rows![
            row("时间线字号", "会话里 Claude 回复正文的字号(85%–150%)", stepper("fs-tl", i64::from(a.timeline_font_pct), (85, 150, 5), "%", cx,
                |this, v, cx| this.update_settings(cx, |s| s.appearance.timeline_font_pct = v as u16)), cx),
            div().px_3().py_3().text_size(px(14. * scale)).child("预览:Claude 的回复会按这个大小显示。let x = 1"),
        ], cx).into_any_element(),
        group("滚动条", rows![
            row("显示方式", "「跟随系统」沿用 macOS「显示滚动条」设置;「自动隐藏」仅在滚动时出现;「始终显示」常驻", segmented("sb-style",
                vec![(ScrollbarStyle::System, "跟随系统"), (ScrollbarStyle::Auto, "自动隐藏"), (ScrollbarStyle::Always, "始终显示")], a.scrollbar, cx,
                |this, v, cx| this.update_settings(cx, |s| s.appearance.scrollbar = v)), cx),
        ], cx).into_any_element(),
        group("流光样式", rows![
            row("流光效果", a.shimmer.describe(), segmented("shimmer-kind",
                ShimmerKind::ALL.iter().map(|k| (*k, k.label())).collect(), a.shimmer, cx,
                |this, v, cx| this.update_settings(cx, |s| s.appearance.shimmer = v)), cx),
            div().px_3().py_3().text_size(px(14.)).text_color(t.muted_foreground)
                .child(crate::shimmer_text::shimmer_label("预览:思考中… (12s · 按 Esc 中断)", "shimmer-demo", cx)),
        ], cx).into_any_element(),
        group("等待动画", spinners, cx).into_any_element(),
        note("运行中会话流末尾的「处理中…」与思考行会按所选风格显示文案;颜色跟随主题强调色", cx).into_any_element(),
    ]
}
