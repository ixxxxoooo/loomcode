//! 演示 ↔ 真实一键切换:弹确认框,记住选择(只有开发版),再用新模式重启自己,始终只留一个实例。
//!
//! 启动时的模式优先级:显式指定(`--demo` / `--real` / `LOOM_DEMO=1|0`)> 开发版记住的上次选择 > 真实。
//! 记忆文件是 `~/.loomcode/dev-mode`(内容 `demo` 或 `real`),只在开发版(`LOOM_DEV=1`)读写,
//! 不放进 `state.json`——那份在演示模式下落在沙盒里,切回真实后就读不到了。
//!
//! 重启靠一个脱离当前进程的小 shell:等旧进程退出,写回 `target/dev.pid`,再 `exec` 新实例。
//!
//! @author ixxxxoooo

use crate::workbench::Workbench;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::*;
use std::path::{Path, PathBuf};

/// 记忆文件路径:`<home>/.loomcode/dev-mode`。
pub fn memory_path(home: &Path) -> PathBuf {
    home.join(".loomcode").join("dev-mode")
}

/// 读取记住的模式:`Some(true)` 演示,`Some(false)` 真实,没有记忆或内容无法识别返回 `None`。
pub fn read_memory(home: &Path) -> Option<bool> {
    match std::fs::read_to_string(memory_path(home)).ok()?.trim() {
        "demo" => Some(true),
        "real" => Some(false),
        _ => None,
    }
}

/// 记住模式(失败静默:记忆只是便利,不能因为写不了文件影响切换)。
pub fn write_memory(home: &Path, demo: bool) {
    let path = memory_path(home);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, if demo { "demo\n" } else { "real\n" });
}

/// 决定启动模式。
///
/// 参数:`args` 命令行参数;`env_demo` 环境变量 `LOOM_DEMO` 的值;`is_dev` 是否开发版;`memory` 记住的上次选择。
/// 返回:是否演示模式。显式指定优先;`--real` / `LOOM_DEMO=0` 能压过记忆(`--demo` 与 `--real` 同时出现时以 `--real` 为准)。
pub fn resolve_demo(args: &[String], env_demo: Option<&str>, is_dev: bool, memory: Option<bool>) -> bool {
    let has = |f: &str| args.iter().any(|a| a == f);
    if has("--real") || env_demo == Some("0") {
        return false;
    }
    if has("--demo") || env_demo == Some("1") {
        return true;
    }
    if is_dev { memory.unwrap_or(false) } else { false }
}

/// 重启时传给新进程的参数:去掉旧的模式开关,按目标模式补上 `--demo`(真实模式不带任何开关,
/// 由记忆文件或默认值决定;开发版已经把目标写进记忆,正式版默认就是真实)。
pub fn relaunch_args(args: &[String], to_demo: bool) -> Vec<String> {
    let mut out: Vec<String> = args.iter().skip(1).filter(|a| *a != "--demo" && *a != "--real").cloned().collect();
    if to_demo {
        out.push("--demo".into());
    }
    out
}

/// 由可执行文件路径推出开发版的 pid 文件:`<工程>/target/debug/loomcode` → `<工程>/target/dev.pid`。
/// 不是这种布局(例如打包版)返回 `None`。
pub fn pid_file_for(exe: &Path) -> Option<PathBuf> {
    let debug = exe.parent()?;
    let target = debug.parent()?;
    (debug.file_name()? == "debug" && target.file_name()? == "target").then(|| target.join("dev.pid"))
}

/// 重启脚本:等旧进程退出 → 可选写 pid → `exec` 新实例(日志追加到 `LOOM_RELAUNCH_LOG`)。
pub const RELAUNCH_SCRIPT: &str = r#"sleep 0.6; if [ -n "$LOOM_RELAUNCH_PID" ]; then echo $$ > "$LOOM_RELAUNCH_PID"; fi; exec "$@" >>"${LOOM_RELAUNCH_LOG:-/dev/null}" 2>&1"#;

/// 启动一个脱离当前进程的重启 shell;调用方随后退出应用。返回是否启动成功。
fn spawn_relaunch(to_demo: bool) -> bool {
    use std::os::unix::process::CommandExt;
    let Ok(exe) = std::env::current_exe() else { return false };
    let args: Vec<String> = std::env::args().collect();
    let new_args = relaunch_args(&args, to_demo);
    let mut cmd = std::process::Command::new("sh");
    cmd.arg("-c").arg(RELAUNCH_SCRIPT).arg("sh").arg(&exe).args(&new_args);
    // 旧进程的模式 / 会话环境不能带过去,否则新实例又按旧的来
    cmd.env_remove("LOOM_DEMO").env_remove("LOOM_SESSION").env_remove("LOOM_AUTOSWITCH");
    // 开发版:把 pid 写回 target/dev.pid,日志续写到 target/dev.log,和 scripts/dev.sh 保持一致
    if std::env::var("LOOM_DEV").is_ok_and(|v| v == "1") && let Some(pid_file) = pid_file_for(&exe) {
        cmd.env("LOOM_RELAUNCH_LOG", pid_file.with_file_name("dev.log"));
        cmd.env("LOOM_RELAUNCH_PID", &pid_file);
    }
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    cmd.process_group(0);
    cmd.spawn().is_ok()
}

impl Workbench {
    /// 切换后的目标模式文案。
    pub fn switch_mode_label(&self) -> &'static str {
        if self.demo { "切换到真实模式" } else { "切换到演示模式" }
    }

    /// 弹确认框,确认后切换并重启。
    pub fn request_switch_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (wb, to_demo) = (cx.entity().downgrade(), !self.demo);
        let (title, body) = if to_demo {
            ("切换到演示模式?", "将重启 LoomCode,换成脚本化后端与沙盒项目,不会读取或修改你的真实会话。未保存的编辑与输入框草稿会丢失。")
        } else {
            ("切换到真实模式?", "将重启 LoomCode,连接本机 claude 并读取真实的项目与会话。未保存的编辑与输入框草稿会丢失。")
        };
        window.open_dialog(cx, move |dialog, _, _| {
            let wb = wb.clone();
            let footer = DialogFooter::new()
                .child(Button::new("mode-cancel").label("取消").on_click(|_, window, cx| window.close_all_dialogs(cx)))
                .child(Button::new("mode-ok").primary().label("切换并重启").on_click(move |_, window, cx| {
                    window.close_all_dialogs(cx);
                    let _ = wb.update(cx, |this, cx| this.switch_mode_now(to_demo, cx));
                }));
            dialog.title(title).child(body).footer(footer)
        });
    }

    /// 立即切换:记住选择(开发版)、启动重启 shell、退出当前进程。
    pub fn switch_mode_now(&mut self, to_demo: bool, cx: &mut Context<Self>) {
        let is_dev = std::env::var("LOOM_DEV").is_ok_and(|v| v == "1");
        if is_dev && let Some(home) = std::env::var_os("HOME") {
            write_memory(Path::new(&home), to_demo);
        }
        loom_config::diag::info(&format!("切换模式并重启:→ {}", if to_demo { "演示" } else { "真实" }));
        if spawn_relaunch(to_demo) {
            cx.quit();
        } else {
            loom_config::diag::error("切换模式失败:无法启动新进程");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[::core::prelude::v1::test]
    fn 启动模式优先级() {
        // 没有任何指定:正式版永远真实,开发版看记忆
        assert!(!resolve_demo(&v(&["x"]), None, false, Some(true)));
        assert!(resolve_demo(&v(&["x"]), None, true, Some(true)));
        assert!(!resolve_demo(&v(&["x"]), None, true, Some(false)));
        assert!(!resolve_demo(&v(&["x"]), None, true, None));
        // 显式指定压过记忆
        assert!(resolve_demo(&v(&["x", "--demo"]), None, true, Some(false)));
        assert!(resolve_demo(&v(&["x"]), Some("1"), false, None));
        assert!(!resolve_demo(&v(&["x", "--real"]), None, true, Some(true)));
        assert!(!resolve_demo(&v(&["x"]), Some("0"), true, Some(true)));
        // 同时出现时 --real 说了算
        assert!(!resolve_demo(&v(&["x", "--demo", "--real"]), None, true, None));
    }

    #[::core::prelude::v1::test]
    fn 重启参数去掉旧开关再按目标补() {
        assert_eq!(relaunch_args(&v(&["loomcode", "--demo", "--foo"]), false), v(&["--foo"]));
        assert_eq!(relaunch_args(&v(&["loomcode", "--foo"]), true), v(&["--foo", "--demo"]));
        assert_eq!(relaunch_args(&v(&["loomcode", "--real", "--demo"]), true), v(&["--demo"]));
    }

    #[::core::prelude::v1::test]
    fn 记忆读写往返() {
        let dir = std::env::temp_dir().join(format!("loom-mode-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(read_memory(&dir), None);
        write_memory(&dir, true);
        assert_eq!(read_memory(&dir), Some(true));
        write_memory(&dir, false);
        assert_eq!(read_memory(&dir), Some(false));
        std::fs::write(memory_path(&dir), "乱码").unwrap();
        assert_eq!(read_memory(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[::core::prelude::v1::test]
    fn pid_文件只认开发版布局() {
        assert_eq!(pid_file_for(Path::new("/w/UCode/target/debug/loomcode")), Some(PathBuf::from("/w/UCode/target/dev.pid")));
        assert_eq!(pid_file_for(Path::new("/Applications/LoomCode.app/Contents/MacOS/loomcode")), None);
        assert_eq!(pid_file_for(Path::new("/w/target/release/loomcode")), None);
    }
}
