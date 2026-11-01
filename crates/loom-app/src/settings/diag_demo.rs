//! 演示模式的诊断数据:没有真实崩溃记录时放几条假的,方便截图与检查界面。
//!
//! @author ixxxxoooo

use loom_diag::CrashRecord;

/// 演示用的崩溃与卡顿记录(新在前)。
pub fn fake_records() -> Vec<CrashRecord> {
    let mk = |kind: &str, time: &str, uptime_s: u64, summary: &str, detail: &str| CrashRecord {
        kind: kind.into(),
        time: time.into(),
        pid: 4242,
        run_id: "demo".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        mode: "demo".into(),
        uptime_s,
        summary: summary.into(),
        detail: detail.into(),
        path: Default::default(),
    };
    vec![
        mk("hang", "2026-11-01 20:41:07", 5120, "主线程停滞超过 12 秒", "主线程停滞 12040 ms 仍未恢复。\n调用栈采样文件:~/.loomcode/crashes/hang-20261101-204107-4242.txt\n\n最近的操作(面包屑):\n20:40:51 打开会话 showcase-tour\n20:40:55 打开文件 src/main.rs"),
        mk("ips", "2026-11-01 21:09:44", 1260, "EXC_BAD_ACCESS (SIGSEGV) @ gpui::window::Window::draw", "系统崩溃报告:loomcode-2026-11-01-210944.ips\n崩溃线程:main\n#0 gpui::window::Window::draw\n#1 gpui::app::App::update_window"),
        mk("abnormal-exit", "2026-10-31 18:03:12", 86, "没有崩溃线索:被强制结束(kill -9、强制退出)、内存不足被系统终止或断电", "最后一次心跳距启动 86 秒;最后一次内存采样 412 MB。"),
    ]
}
