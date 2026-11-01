//! 设置页:左侧分类导航(带搜索、键盘上下切换、记住上次分类)+ 右侧表单。所有修改即时生效并写入 `~/.loomcode/settings.json`。
//!
//! 子模块:`widgets` 公共控件;`forms` / `fields` 表单状态;`nav` 导航搜索与键盘;`pages` 通用 / 外观 / 对话 / 快捷键 / 关于;
//! Claude 配置页 `model_page` / `providers_page`(含模型映射,见 `mapping_page`) / `prompts_page` / `permissions_page` / `trust_page` / `env_page`;
//! 工作台页 `editor_page` / `terminal_page` / `browser_page` / `scm_page` / `intel_page`;
//! 插件、MCP、技能、命令、子 Agent、规则、Hooks 统一收在「自定义」里(`customize_page`,左侧栏入口与设置分类共用),
//! 各标签的内容是 `plugins_page` / `mcp_page` / `items_page` / `claude_pages`(记忆与共用编辑器)/ `hooks_page`;
//! 其他 `network_page` / `usage_page` / `privacy_page` / `diagnostics_page` / `advanced_page`;`*_actions` 为对应的 Workbench 方法。
//!
//! @author ixxxxoooo

mod actions;
mod advanced_page;
mod appearance_page;
mod browser_page;
mod claude_pages;
mod cli;
mod confirm;
mod customize_page;
mod density_preview;
mod diag_demo;
mod diagnostics_page;
mod doc_actions;
mod editor_page;
mod ext_ui;
mod env_page;
mod fields;
mod forms;
mod hook_actions;
mod hooks_page;
mod intel_page;
mod item_actions;
mod items_page;
mod layer_actions;
mod mapping_page;
mod mcp_actions;
mod mcp_page;
mod model_page;
mod nav;
mod network_page;
mod ops_actions;
mod pages;
mod plugin_actions;
mod plugins_page;
mod permissions_page;
mod privacy_page;
mod prompts_page;
mod provider_actions;
mod provider_switch;
mod providers_page;
mod scm_page;
mod terminal_page;
mod trust_page;
mod usage_page;
mod widgets;

pub use customize_page::CustomizeTab;
pub use forms::Forms;
pub use usage_page::Metric as UsageMetric;

use crate::theme::R_SM;
use crate::ui::icon;
use std::time::Duration;
use crate::window_header::{HEADER_H, draggable};
use crate::workbench::{Page, Workbench};
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::component::input::Input;
use gpui_kit::*;

/// 设置分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    General,
    Appearance,
    Chat,
    Shortcuts,
    Customize,
    Model,
    Providers,
    Prompts,
    Permissions,
    Trust,
    Env,
    Network,
    Editor,
    Terminal,
    Browser,
    Scm,
    Intel,
    Usage,
    Privacy,
    Diagnostics,
    Advanced,
    About,
}

impl SettingsTab {
    /// 导航分组。
    pub const GROUPS: [(&'static str, &'static [SettingsTab]); 4] = [
        ("客户端", &[Self::General, Self::Appearance, Self::Chat, Self::Shortcuts]),
        ("Claude Code", &[Self::Customize, Self::Model, Self::Providers, Self::Prompts, Self::Permissions, Self::Trust, Self::Env]),
        ("工作台", &[Self::Editor, Self::Terminal, Self::Browser, Self::Scm, Self::Intel]),
        ("其他", &[Self::Network, Self::Usage, Self::Privacy, Self::Diagnostics, Self::Advanced, Self::About]),
    ];

    /// 全部分类(`LOOM_SETTINGS` 按小写名查找)。
    pub fn all() -> impl Iterator<Item = SettingsTab> {
        Self::GROUPS.into_iter().flat_map(|(_, tabs)| tabs.iter().copied())
    }

    /// 稳定的小写名(`LOOM_SETTINGS`、⌘K、记住上次分类都用它)。
    pub fn key(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Appearance => "appearance",
            Self::Chat => "chat",
            Self::Shortcuts => "shortcuts",
            Self::Customize => "customize",
            Self::Model => "model",
            Self::Providers => "providers",
            Self::Prompts => "prompts",
            Self::Permissions => "permissions",
            Self::Trust => "trust",
            Self::Env => "env",
            Self::Network => "network",
            Self::Editor => "editor",
            Self::Terminal => "terminal",
            Self::Browser => "browser",
            Self::Scm => "scm",
            Self::Intel => "intel",
            Self::Usage => "usage",
            Self::Privacy => "privacy",
            Self::Diagnostics => "diagnostics",
            Self::Advanced => "advanced",
            Self::About => "about",
        }
    }

    /// 按小写名查找(如 `mcp`、`providers`)。
    pub fn from_name(name: &str) -> Option<SettingsTab> {
        Self::all().find(|t| t.key() == name)
    }

    /// 导航里显示的名称。
    pub fn label(self) -> &'static str {
        match self {
            Self::General => "通用",
            Self::Appearance => "外观",
            Self::Chat => "对话",
            Self::Shortcuts => "快捷键",
            Self::Customize => "自定义",
            Self::Model => "模型与思考",
            Self::Providers => "模型服务商",
            Self::Prompts => "提示词",
            Self::Permissions => "权限",
            Self::Trust => "项目信任",
            Self::Env => "环境变量",
            Self::Network => "网络",
            Self::Editor => "编辑器",
            Self::Terminal => "终端",
            Self::Browser => "浏览器",
            Self::Scm => "源代码管理",
            Self::Intel => "代码智能",
            Self::Usage => "用量与成本",
            Self::Privacy => "隐私与遥测",
            Self::Diagnostics => "诊断",
            Self::Advanced => "高级",
            Self::About => "关于",
        }
    }

    /// 搜索用的关键词(页内主要设置项的名称)。
    pub fn keywords(self) -> &'static str {
        match self {
            Self::General => "窗口 输入 通知 菜单栏 托盘 语言 启动 发送",
            Self::Appearance => "主题 颜色 强调色 深色 浅色 paper one 配色",
            Self::Chat => "对话 思考 展开 消息 时间线",
            Self::Shortcuts => "快捷键 键盘 按键",
            Self::Customize => "插件 mcp 技能 skills 命令 子agent 规则 钩子 hooks 记忆 claude.md 市场",
            Self::Model => "模型 思考 thinking 压缩 compact 输出风格 effort 默认模式",
            Self::Providers => "服务商 api key base url 密钥 deepseek 映射 模型映射 opus sonnet haiku 槽位",
            Self::Prompts => "提示词 系统提示 追加 prompt",
            Self::Permissions => "权限 允许 拒绝 询问 规则 模式 bypass",
            Self::Trust => "信任 项目信任 hooks mcp",
            Self::Env => "环境变量 env",
            Self::Network => "网络 代理 proxy socks 超时 no_proxy 连通性",
            Self::Editor => "编辑器 迷你地图 minimap 行号 折叠 换行 缩进 自动保存 括号 语法 高亮",
            Self::Terminal => "终端 字号 光标 shell 回看 登录 scrollback",
            Self::Browser => "浏览器 user-agent 超时 抓取",
            Self::Scm => "源代码管理 git worktree diff 改动",
            Self::Intel => "代码智能 忽略 搜索 lsp 诊断",
            Self::Usage => "用量 成本 token 费用 预算 估算",
            Self::Privacy => "隐私 遥测 telemetry 崩溃 报告",
            Self::Diagnostics => "诊断 日志 doctor 进程 导出",
            Self::Advanced => "高级 重置 claude 路径 配置 备份",
            Self::About => "关于 版本 许可 更新",
        }
    }

    /// 导航图标。
    pub fn icon(self) -> IconName {
        match self {
            Self::General => IconName::SlidersHorizontal,
            Self::Appearance => IconName::Palette,
            Self::Chat => IconName::MessageSquare,
            Self::Shortcuts => IconName::Keyboard,
            Self::Customize => IconName::Plug,
            Self::Model => IconName::Sparkle,
            Self::Providers => IconName::Server,
            Self::Prompts => IconName::FileText,
            Self::Permissions => IconName::Shield,
            Self::Trust => IconName::ShieldCheck,
            Self::Env => IconName::Variable,
            Self::Network => IconName::Globe,
            Self::Editor => IconName::Code,
            Self::Terminal => IconName::SquareTerminal,
            Self::Browser => IconName::Network,
            Self::Scm => IconName::GitBranch,
            Self::Intel => IconName::Braces,
            Self::Usage => IconName::ChartColumn,
            Self::Privacy => IconName::Lock,
            Self::Diagnostics => IconName::Stethoscope,
            Self::Advanced => IconName::Wrench,
            Self::About => IconName::Info,
        }
    }

    /// 是否命中搜索词(分类名、分组名、关键词任一包含,不区分大小写)。
    pub fn matches(self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return true;
        }
        let group = Self::GROUPS.iter().find(|(_, tabs)| tabs.contains(&self)).map_or("", |(g, _)| *g);
        [self.label(), group, self.keywords()].iter().any(|s| s.to_lowercase().contains(&q))
    }
}

/// 渲染「自定义」整页(左侧栏入口,主窗口中央区域)。
pub fn render_customize(wb: &mut Workbench, window: &mut Window, cx: &mut Context<Workbench>) -> AnyElement {
    customize_page::render_page(wb, window, cx)
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
        SettingsTab::Appearance => pages::appearance(wb, cx),
        SettingsTab::Chat => pages::chat(wb, cx),
        SettingsTab::Customize => customize_page::render(wb, window, cx),
        SettingsTab::Model => model_page::render(wb, cx),
        SettingsTab::Providers => providers_page::render(wb, cx),
        SettingsTab::Prompts => prompts_page::render(wb, cx),
        SettingsTab::Env => env_page::render(wb, cx),
        SettingsTab::Network => network_page::render(wb, cx),
        SettingsTab::Editor => editor_page::render(wb, cx),
        SettingsTab::Terminal => terminal_page::render(wb, cx),
        SettingsTab::Browser => browser_page::render(wb, cx),
        SettingsTab::Scm => scm_page::render(wb, cx),
        SettingsTab::Intel => intel_page::render(wb, cx),
        SettingsTab::Usage => usage_page::render(wb, cx),
        SettingsTab::Privacy => privacy_page::render(wb, cx),
        SettingsTab::Diagnostics => diagnostics_page::render(wb, cx),
        SettingsTab::Advanced => advanced_page::render(wb, cx),
        SettingsTab::Permissions => permissions_page::permissions(wb, cx),
        SettingsTab::Trust => trust_page::render(wb, cx),
        SettingsTab::Shortcuts => pages::shortcuts(cx),
        SettingsTab::About => pages::about(wb, cx),
    };
    let query = wb.field_value("settings_search", cx);
    let searching = !query.trim().is_empty();
    let visible: Vec<SettingsTab> = SettingsTab::all().filter(|t| t.matches(&query)).collect();
    // 整窗页面,没有顶部行:导航栏顶部留出红绿灯位置(可拖动窗口),下面是「返回」与搜索框,内容区顶部同高的空白也可拖动
    h_flex().size_full().bg(t.background)
        .child(
            v_flex().w(px(210.)).h_full().flex_none().px_2().pb_2().gap_0p5().border_r_1().border_color(t.border).bg(t.sidebar)
                .child(draggable(div().id("st-drag-nav").w_full().h(px(HEADER_H)).flex_none()))
                .child(
                    h_flex().id("st-back").h(px(30.)).px_2().gap_2().items_center().rounded(px(R_SM)).cursor_pointer().text_size(px(13.))
                        .hover(|d| d.bg(t.sidebar_accent))
                        .on_click(cx.listener(|this, _, _, cx| this.set_page(Page::Chat, cx)))
                        .child(icon(IconName::ArrowLeft, 14., t.muted_foreground))
                        .child("返回"),
                )
                .child(div().pt_1().pb_2().child(Input::new(wb.field("settings_search")).small().cleanable(true).prefix(icon(IconName::Search, 13., t.muted_foreground))))
                .child(v_flex().id("st-nav-scroll").flex_1().min_h_0().overflow_y_scroll().gap_0p5()
                    .children(SettingsTab::GROUPS.into_iter().filter_map(|(group, tabs)| {
                        let tabs: Vec<SettingsTab> = tabs.iter().copied().filter(|t| visible.contains(t)).collect();
                        (!tabs.is_empty()).then(|| {
                            v_flex().gap_0p5()
                                .child(div().px_2().pt_2().pb_1().text_size(px(11.)).text_color(t.muted_foreground).child(group))
                                .children(tabs.into_iter().map(|st| {
                                    let on = st == tab;
                                    // 搜索时第一项带「回车跳转」提示色
                                    let first = searching && visible.first() == Some(&st);
                                    h_flex().id(SharedString::from(format!("st-{}", st.key()))).h(px(28.)).px_2().gap_2().items_center().rounded(px(R_SM)).cursor_pointer().text_size(px(13.))
                                        .when(on, |d| d.bg(t.sidebar_accent))
                                        .when(first && !on, |d| d.bg(t.sidebar_accent.opacity(0.5)))
                                        .hover(|d| d.bg(t.sidebar_accent))
                                        .on_click(cx.listener(move |this, _, _, cx| this.select_settings_tab(st, cx)))
                                        .child(icon(st.icon(), 14., if on { t.foreground } else { t.muted_foreground }))
                                        .child(st.label())
                                }))
                        })
                    }))
                    .when(searching && visible.is_empty(), |d| d.child(div().px_2().pt_3().text_size(px(12.)).text_color(t.muted_foreground).child("没有匹配的设置"))))
        )
        .child(
            v_flex().flex_1().min_w_0().h_full()
                .child(draggable(div().id("st-drag-body").w_full().h(px(HEADER_H)).flex_none()))
                .child(
                    div().id("settings-scroll").flex_1().min_h_0().w_full().overflow_y_scroll().flex().flex_col().items_center().px_6().pb_10()
                        // 切换分类时内容淡入(元素 id 随分类变化,动画会重新开始)
                        .child(v_flex().id(SharedString::from(format!("page-{}", tab.key()))).w_full().max_w(px(f32::from(wb.settings.appearance.settings_width.clamp(560, 1600)))).flex_none().child(body)
                            .with_animation(SharedString::from(format!("fade-{}", tab.key())), Animation::new(Duration::from_millis(150)), |d, k| d.opacity(0.4 + 0.6 * k))),
                ),
        )
        .into_any_element()
}
