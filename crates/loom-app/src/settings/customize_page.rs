//! 「自定义」页:一个搜索框加七个标签(插件 / MCP / 技能 / 命令 / 子 Agent / 规则 / Hooks),
//! 内容复用对应的管理页。左侧栏的「自定义」入口(整页)与设置里的「自定义」分类共用这一份渲染。
//!
//! @author ixxxxoooo

use super::{claude_pages, hooks_page, items_page, mcp_page, plugins_page};
use crate::ui::icon;
use crate::workbench::Workbench;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_config::items::ItemKind;

/// 自定义页的标签。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomizeTab {
    Plugins,
    Mcp,
    Skills,
    Commands,
    Agents,
    Rules,
    Hooks,
}

impl CustomizeTab {
    pub const ALL: [CustomizeTab; 7] = [Self::Plugins, Self::Mcp, Self::Skills, Self::Commands, Self::Agents, Self::Rules, Self::Hooks];

    /// 标签名。
    pub fn label(self) -> &'static str {
        match self {
            Self::Plugins => "插件",
            Self::Mcp => "MCP",
            Self::Skills => "技能",
            Self::Commands => "命令",
            Self::Agents => "子 Agent",
            Self::Rules => "规则",
            Self::Hooks => "Hooks",
        }
    }

    /// 按小写名查找(调试用环境变量 `LOOM_CUSTOMIZE`)。
    pub fn from_name(name: &str) -> Option<Self> {
        let key = |t: &Self| match t {
            Self::Plugins => "plugins",
            Self::Mcp => "mcp",
            Self::Skills => "skills",
            Self::Commands => "commands",
            Self::Agents => "agents",
            Self::Rules => "rules",
            Self::Hooks => "hooks",
        };
        Self::ALL.into_iter().find(|t| key(t) == name)
    }
}

/// 渲染自定义页内容(不含外层滚动容器)。
///
/// 参数:`wb` 工作台;`window` / `cx` gpui 上下文。返回:页面元素。
pub fn render(wb: &mut Workbench, window: &mut Window, cx: &mut Context<Workbench>) -> AnyElement {
    wb.ensure_claude_cfg();
    wb.ensure_forms(window, cx);
    let tab = wb.customize_tab;
    let filter = wb.field_value("search", cx).to_lowercase();
    let body = match tab {
        CustomizeTab::Plugins => plugins_page::render(wb, &filter, cx),
        CustomizeTab::Mcp => mcp_page::mcp(wb, &filter, cx),
        CustomizeTab::Skills => items_page::render(wb, ItemKind::Skill, tab, &filter, cx),
        CustomizeTab::Commands => items_page::render(wb, ItemKind::Command, tab, &filter, cx),
        CustomizeTab::Agents => items_page::render(wb, ItemKind::Agent, tab, &filter, cx),
        CustomizeTab::Rules => claude_pages::memory(wb, window, cx),
        CustomizeTab::Hooks => hooks_page::hooks(wb, &filter, cx),
    };
    let t = cx.theme().clone();
    v_flex().w_full().gap_4()
        .child(v_flex().w_full().gap_3()
            .child(h_flex().w_full().items_center().gap_2()
                .child(div().flex_1().child(Input::new(wb.field("search")).small().prefix(icon(IconName::Search, 14., t.muted_foreground))))
                .child(Button::new("customize-refresh").small().ghost().icon(IconName::RefreshCw).tooltip("重新读取")
                    .on_click(cx.listener(|this, _, _, cx| { this.reload_claude_cfg(); this.refresh_plugins(); cx.notify(); }))))
            .child(h_flex().gap_1p5().flex_wrap().children(CustomizeTab::ALL.into_iter().map(|c| {
                let on = c == tab;
                h_flex().id(SharedString::from(format!("cz-{}", c.label()))).h(px(28.)).px_3().items_center().rounded(px(6.)).border_1().cursor_pointer().text_size(px(13.))
                    .when(on, |d| d.bg(t.primary.opacity(0.12)).border_color(t.primary.opacity(0.35)).text_color(t.primary).font_weight(FontWeight::MEDIUM))
                    .when(!on, |d| d.bg(t.muted).border_color(t.border).hover(|d| d.bg(t.list_hover)))
                    .on_click(cx.listener(move |this, _, _, cx| { this.customize_tab = c; cx.notify(); }))
                    .child(c.label())
            }))))
        .child(div().w_full().h(px(1.)).bg(t.border))
        .child(body)
        .into_any_element()
}

/// 主窗口里的整页:居中可滚动,宽度比设置页稍宽。
pub fn render_page(wb: &mut Workbench, window: &mut Window, cx: &mut Context<Workbench>) -> AnyElement {
    let content = render(wb, window, cx);
    div().id("customize-scroll").size_full().overflow_y_scroll().flex().justify_center().px_6().pb_10()
        .child(v_flex().w_full().max_w(px(900.)).pt_4().child(content))
        .into_any_element()
}
