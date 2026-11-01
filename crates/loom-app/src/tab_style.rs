//! 标签的统一外观:IDE 编辑器文件标签与 IDE 对话栏会话标签共用同一套尺寸、字号、
//! 选中态(顶部 2px 主题色条,绝对定位,不挤占内容)与关闭按钮。
//!
//! @author ixxxxoooo

use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, Icon, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// 标签条高度。
pub const TAB_H: f32 = 36.;
/// 标签字号。
pub const TAB_FONT: f32 = 13.;

/// 一个标签的外壳:统一高度、内边距、字号、右分隔线与选中 / 悬停底色。
///
/// 参数:`id` 元素 id;`group` 悬停分组名(供关闭按钮「悬停才显示」);`active` 是否选中。
/// 返回:可继续链式追加事件与子元素的标签外壳(选中时自带顶部色条)。
pub fn tab_shell(id: SharedString, group: SharedString, active: bool, cx: &App) -> Stateful<Div> {
    let t = cx.theme();
    h_flex().id(id).group(group).relative().h_full().flex_none().pl_3().pr_1().gap_1p5().items_center().cursor_pointer()
        .text_size(px(TAB_FONT)).border_r_1().border_color(t.border)
        .when(active, |d| d.bg(t.background).text_color(t.foreground).child(div().absolute().top_0().left_0().right_0().h(px(2.)).bg(t.primary)))
        .when(!active, |d| d.text_color(t.muted_foreground).hover(|s| s.bg(t.list_hover)))
}

/// 标签右侧的关闭按钮盒子(18px 圆角,悬停加底色);图标可见性由调用方在子元素上控制。
///
/// 参数:`id` 元素 id。返回:无子元素的盒子,调用方追加图标 / 圆点与点击处理。
pub fn tab_close_box(id: SharedString, cx: &App) -> Stateful<Div> {
    let t = cx.theme();
    div().id(id).size(px(18.)).flex_none().flex().items_center().justify_center().rounded(px(4.)).hover(|s| s.bg(t.border))
}

/// 关闭图标(统一 12px、弱化色)。
pub fn tab_close_icon(cx: &App) -> Icon {
    icon(IconName::X, 12., cx.theme().muted_foreground)
}
