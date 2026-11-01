//! 设置页:左侧分类导航 + 右侧表单。所有修改即时生效并写入 `~/.loomcode/settings.json`。
//!
//! 子模块:`widgets` 公共控件;`forms` / `fields` 表单状态;`pages` 通用 / 对话 / 快捷键 / 关于;
//! Claude 配置页 `model_page` / `providers_page` / `permissions_page` / `mcp_page` / `hooks_page` /
//! `items_page`(命令、子 Agent、Skills)/ `claude_pages`(记忆与共用编辑器)/ `env_page`;
//! 其他 `network_page` / `usage_page` / `diagnostics_page` / `advanced_page`;`*_actions` 为对应的 Workbench 方法。
//!
//! @author ixxxxoooo

mod actions;
mod advanced_page;
mod claude_pages;
mod diagnostics_page;
mod doc_actions;
mod env_page;
mod fields;
mod forms;
mod hook_actions;
mod hooks_page;
mod items_page;
mod layer_actions;
mod mcp_actions;
mod mcp_page;
mod model_page;
mod network_page;
mod ops_actions;
mod pages;
mod permissions_page;
mod provider_actions;
mod providers_page;
mod usage_page;
mod widgets;

pub use forms::Forms;
pub use usage_page::Metric as UsageMetric;

use crate::ui::icon;
use crate::window_header::{HEADER_H, draggable};
use crate::workbench::{Page, Workbench};
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_config::items::ItemKind;

/// 设置分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    General,
    Chat,
    Shortcuts,
    Model,
    Providers,
    Permissions,
    Mcp,
    Hooks,
    Commands,
    Agents,
    Skills,
    Memory,
    Env,
    Network,
    Usage,
    Diagnostics,
    Advanced,
    About,
}

impl SettingsTab {
    /// 导航分组。
    pub const GROUPS: [(&'static str, &'static [SettingsTab]); 3] = [
        ("客户端", &[Self::General, Self::Chat, Self::Shortcuts]),
        ("Claude Code", &[Self::Model, Self::Providers, Self::Permissions, Self::Mcp, Self::Hooks, Self::Commands, Self::Agents, Self::Skills, Self::Memory, Self::Env]),
        ("其他", &[Self::Network, Self::Usage, Self::Diagnostics, Self::Advanced, Self::About]),
    ];

    /// 全部分类(`LOOM_SETTINGS` 按小写名查找)。
    pub fn all() -> impl Iterator<Item = SettingsTab> {
        Self::GROUPS.into_iter().flat_map(|(_, tabs)| tabs.iter().copied())
    }

    /// 按小写名查找(如 `mcp`、`providers`)。
    pub fn from_name(name: &str) -> Option<SettingsTab> {
        Self::all().find(|t| format!("{t:?}").to_lowercase() == name)
    }

    /// 导航里显示的名称。
    pub fn label(self) -> &'static str {
        match self {
            Self::General => "通用",
            Self::Chat => "对话",
            Self::Shortcuts => "快捷键",
            Self::Model => "模型与思考",
            Self::Providers => "模型服务商",
            Self::Permissions => "权限",
            Self::Mcp => "MCP 服务器",
            Self::Hooks => "钩子",
            Self::Commands => "斜杠命令",
            Self::Agents => "子 Agent",
            Self::Skills => "Skills",
            Self::Memory => "记忆",
            Self::Env => "环境变量",
            Self::Network => "网络",
            Self::Usage => "用量",
            Self::Diagnostics => "诊断",
            Self::Advanced => "高级",
            Self::About => "关于",
        }
    }

    /// 导航图标。
    pub fn icon(self) -> IconName {
        match self {
            Self::General => IconName::SlidersHorizontal,
            Self::Chat => IconName::MessageSquare,
            Self::Shortcuts => IconName::Keyboard,
            Self::Model => IconName::Sparkle,
            Self::Providers => IconName::Server,
            Self::Permissions => IconName::Shield,
            Self::Mcp => IconName::Plug,
            Self::Hooks => IconName::Webhook,
            Self::Commands => IconName::SquareTerminal,
            Self::Agents => IconName::Bot,
            Self::Skills => IconName::Sparkles,
            Self::Memory => IconName::Brain,
            Self::Env => IconName::Variable,
            Self::Network => IconName::Globe,
            Self::Usage => IconName::ChartColumn,
            Self::Diagnostics => IconName::Stethoscope,
            Self::Advanced => IconName::Wrench,
            Self::About => IconName::Info,
        }
    }
}

/// 渲染整个设置页。
///
/// 参数:`wb` 工作台;`window` / `cx` gpui 上下文。返回:设置页元素。
pub fn render(wb: &mut Workbench, window: &mut Window, cx: &mut Context<Workbench>) -> AnyElement {
    // 当前项目变了就按新项目重读 Claude 配置
    wb.ensure_claude_cfg();
    wb.ensure_forms(window, cx);
    let t = cx.theme().clone();
    let tab = wb.settings_tab;
    wb.sync_fields(tab, window, cx);
    wb.enter_tab(tab, cx);
    let body = match tab {
        SettingsTab::General => pages::general(wb, cx),
        SettingsTab::Chat => pages::chat(wb, cx),
        SettingsTab::Model => model_page::render(wb, cx),
        SettingsTab::Providers => providers_page::render(wb, cx),
        SettingsTab::Env => env_page::render(wb, cx),
        SettingsTab::Network => network_page::render(wb, cx),
        SettingsTab::Usage => usage_page::render(wb, cx),
        SettingsTab::Diagnostics => diagnostics_page::render(wb, cx),
        SettingsTab::Advanced => advanced_page::render(wb, cx),
        SettingsTab::Permissions => permissions_page::permissions(wb, cx),
        SettingsTab::Mcp => mcp_page::mcp(wb, cx),
        SettingsTab::Hooks => hooks_page::hooks(wb, cx),
        SettingsTab::Commands => items_page::render(wb, ItemKind::Command, tab, cx),
        SettingsTab::Agents => items_page::render(wb, ItemKind::Agent, tab, cx),
        SettingsTab::Skills => items_page::render(wb, ItemKind::Skill, tab, cx),
        SettingsTab::Memory => claude_pages::memory(wb, window, cx),
        SettingsTab::Shortcuts => pages::shortcuts(cx),
        SettingsTab::About => pages::about(wb, cx),
    };
    // 整窗页面,没有顶部行:导航栏顶部留出红绿灯位置(可拖动窗口),下面是「返回」,内容区顶部同高的空白也可拖动
    h_flex().size_full().bg(t.background)
        .child(
            v_flex().w(px(200.)).h_full().flex_none().px_2().pb_2().gap_0p5().border_r_1().border_color(t.border).bg(t.sidebar)
                .child(draggable(div().id("st-drag-nav").w_full().h(px(HEADER_H)).flex_none()))
                .child(
                    h_flex().id("st-back").h(px(30.)).px_2().mb_2().gap_2().items_center().rounded(px(6.)).cursor_pointer().text_size(px(13.))
                        .hover(|d| d.bg(t.sidebar_accent))
                        .on_click(cx.listener(|this, _, _, cx| this.set_page(Page::Chat, cx)))
                        .child(icon(IconName::ArrowLeft, 14., t.muted_foreground))
                        .child("返回"),
                )
                .child(v_flex().id("st-nav-scroll").flex_1().min_h_0().overflow_y_scroll().gap_0p5()
                    .children(SettingsTab::GROUPS.into_iter().map(|(group, tabs)| {
                        v_flex().gap_0p5()
                            .child(div().px_2().pt_2().pb_1().text_size(px(11.)).text_color(t.muted_foreground).child(group))
                            .children(tabs.iter().copied().map(|st| {
                                let on = st == tab;
                                h_flex().id(SharedString::from(format!("st-{}", st.label()))).h(px(28.)).px_2().gap_2().items_center().rounded(px(6.)).cursor_pointer().text_size(px(13.))
                                    .when(on, |d| d.bg(t.sidebar_accent))
                                    .hover(|d| d.bg(t.sidebar_accent))
                                    .on_click(cx.listener(move |this, _, _, cx| { this.settings_tab = st; cx.notify(); }))
                                    .child(icon(st.icon(), 14., if on { t.foreground } else { t.muted_foreground }))
                                    .child(st.label())
                            }))
                    }))),
        )
        .child(
            v_flex().flex_1().min_w_0().h_full()
                .child(draggable(div().id("st-drag-body").w_full().h(px(HEADER_H)).flex_none()))
                .child(
                    div().id("settings-scroll").flex_1().min_h_0().w_full().overflow_y_scroll().flex().justify_center().px_6().pb_10()
                        .child(v_flex().w_full().max_w(px(680.)).child(body)),
                ),
        )
        .into_any_element()
}
