//! 设置页:左侧分类导航 + 右侧表单。所有修改即时生效并写入 `~/.loomcode/settings.json`。
//!
//! 子模块:`widgets` 公共控件;`pages` 通用 / 对话 / 快捷键 / 关于;
//! `claude_pages` 权限 / MCP / 钩子 / 记忆;`actions` 这些页面的 Workbench 方法。
//!
//! @author ixxxxoooo

mod actions;
mod claude_pages;
mod pages;
mod widgets;

pub use actions::{claude_home, new_rule_input};

use crate::ui::icon;
use crate::workbench::Workbench;
use gpui_kit::assets::IconName;
use gpui_kit::component::input::EditorState;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::path::PathBuf;

/// 设置分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    General,
    Chat,
    Permissions,
    Mcp,
    Hooks,
    Memory,
    Shortcuts,
    About,
}

impl SettingsTab {
    pub const ALL: [SettingsTab; 8] = [
        Self::General,
        Self::Chat,
        Self::Permissions,
        Self::Mcp,
        Self::Hooks,
        Self::Memory,
        Self::Shortcuts,
        Self::About,
    ];

    /// 导航里显示的名称。
    pub fn label(self) -> &'static str {
        match self {
            Self::General => "通用",
            Self::Chat => "对话",
            Self::Permissions => "权限",
            Self::Mcp => "MCP 服务器",
            Self::Hooks => "钩子",
            Self::Memory => "记忆",
            Self::Shortcuts => "快捷键",
            Self::About => "关于",
        }
    }

    /// 导航图标。
    pub fn icon(self) -> IconName {
        match self {
            Self::General => IconName::SlidersHorizontal,
            Self::Chat => IconName::MessageSquare,
            Self::Permissions => IconName::Shield,
            Self::Mcp => IconName::Plug,
            Self::Hooks => IconName::Webhook,
            Self::Memory => IconName::Brain,
            Self::Shortcuts => IconName::Keyboard,
            Self::About => IconName::Info,
        }
    }
}

/// 记忆页正在编辑的 CLAUDE.md。
pub struct MemoryDoc {
    pub path: PathBuf,
    pub editor: Entity<EditorState>,
    pub saved: bool,
}

/// 渲染整个设置页。
///
/// 参数:`wb` 工作台;`window` / `cx` gpui 上下文。返回:设置页元素。
pub fn render(wb: &mut Workbench, window: &mut Window, cx: &mut Context<Workbench>) -> AnyElement {
    // 当前项目变了就按新项目重读 Claude 配置
    wb.ensure_claude_cfg();
    let t = cx.theme().clone();
    let tab = wb.settings_tab;
    let body = match tab {
        SettingsTab::General => pages::general(wb, cx),
        SettingsTab::Chat => pages::chat(wb, cx),
        SettingsTab::Permissions => claude_pages::permissions(wb, cx),
        SettingsTab::Mcp => claude_pages::mcp(wb, cx),
        SettingsTab::Hooks => claude_pages::hooks(wb, cx),
        SettingsTab::Memory => claude_pages::memory(wb, window, cx),
        SettingsTab::Shortcuts => pages::shortcuts(cx),
        SettingsTab::About => pages::about(wb, cx),
    };
    h_flex().size_full().bg(t.background)
        .child(
            v_flex().w(px(200.)).h_full().flex_none().p_2().gap_0p5().border_r_1().border_color(t.border).bg(t.sidebar)
                .child(div().px_2().py_2().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child("设置"))
                .children(SettingsTab::ALL.into_iter().map(|st| {
                    let on = st == tab;
                    h_flex().id(SharedString::from(format!("st-{}", st.label()))).h(px(30.)).px_2().gap_2().items_center().rounded(px(6.)).cursor_pointer().text_size(px(13.))
                        .when(on, |d| d.bg(t.sidebar_accent))
                        .hover(|d| d.bg(t.sidebar_accent))
                        .on_click(cx.listener(move |this, _, _, cx| { this.settings_tab = st; cx.notify(); }))
                        .child(icon(st.icon(), 14., if on { t.foreground } else { t.muted_foreground }))
                        .child(st.label())
                })),
        )
        .child(
            div().id("settings-scroll").flex_1().min_w_0().h_full().overflow_y_scroll().flex().justify_center().px_6().pb_10()
                .child(v_flex().w_full().max_w(px(680.)).pt_6().child(body)),
        )
        .into_any_element()
}
