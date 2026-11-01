//! 诊断页:环境信息、进程资源、`claude doctor`、日志查看(可复制)与导出诊断包。
//!
//! @author ixxxxoooo

use super::widgets::{block_row, error_banner, group, item, note, page_header, row, rows};
use crate::theme::R_MD;
use crate::ui::{MONO, caption};
use crate::workbench::Workbench;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::*;

/// 等宽文本块(可滚动)。
fn mono_block(id: &'static str, text: String, height: f32, cx: &App) -> Stateful<Div> {
    let t = cx.theme();
    div().id(id).w_full().h(px(height)).p_2().rounded(px(R_MD)).bg(t.muted).overflow_y_scroll()
        .font_family(MONO).text_size(px(11.5)).whitespace_normal().child(text)
}

/// 文本块上方的操作行:说明 + 复制按钮。
fn copy_button(id: &'static str, text: String, cx: &mut Context<Workbench>) -> Button {
    Button::new(id).small().ghost().label("复制").on_click(cx.listener(move |_, _, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(text.clone()))))
}

/// 渲染诊断页。
pub fn render(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let ops = &wb.forms().ops;
    let log_path = loom_config::diag::log_path().map(|p| loom_config::diag::redact_home(&p.display().to_string())).unwrap_or_else(|| "未初始化".into());
    let log_dir = loom_config::diag::log_path().and_then(|p| p.parent()).map(|p| p.to_path_buf());
    let doctor = ops.doctor.clone();
    let doctor_busy = ops.doctor_busy;
    let log_text = if ops.log.is_empty() { "(暂无日志)".to_string() } else { ops.log.join("\n") };
    let claude = wb.claude_info.clone().unwrap_or_else(|| if wb.demo { "演示模式(未连接)".into() } else { "检测中…".into() });
    let proc_rows: Vec<AnyElement> = if ops.procs.is_empty() {
        rows![item(cx).h(px(40.)).child(caption("没有检测到 LoomCode 或 claude 进程", cx))]
    } else {
        ops.procs.iter().map(|(pid, mem, cpu, name)| {
            item(cx).h(px(32.)).gap_3().font_family(MONO).text_size(px(12.))
                .child(div().w(px(70.)).child(pid.to_string()))
                .child(div().flex_1().truncate().child(name.clone()))
                .child(div().w(px(90.)).child(format!("{mem:.0} MB")))
                .child(div().w(px(70.)).child(format!("{cpu:.1}%")))
                .into_any_element()
        }).collect()
    };
    let mut doctor_rows = rows![
        row("运行 claude doctor", "检查安装方式、版本、自动更新与配置问题(最多等待 30 秒)", Button::new("diag-doctor").small().outline().label(if doctor_busy { "运行中…" } else { "运行" }).disabled(doctor_busy)
            .on_click(cx.listener(|this, _, _, cx| this.run_doctor(cx))), cx),
    ];
    if let Some(text) = doctor {
        doctor_rows.push(block_row("输出", "", v_flex().w_full().gap_1()
            .child(h_flex().w_full().justify_end().child(copy_button("copy-doctor", text.clone(), cx)))
            .child(mono_block("diag-doctor-out", text, 200., cx)), cx).into_any_element());
    }
    v_flex().w_full()
        .child(page_header("诊断", "环境信息、进程资源与日志;出问题时导出诊断包发给开发者", h_flex().gap_2()
            .child(Button::new("diag-refresh").small().outline().label("刷新").on_click(cx.listener(|this, _, _, cx| this.refresh_diag(cx))))
            .child(Button::new("diag-export").small().primary().label("导出诊断包").on_click(cx.listener(|this, _, _, cx| this.export_diagnostics(cx)))), cx))
        .children(error_banner(wb, cx))
        .child(group("环境", rows![
            row("LoomCode", "", div().font_family(MONO).text_size(px(12.5)).child(format!("v{} · {}", env!("CARGO_PKG_VERSION"), if wb.demo { "演示模式" } else { "真实模式" })), cx),
            row("Claude Code", "", div().font_family(MONO).text_size(px(12.5)).child(claude), cx),
        ], cx))
        .child(group("进程资源", proc_rows, cx))
        .child(group("claude doctor", doctor_rows, cx))
        .child(group("日志(最近 200 行)", rows![
            row("日志文件", &log_path, h_flex().gap_1()
                .child(copy_button("copy-log", log_text.clone(), cx))
                .child(Button::new("diag-open-logs").small().ghost().label("打开日志目录").on_click(move |_, _, _| {
                    if let Some(dir) = log_dir.clone() {
                        let _ = std::process::Command::new("open").arg(dir).spawn();
                    }
                })), cx),
            block_row("内容", "", mono_block("diag-log", log_text, 260., cx), cx),
        ], cx))
        .child(note("诊断包是纯文本,家目录显示为 ~,只含配置概况与变量名,不含密钥和环境变量值;导出后在 Finder 中显示", cx))
        .into_any_element()
}
