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
/// 参数:`project` 项目名;`branch` 当前分支;`info` 为 (行, 列, 语言, 是否 CRLF);`mode` 权限模式文字。
pub fn status_bar(project: Option<String>, branch: Option<String>, info: Option<(u32, u32, &'static str, bool)>, mode: Option<String>, cx: &App) -> impl IntoElement {
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
                .when_some(project, |d, n| d.child(h_flex().gap_1().items_center().child(icon(IconName::Folder, 11., muted)).child(n)))
                .when_some(branch, |d, b| d.child(h_flex().gap_1().items_center().child(icon(IconName::GitBranch, 11., muted)).child(b))),
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
                }),
        )
}
