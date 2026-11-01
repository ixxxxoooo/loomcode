//! 诊断页:环境信息、进程资源、`claude doctor`、日志查看与导出诊断包。
//!
//! @author ixxxxoooo

use super::widgets::{error_banner, page_title, row, section_title};
use crate::ui::{MONO, caption};
use crate::workbench::Workbench;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// 等宽文本块(可滚动)。
fn mono_block(id: &'static str, text: String, height: f32, cx: &App) -> Stateful<Div> {
    let t = cx.theme();
    div().id(id).w_full().h(px(height)).p_2().rounded(px(8.)).bg(t.muted).overflow_y_scroll()
        .font_family(MONO).text_size(px(11.5)).whitespace_normal().child(text)
}

/// 渲染诊断页。
pub fn render(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let t = cx.theme().clone();
    let ops = &wb.forms().ops;
    let log_path = loom_config::diag::log_path().map(|p| loom_config::diag::redact_home(&p.display().to_string())).unwrap_or_else(|| "未初始化".into());
    let log_dir = loom_config::diag::log_path().and_then(|p| p.parent()).map(|p| p.to_path_buf());
    let doctor = ops.doctor.clone();
    let doctor_busy = ops.doctor_busy;
    v_flex().w_full()
        .child(h_flex().w_full().items_center()
            .child(div().flex_1().child(page_title("诊断")))
            .child(h_flex().gap_2()
                .child(Button::new("diag-refresh").small().outline().label("刷新").on_click(cx.listener(|this, _, _, cx| this.refresh_diag(cx))))
                .child(Button::new("diag-export").small().primary().label("导出诊断包").on_click(cx.listener(|this, _, _, cx| this.export_diagnostics(cx))))))
        .child(caption("诊断包是纯文本,家目录显示为 ~,只含配置概况与变量名,不含密钥和环境变量值;导出后在 Finder 中显示,可以直接发给开发者", cx))
        .children(error_banner(wb, cx))
        .child(section_title("环境", cx))
        .child(row("LoomCode", "", div().font_family(MONO).text_size(px(12.5)).child(format!("v{} · {}", env!("CARGO_PKG_VERSION"), if wb.demo { "演示模式" } else { "真实模式" })), cx))
        .child(row("Claude Code", "", div().font_family(MONO).text_size(px(12.5)).child(wb.claude_info.clone().unwrap_or_else(|| if wb.demo { "演示模式(未连接)".into() } else { "检测中…".into() })), cx))
        .child(section_title("进程资源", cx))
        .child(v_flex().w_full().gap_0p5().children(ops.procs.iter().map(|(pid, mem, cpu, name)| {
            h_flex().w_full().h(px(26.)).px_2().gap_3().items_center().rounded(px(5.)).bg(t.muted).font_family(MONO).text_size(px(12.))
                .child(div().w(px(70.)).child(pid.to_string()))
                .child(div().flex_1().truncate().child(name.clone()))
                .child(div().w(px(90.)).child(format!("{mem:.0} MB")))
                .child(div().w(px(70.)).child(format!("{cpu:.1}%")))
        })))
        .child(h_flex().w_full().items_center().pt_5().pb_1()
            .child(div().flex_1().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(t.muted_foreground).child("claude doctor"))
            .child(Button::new("diag-doctor").small().outline().label(if doctor_busy { "运行中…" } else { "运行" }).disabled(doctor_busy)
                .on_click(cx.listener(|this, _, _, cx| this.run_doctor(cx)))))
        .child(caption("检查安装方式、版本、自动更新与配置问题(最多等待 30 秒)", cx))
        .when_some(doctor, |d, text| d.child(div().pt_2().child(mono_block("diag-doctor-out", text, 200., cx))))
        .child(h_flex().w_full().items_center().pt_5().pb_1()
            .child(div().flex_1().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(t.muted_foreground).child("日志(最近 200 行)"))
            .child(Button::new("diag-open-logs").small().ghost().label("打开日志目录").on_click(move |_, _, _| {
                if let Some(dir) = log_dir.clone() {
                    let _ = std::process::Command::new("open").arg(dir).spawn();
                }
            })))
        .child(caption(log_path, cx))
        .child(div().pt_2().child(mono_block("diag-log", if ops.log.is_empty() { "(暂无日志)".into() } else { ops.log.join("\n") }, 260., cx)))
        .into_any_element()
}
