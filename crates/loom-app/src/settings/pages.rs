//! 通用 / 对话 / 快捷键 / 关于 四个设置页。
//!
//! @author ixxxxoooo

use super::widgets::{block_row, group, page_header, row, rows, segmented, switch_row};
use crate::ui::{MONO, pill};
use crate::workbench::Workbench;
use loom_config::ThemeChoice;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
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

/// 通用页:窗口行为、输入、通知与菜单栏、语言。
pub fn general(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let right_open = wb.settings.right_open;
    let fused = wb.settings.fused_ide;
    v_flex().w_full()
        .child(page_header("通用", "窗口行为、输入习惯与系统集成", div(), cx))
        .child(group("窗口", rows![
            row("右侧面板默认打开", "启动时是否显示改动 / 文件 / 终端面板", Switch::new("sw-right").checked(right_open)
                .on_click(cx.listener(move |this, checked: &bool, _, cx| { let v = *checked; this.update_settings(cx, |s| s.right_open = v) })), cx),
            row("融合模式", "关闭时 IDE 是独立窗口(⇧⌘E 打开或回到 Agent);开启后在同一窗口原地切换", Switch::new("sw-fused").checked(fused)
                .on_click(cx.listener(move |this, checked: &bool, _, cx| { let v = *checked; this.update_settings(cx, |s| s.fused_ide = v); this.on_fused_changed(cx) })), cx),
        ], cx))
        .child(group("输入", rows![
            row("发送键", "另一个键用于换行(Shift+Enter 始终换行)", segmented("send-key", vec![(false, "Enter 发送"), (true, "⌘Enter 发送")], wb.settings.send_with_cmd_enter, cx,
                |this, v, cx| this.update_settings(cx, |s| s.send_with_cmd_enter = v)), cx),
        ], cx))
        .child(group("通知与菜单栏", rows![
            row("任务完成时通知", "回合结束且 LoomCode 不在前台时发送系统通知", Switch::new("sw-notify").checked(wb.settings.notify_on_finish)
                .on_click(cx.listener(|this, c: &bool, _, cx| { let v = *c; this.update_settings(cx, |s| s.notify_on_finish = v) })), cx),
            row("菜单栏图标", "在系统菜单栏显示 LoomCode 图标", Switch::new("sw-tray").checked(wb.settings.tray_icon)
                .on_click(cx.listener(|this, c: &bool, _, cx| { let v = *c; this.update_settings(cx, |s| s.tray_icon = v); crate::app_icon::set_tray_visible(v) })), cx),
        ], cx))
        .child(group("语言", rows![
            row("界面语言", "目前仅提供简体中文", pill("简体中文", cx.theme().foreground, cx.theme().muted), cx),
        ], cx))
        .into_any_element()
}

/// 外观页:主题与强调色。
pub fn appearance(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    v_flex().w_full()
        .child(page_header("外观", "主题、内容宽度、字号、滚动条与等待动画", div(), cx))
        .child(group("主题", rows![
            block_row("配色方案", "纸感为默认;One 是中性灰蓝风格;也可以跟随系统明暗", theme_picker(wb, cx), cx),
        ], cx))
        .child(group("强调色", rows![
            row("强调色", "按钮、选中态、悬停、焦点环、选区、链接等使用的主题色;每个主题单独记忆,并按明暗自动调整明度", accent_picker(wb, cx), cx),
        ], cx))
        .children(super::appearance_page::sections(wb, cx))
        .into_any_element()
}

/// 主题选择:每个主题一张带色块预览的卡片。
fn theme_picker(wb: &Workbench, cx: &mut Context<Workbench>) -> Div {
    let t = cx.theme().clone();
    let cur = wb.settings.theme;
    h_flex().w_full().flex_wrap().gap_3().children(ThemeChoice::ALL.into_iter().enumerate().map(|(i, choice)| {
        let (name, bg, fg, accent) = crate::theme::preview(choice);
        let on = cur == choice;
        let (bg, fg, accent): (Hsla, Hsla, Hsla) = (rgb(bg).into(), rgb(fg).into(), rgb(accent).into());
        v_flex().id(SharedString::from(format!("theme-{i}"))).w(px(120.)).gap_1p5().cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| this.update_settings(cx, |s| s.theme = choice)))
            .child(v_flex().h(px(68.)).p_2().gap_1().rounded(px(crate::theme::R_MD)).border_2().bg(bg)
                .border_color(if on { t.primary } else { t.border })
                .child(div().w(px(36.)).h(px(5.)).rounded_full().bg(accent))
                .child(div().w(px(70.)).h(px(4.)).rounded_full().bg(fg.opacity(0.55)))
                .child(div().w(px(54.)).h(px(4.)).rounded_full().bg(fg.opacity(0.3))))
            .child(div().text_size(px(12.)).text_color(if on { t.foreground } else { t.muted_foreground }).when(on, |d| d.font_weight(FontWeight::MEDIUM)).child(name.to_string()))
    }))
}

/// 强调色预设:(名称, RRGGBB);`None` 为主题默认。
const ACCENTS: &[(&str, Option<&str>)] = &[("主题默认", None), ("赤陶", Some("C15F3C")), ("橙", Some("EA580C")), ("红", Some("DC2626")), ("粉", Some("DB2777")), ("紫", Some("7C3AED")), ("蓝", Some("2563EB")), ("青", Some("0891B2")), ("绿", Some("16A34A"))];

/// 强调色选择:预设色点 + 自定义十六进制。
fn accent_picker(wb: &Workbench, cx: &mut Context<Workbench>) -> Div {
    let t = cx.theme().clone();
    let cur = wb.settings.accent.clone();
    h_flex().gap_1p5().items_center()
        .children(ACCENTS.iter().enumerate().map(|(i, (_name, hex))| {
            // 色点显示该主题下的实际效果(按明暗调整过明度);主题默认显示主题自带的主题色
            let color: Hsla = match hex.and_then(crate::theme::parse_hex) {
                Some(v) => crate::theme::adapt_accent(rgb(v).into(), t.is_dark()),
                None => rgb(crate::theme::preview(wb.settings.theme).3).into(),
            };
            let on = cur.as_deref() == *hex;
            let v = hex.map(str::to_string);
            div().id(SharedString::from(format!("accent-{i}"))).size(px(20.)).rounded_full().bg(color).cursor_pointer()
                .border_2().border_color(if on { t.foreground } else { t.background })
                .on_click(cx.listener(move |this, _, _, cx| { let v = v.clone(); this.update_settings(cx, |s| s.accent = v) }))
        }))
        .child(div().w(px(110.)).child(Input::new(wb.field("accent")).small()))
        .child(Button::new("accent-save").small().outline().label("应用").on_click(cx.listener(|this, _, _, cx| {
            let v = this.field_value("accent", cx);
            if v.is_empty() || crate::theme::parse_hex(&v).is_some() {
                this.update_settings(cx, |s| s.accent = (!v.is_empty()).then(|| v.trim_start_matches('#').to_uppercase()));
            }
        })))
}

/// 对话页:新会话默认模型 / 权限模式 / 思考强度,思考显示方式。
pub fn chat(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let s = wb.settings.clone();
    let models: Vec<(String, String)> = default_models().into_iter().map(|m| (m.id, m.name)).collect();
    let cur_model = s.default_model.clone();
    v_flex().w_full()
        .child(page_header("对话", "新建会话时使用的默认值,以及对话流的显示方式", div(), cx))
        .child(group("新会话默认值", rows![
            row("默认模型", "新建会话时使用", h_flex().gap_1().children(models.into_iter().enumerate().map(|(i, (id, name))| {
                let active = id == cur_model;
                let id2 = id.clone();
                Button::new(SharedString::from(format!("dm-{i}"))).small().when(active, |b| b.primary()).when(!active, |b| b.ghost()).label(name)
                    .on_click(cx.listener(move |this, _, _, cx| { let v = id2.clone(); this.update_settings(cx, |s| s.default_model = v) }))
            })), cx),
            row("默认权限模式", "也可以在输入框随时切换(⇧Tab)", segmented("dmode", PermissionMode::ALL.iter().map(|m| (*m, m.label())).collect(), s.default_mode, cx,
                |this, v, cx| this.update_settings(cx, |s| s.default_mode = v)), cx),
            row("默认思考强度", "也可以在输入框随时切换(⌥T)", segmented("deff", Effort::ALL.iter().map(|e| (*e, e.label())).collect(), s.default_effort, cx,
                |this, v, cx| this.update_settings(cx, |s| s.default_effort = v)), cx),
        ], cx))
        .child(group("显示", rows![
            row("默认展开思考内容", "关闭时思考折叠为「已思考」一行", Switch::new("sw-think").checked(s.expand_thinking)
                .on_click(cx.listener(move |this, checked: &bool, _, cx| { let v = *checked; this.update_settings(cx, |s| s.expand_thinking = v) })), cx),
        ], cx))
        .into_any_element()
}

/// 快捷键页(只读,按窗口分组)。
pub fn shortcuts(cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    v_flex().w_full()
        .child(page_header("快捷键", "按窗口分组;目前不支持自定义", div(), cx))
        .children(SHORTCUTS.iter().map(|(name, items)| {
            group(name, items.iter().map(|(k, d)| {
                row(d, "", div().px_2().h(px(22.)).flex().items_center().rounded(px(5.)).bg(t.muted).font_family(MONO).text_size(px(12.)).child(k.to_string()), cx).into_any_element()
            }).collect(), cx)
        }))
        .into_any_element()
}

/// 关于页:版本、claude 版本、运行模式、配置文件路径。
pub fn about(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let claude = wb.claude_info.clone().unwrap_or_else(|| if wb.demo { "演示模式(未连接)".into() } else { "检测中…".into() });
    let reveal = wb.settings_path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    v_flex().w_full()
        .child(page_header("关于", "LoomCode:Rust + gpui-kit 的 Claude Code 工作台", pill(format!("v{}", env!("CARGO_PKG_VERSION")), t.foreground, t.muted), cx))
        .child(group("版本", rows![
            row("LoomCode", "只支持 Claude Code,直连本机 claude 的 stream-json 控制协议", div().font_family(MONO).text_size(px(12.5)).child(format!("v{}", env!("CARGO_PKG_VERSION"))), cx),
            row("Claude Code", "本机 claude 命令行", div().font_family(MONO).text_size(px(12.5)).child(claude), cx),
            row("运行模式", "演示模式用脚本化后端与沙盒项目;切换会重启应用", h_flex().gap_2().items_center()
                .child(pill(if wb.demo { "演示" } else { "真实" }, if wb.demo { t.warning } else { t.success }, t.muted))
                .child(Button::new("about-switch-mode").small().label(wb.switch_mode_label())
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(crate::workbench::SwitchMode), cx))), cx),
            row("系统", "", div().font_family(MONO).text_size(px(12.5)).child(format!("{} · {}", std::env::consts::OS, std::env::consts::ARCH)), cx),
        ], cx))
        .child(group("更新", rows![
            row("当前状态", &wb.update_text(), update_buttons(wb, cx), cx),
            switch_row("about-auto-update", "启动时自动检查更新", "发现新版本后自动下载并校验(进度显示在侧栏、状态栏与关于页),重启更新由你点击确认", wb.settings.auto_update_check, cx, |this, v, cx| this.update_settings(cx, |s| s.auto_update_check = v)),
        ], cx))
        .child(group("文件", rows![
            row("配置文件", "客户端偏好;Claude 自己的配置在 ~/.claude", div().font_family(MONO).text_size(px(12.)).text_color(t.muted_foreground).child(wb.settings_path.display().to_string()), cx),
            row("数据目录", "设置、服务商、日志与诊断包所在目录", Button::new("about-reveal").small().ghost().label("在 Finder 中显示")
                .on_click(move |_, _, _| { let _ = std::process::Command::new("open").arg(reveal.clone()).spawn(); }), cx),
        ], cx))
        .child(group("致谢", rows![
            row("gpui / gpui-kit", "界面框架与组件库(Apache-2.0)", div(), cx),
            row("Material Icon Theme", "文件图标,取自 OhMyAgent 的图标集", div(), cx),
        ], cx))
        .into_any_element()
}

/// 更新区右侧按钮:按状态显示「检查更新 / 下载 / 重启更新」,并提供发布页入口。
fn update_buttons(wb: &Workbench, cx: &mut Context<Workbench>) -> Div {
    use crate::updater::UpdateState as U;
    let busy = matches!(wb.update, U::Checking | U::Downloading(..));
    let main = match &wb.update {
        U::Available(_) => Button::new("up-download").small().primary().label("下载更新").on_click(cx.listener(|this, _, _, cx| this.download_update(cx))),
        U::Ready(_) => Button::new("up-install").small().primary().label("重启更新").on_click(cx.listener(|this, _, _, cx| this.install_update(cx))),
        _ => Button::new("up-check").small().outline().label(if busy { "请稍候…" } else { "检查更新" }).disabled(busy).on_click(cx.listener(|this, _, _, cx| this.check_update(cx))),
    };
    h_flex().gap_2().child(main)
        .child(Button::new("up-releases").small().ghost().label("发布页").on_click(|_, _, _| { let _ = std::process::Command::new("open").arg(format!("{}/releases", loom_config::update::REPOSITORY_URL)).spawn(); }))
}
