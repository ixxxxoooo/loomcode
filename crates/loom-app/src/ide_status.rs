//! IDE 窗口底部状态栏:左侧项目与分支,右侧光标位置、语言、编码、换行符(与 VSCode 一致合在一条里)。
//!
//! @author ixxxxoooo

use crate::ui::{MONO, icon};
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// 构造状态栏。
///
/// 参数:`project` 项目名;`branch` 当前分支;`info` 为 (行, 列, 语言, 是否 CRLF);`lsp` 为 (语言服务器状态文字, 错误数, 警告数);`mode` 权限模式文字;`bell` 最右侧的通知铃铛;`on_branch` 点分支名(弹出分支选择器);`demo` 是否演示模式(开发版 / 演示模式标识显示在最左,正对左下角的设置按钮)。
pub fn status_bar(project: Option<String>, branch: Option<String>, info: Option<(u32, u32, &'static str, bool)>, lsp: Option<(String, usize, usize)>, mode: Option<String>, demo: bool, bell: AnyElement, on_branch: impl Fn(&mut Window, &mut App) + 'static, cx: &App) -> impl IntoElement {
    let t = cx.theme().clone();
    let muted = t.muted_foreground;
    // 不用 StatusBar 组件:它不保证内容垂直居中。左右两组都 items_center,整条固定 24px 高
    let group = || h_flex().h_full().items_center().gap_3().px_2();
    h_flex()
        .h(px(24.))
        .w_full()
        .flex_none()
        .items_center()
        .border_t_1()
        .border_color(t.border)
        .bg(t.sidebar)
        .text_size(px(11.))
        .text_color(muted)
        .child(
            group()
                .when_some(crate::ui::env_badges(demo, cx), |d, b| d.child(b))
                .when_some(project, |d, n| d.child(h_flex().gap_1().items_center().child(icon(IconName::Folder, 11., muted)).child(n)))
                .when_some(branch, |d, b| {
                    d.child(
                        h_flex().id("status-branch").h_full().px_1().gap_1().items_center().cursor_pointer()
                            .hover(|d| d.bg(t.list_hover).text_color(t.foreground))
                            .on_click(move |_, window, cx| on_branch(window, cx))
                            .child(icon(IconName::GitBranch, 11., muted)).child(b),
                    )
                })
                // 语言服务器:错误 / 警告计数(有才显示)与服务器名 / 状态
                .when_some(lsp, |d, (label, errors, warnings)| {
                    d.child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .when(errors > 0, |d| d.child(h_flex().gap_1().items_center().text_color(t.danger).child(icon(IconName::CircleX, 11., t.danger)).child(errors.to_string())))
                            .when(warnings > 0, |d| d.child(h_flex().gap_1().items_center().text_color(t.warning).child(icon(IconName::TriangleAlert, 11., t.warning)).child(warnings.to_string())))
                            .child(div().child(label)),
                    )
                }),
        )
        .child(div().flex_1())
        .child(
            group()
                .font_family(MONO)
                .when_some(mode, |d, m| d.child(div().child(format!("权限:{m}"))))
                .when_some(info, |d, (line, col, lang, crlf)| {
                    d.child(format!("行 {line},列 {col}"))
                        .child(if lang == "text" { "纯文本".to_string() } else { lang.to_string() })
                        .child("UTF-8")
                        .child(if crlf { "CRLF" } else { "LF" })
                })
                .child(bell),
        )
}
