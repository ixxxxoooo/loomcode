//! 诊断页:环境信息、进程资源、`claude doctor`、日志查看(可复制)与导出诊断包。
//!
//! @author ixxxxoooo

use super::widgets::{Tone, banner, block_row, error_banner, group, item, note, page_header, row, rows, segmented};
use crate::theme::R_MD;
use loom_diag::{Level, frames, hang, monitor, stats};
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


/// 迷你柱状曲线:固定 60 个槽位、靠右排列(新数据在右);高度在 `[下限, 最大值]` 之间等比缩放,
/// 数值全相同时也有一个中等高度,不会撑满。
fn sparkline(values: &[f32], floor: f32, color: Hsla) -> Div {
    let max = values.iter().cloned().fold(floor + 1.0, f32::max);
    h_flex().w_full().h(px(36.)).items_end().justify_end().gap(px(2.))
        .children(values.iter().map(move |v| {
            let ratio = if max - floor > 0. { ((v - floor) / (max - floor)).clamp(0., 1.) } else { 0. };
            div().w(px(6.)).h(px((4. + 32. * ratio).max(1.))).rounded_t(px(1.5)).bg(color)
        }))
}

/// 把采样曲线压成最多 `n` 根柱(每组取最大值,不丢尖峰)。
fn bucketed(samples: &[monitor::Sample], n: usize, f: impl Fn(&monitor::Sample) -> f32) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }
    let size = samples.len().div_ceil(n).max(1);
    samples.chunks(size).map(|c| c.iter().map(&f).fold(0., f32::max)).collect()
}

/// 毫秒显示成「x.x ms」。
fn ms(us: u32) -> String {
    format!("{:.1} ms", us as f64 / 1000.)
}

/// 性能分组:内存 / CPU 曲线、界面构建耗时、最近一次卡顿、实体计数。
fn perf_group(cx: &mut Context<Workbench>) -> Div {
    let t = cx.theme().clone();
    let samples = monitor::samples();
    let last = samples.last().copied().unwrap_or_default();
    let rss_bars = bucketed(&samples, 60, |s| s.rss_mb as f32);
    let rss_floor = rss_bars.iter().cloned().fold(f32::MAX, f32::min) * 0.9;
    let f = frames::summary();
    let stall = match hang::last_stall() {
        Some((d, at)) => format!("{:.1} 秒(约 {} 分钟前)", d as f64 / 1000., stats::now_secs().saturating_sub(at) / 60),
        None => "本次运行没有卡顿".into(),
    };
    let gauges = stats::gauges();
    let gauge_text = if gauges.is_empty() { "统计中…".to_string() } else { gauges.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(" · ") };
    let mono = |s: String| div().font_family(MONO).text_size(px(12.5)).child(s);
    group("性能(最近 5 分钟)", rows![
        row("内存", "本进程常驻内存", mono(format!("{} MB", last.rss_mb)), cx),
        block_row("内存曲线", "", sparkline(&rss_bars, rss_floor, t.primary.opacity(0.7)), cx),
        row("CPU", "100% = 一个核心满载", mono(format!("{:.0}%", last.cpu)), cx),
        block_row("CPU 曲线", "", sparkline(&bucketed(&samples, 60, |s| s.cpu), 0., t.warning.opacity(0.7)), cx),
        row("界面构建耗时", &format!("根视图 render 的耗时(最近 {} 次)", f.count), mono(format!("p50 {} · p95 {} · 最大 {}", ms(f.p50_us), ms(f.p95_us), ms(f.max_us))), cx),
        row("最近一次卡顿", "界面线程停滞超过 3 秒才会记录", mono(stall), cx),
        row("实体计数", "项目、会话、时间线条目、终端", mono(gauge_text), cx),
    ], cx)
}

/// 崩溃与卡顿分组:记录列表(可展开、可复制)与打开目录。
fn crash_group(wb: &Workbench, cx: &mut Context<Workbench>) -> Div {
    let ops = &wb.forms().ops;
    let t = cx.theme().clone();
    let dir = loom_diag::dirs().map(|d| d.crashes.clone());
    let mut rows_v: Vec<AnyElement> = vec![row("记录目录", "panic、原生崩溃、卡死、异常退出与系统崩溃报告摘要", Button::new("diag-open-crashes").small().ghost().label("打开目录").on_click(move |_, _, _| {
        if let Some(d) = dir.clone() {
            let _ = std::process::Command::new("open").arg(d).spawn();
        }
    }), cx).into_any_element()];
    if ops.crashes.is_empty() {
        rows_v.push(item(cx).h(px(40.)).child(caption("没有崩溃或卡顿记录", cx)).into_any_element());
    }
    for (i, r) in ops.crashes.iter().enumerate() {
        let open = ops.crash_open == Some(i);
        let detail = r.detail.clone();
        let head = item(cx).h(px(36.)).gap_3()
            .child(div().w(px(150.)).flex_none().whitespace_nowrap().font_family(MONO).text_size(px(11.5)).text_color(t.muted_foreground).child(r.time.clone()))
            .child(div().w(px(84.)).flex_none().text_size(px(12.)).font_weight(FontWeight::MEDIUM).child(r.kind_label()))
            .child(div().flex_1().min_w_0().truncate().text_size(px(12.)).child(r.summary.clone()))
            .child(Button::new(SharedString::from(format!("crash-toggle-{i}"))).small().ghost().label(if open { "收起" } else { "详情" })
                .on_click(cx.listener(move |this, _, _, cx| {
                    let ops = &mut this.forms_mut().ops;
                    ops.crash_open = if ops.crash_open == Some(i) { None } else { Some(i) };
                    cx.notify();
                })))
            .child(copy_button_dyn(SharedString::from(format!("crash-copy-{i}")), format!("[{}] {} v{} {}\n{}\n{}", r.kind_label(), r.time, r.version, r.mode, r.summary, detail), cx));
        rows_v.push(if open {
            v_flex().w_full().child(head).child(div().px(px(14.)).pb_2().child(mono_block("crash-detail", r.detail.clone(), 220., cx))).into_any_element()
        } else {
            head.into_any_element()
        });
    }
    group("崩溃与卡顿", rows_v, cx)
}

/// 同 [`copy_button`],id 可以是运行时生成的。
fn copy_button_dyn(id: SharedString, text: String, cx: &mut Context<Workbench>) -> Button {
    Button::new(id).small().ghost().label("复制").on_click(cx.listener(move |_, _, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(text.clone()))))
}

/// 按级别过滤日志行(0 全部 / 1 警告及以上 / 2 仅错误)。
fn filter_log(lines: &[String], filter: u8) -> Vec<&String> {
    lines.iter().filter(|l| match filter {
        1 => l.contains("[WARN]") || l.contains("[ERROR]"),
        2 => l.contains("[ERROR]"),
        _ => true,
    }).collect()
}

/// 渲染诊断页。
pub fn render(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let ops = &wb.forms().ops;
    let log_path = loom_config::diag::log_path().map(|p| loom_config::diag::redact_home(&p.display().to_string())).unwrap_or_else(|| "未初始化".into());
    let log_dir = loom_config::diag::log_path().and_then(|p| p.parent()).map(|p| p.to_path_buf());
    let doctor = ops.doctor.clone();
    let doctor_busy = ops.doctor_busy;
    let shown = filter_log(&ops.log, ops.log_filter);
    let log_text = if shown.is_empty() { "(暂无日志)".to_string() } else { shown.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n") };
    let newest = ops.crashes.first().cloned();
    let today = loom_diag::timestamp().chars().take(10).collect::<String>();
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
        .child(page_header("诊断", "性能、崩溃记录、进程资源与日志;出问题时导出诊断包发给开发者", h_flex().gap_2()
            .child(Button::new("diag-refresh").small().outline().label("刷新").on_click(cx.listener(|this, _, _, cx| this.refresh_diag(cx))))
            .child(Button::new("diag-export").small().primary().label("导出诊断包").on_click(cx.listener(|this, _, _, cx| this.export_diagnostics(cx)))), cx))
        .children(error_banner(wb, cx))
        .children(newest.map(|r| banner(if r.time.starts_with(&today) { Tone::Danger } else { Tone::Info }, format!("最近一次异常:{} · {} · {}", r.time, r.kind_label(), r.summary), cx)))
        .child(group("环境", rows![
            row("LoomCode", "", div().font_family(MONO).text_size(px(12.5)).child(format!("v{} · {}", env!("CARGO_PKG_VERSION"), if wb.demo { "演示模式" } else { "真实模式" })), cx),
            row("Claude Code", "", div().font_family(MONO).text_size(px(12.5)).child(claude), cx),
        ], cx))
        .child(perf_group(cx))
        .child(crash_group(wb, cx))
        .child(group("进程资源", proc_rows, cx))
        .child(group("claude doctor", doctor_rows, cx))
        .child(group("日志(最近 200 行)", rows![
            row("日志级别", "重启前后都生效;环境变量 LOOM_LOG 可临时覆盖。单个文件 5 MB,保留 5 份", segmented("log-level", Level::ALL.iter().map(|l| (*l, l.label())).collect(), loom_diag::level(), cx,
                |this, l, cx| this.update_settings(cx, |s| s.log_level = l.key().into())), cx),
            row("显示过滤", "只影响下方显示与复制,不影响写入", segmented("log-filter", vec![(0u8, "全部"), (1, "警告及以上"), (2, "仅错误")], ops.log_filter, cx,
                |this, v, cx| { this.forms_mut().ops.log_filter = v; cx.notify(); }), cx),
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
