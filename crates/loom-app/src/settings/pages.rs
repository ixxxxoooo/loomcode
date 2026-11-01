//! 通用 / 对话 / 快捷键 / 关于 四个设置页。
//!
//! @author ixxxxoooo

use super::widgets::{page_title, row, section_title, segmented};
use crate::ui::{MONO, pill};
use crate::workbench::Workbench;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_config::ThemeChoice;
use loom_core::*;

/// 快捷键清单,按窗口分组(与 `main.rs`、`composer.rs`、`ide_window.rs` 里的绑定保持一致)。
pub const SHORTCUTS: &[(&str, &[(&str, &str)])] = &[
    ("全局", &[
        ("⌘K", "命令面板 / 中央搜索"),
        ("⌘N", "新会话"),
        ("⌘B", "显示 / 隐藏左侧栏"),
        ("⌥⌘B", "显示 / 隐藏右侧面板"),
        ("⌘,", "打开设置"),
        ("⌘S", "保存当前文件"),
        ("⇧⌘E", "切换 Agent / IDE 窗口"),
        ("⌃`", "显示 / 隐藏终端"),
        ("⌃⇧`", "新建终端"),
    ]),
    ("输入框", &[
        ("Enter", "发送消息"),
        ("⇧Enter", "换行"),
        ("⇧Tab", "循环切换权限模式"),
        ("⌥T", "循环切换思考强度"),
        ("↑ / ↓", "在补全列表中移动"),
        ("Tab", "接受补全"),
        ("Esc", "关闭弹层 / 补全"),
    ]),
    ("IDE 窗口", &[
        ("⌘P", "快速打开文件"),
        ("⇧⌘F", "在项目中搜索"),
        ("⌃G", "跳转到行"),
        ("⌘L", "把选中代码引用到对话"),
        ("⌘W", "关闭标签"),
        ("⌥⌘← / ⌥⌘→", "上一个 / 下一个标签"),
    ]),
];

/// 通用页:外观、融合模式、语言。
pub fn general(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let theme = wb.settings.theme;
    let right_open = wb.settings.right_open;
    let fused = wb.settings.fused_ide;
    v_flex().w_full()
        .child(page_title("通用"))
        .child(section_title("外观", cx))
        .child(row("主题", "纸感浅色 / 深色,或跟随系统", segmented("theme", vec![(ThemeChoice::PaperLight, "浅色"), (ThemeChoice::PaperDark, "深色"), (ThemeChoice::System, "跟随系统")], theme, cx,
            |this, v, cx| this.update_settings(cx, |s| s.theme = v)), cx))
        .child(row("右侧面板默认打开", "启动时是否显示改动 / 文件 / 终端面板", Switch::new("sw-right").checked(right_open)
            .on_click(cx.listener(move |this, checked: &bool, _, cx| { let v = *checked; this.update_settings(cx, |s| s.right_open = v) })), cx))
        .child(row("融合模式", "关闭时 IDE 是独立窗口(⇧⌘E 打开或回到 Agent);开启后在同一窗口原地切换", Switch::new("sw-fused").checked(fused)
            .on_click(cx.listener(move |this, checked: &bool, _, cx| { let v = *checked; this.update_settings(cx, |s| s.fused_ide = v); this.on_fused_changed(cx) })), cx))
        .child(section_title("语言", cx))
        .child(row("界面语言", "目前仅提供简体中文", pill("简体中文", cx.theme().foreground, cx.theme().muted), cx))
        .into_any_element()
}

/// 对话页:新会话默认模型 / 权限模式 / 思考强度,思考显示方式。
pub fn chat(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let s = wb.settings.clone();
    let models: Vec<(String, String)> = default_models().into_iter().map(|m| (m.id, m.name)).collect();
    let cur_model = s.default_model.clone();
    v_flex().w_full()
        .child(page_title("对话"))
        .child(section_title("新会话默认值", cx))
        .child(row("默认模型", "新建会话时使用", h_flex().gap_1().children(models.into_iter().enumerate().map(|(i, (id, name))| {
            let active = id == cur_model;
            let id2 = id.clone();
            Button::new(SharedString::from(format!("dm-{i}"))).small().when(active, |b| b.primary()).when(!active, |b| b.ghost()).label(name)
                .on_click(cx.listener(move |this, _, _, cx| { let v = id2.clone(); this.update_settings(cx, |s| s.default_model = v) }))
        })), cx))
        .child(row("默认权限模式", "也可以在输入框随时切换(⇧Tab)", segmented("dmode", PermissionMode::ALL.iter().map(|m| (*m, m.label())).collect(), s.default_mode, cx,
            |this, v, cx| this.update_settings(cx, |s| s.default_mode = v)), cx))
        .child(row("默认思考强度", "也可以在输入框随时切换(⌥T)", segmented("deff", Effort::ALL.iter().map(|e| (*e, e.label())).collect(), s.default_effort, cx,
            |this, v, cx| this.update_settings(cx, |s| s.default_effort = v)), cx))
        .child(section_title("显示", cx))
        .child(row("默认展开思考内容", "关闭时思考折叠为「已思考」一行", Switch::new("sw-think").checked(s.expand_thinking)
            .on_click(cx.listener(move |this, checked: &bool, _, cx| { let v = *checked; this.update_settings(cx, |s| s.expand_thinking = v) })), cx))
        .into_any_element()
}

/// 快捷键页(只读,按窗口分组)。
pub fn shortcuts(cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    v_flex().w_full()
        .child(page_title("快捷键"))
        .children(SHORTCUTS.iter().map(|(group, items)| {
            v_flex().w_full()
                .child(section_title(group, cx))
                .children(items.iter().map(|(k, d)| {
                    h_flex().w_full().py_2().gap_3().items_center().border_b_1().border_color(t.border)
                        .child(div().flex_1().text_size(px(13.)).child(d.to_string()))
                        .child(div().px_2().h(px(22.)).flex().items_center().rounded(px(5.)).bg(t.muted).font_family(MONO).text_size(px(12.)).child(k.to_string()))
                }))
        }))
        .into_any_element()
}

/// 关于页:版本、claude 版本、运行模式、配置文件路径。
pub fn about(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let claude = wb.claude_info.clone().unwrap_or_else(|| if wb.demo { "演示模式(未连接)".into() } else { "检测中…".into() });
    v_flex().w_full()
        .child(page_title("关于"))
        .child(row("LoomCode", "Rust + gpui-kit 的 Claude Code 工作台", pill(format!("v{}", env!("CARGO_PKG_VERSION")), t.foreground, t.muted), cx))
        .child(row("Claude Code", "本机 claude 命令行", div().font_family(MONO).text_size(px(12.5)).child(claude), cx))
        .child(row("运行模式", "", pill(if wb.demo { "演示" } else { "真实" }, if wb.demo { t.warning } else { t.success }, t.muted), cx))
        .child(row("配置文件", "", div().font_family(MONO).text_size(px(12.)).text_color(t.muted_foreground).child(wb.settings_path.display().to_string()), cx))
        .into_any_element()
}
