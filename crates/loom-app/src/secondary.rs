//! 新窗口(⇧⌘N):再起一个本应用进程,各自一个窗口、各看各的项目。
//!
//! 工作台里的编辑器、源代码管理、全局设置都是「一个进程一份」,所以新窗口做成独立进程(`--new-window`),
//! 而不是同进程再建一个工作台。为不和主窗口互相覆盖磁盘上的状态,新窗口的进程是「从属」的:
//! 不写 `state.json` / `settings.json`(只把最近项目合并写回设置)、不记窗口位置、不建托盘图标、
//! 不自动检查更新、不恢复上次会话(同一个会话不能在两个进程里同时跑)。
//!
//! @author ixxxxoooo

use std::sync::OnceLock;

/// 本进程是不是从属窗口进程,以及启动时要打开的项目。
static INFO: OnceLock<(bool, Option<String>)> = OnceLock::new();

/// 解析启动参数:`--new-window` 标记从属窗口,`--project <路径>` 指定要打开的项目。
pub fn parse_args(args: &[String]) -> (bool, Option<String>) {
    let secondary = args.iter().any(|a| a == "--new-window");
    let project = args.iter().position(|a| a == "--project").and_then(|i| args.get(i + 1)).cloned().filter(|p| !p.is_empty());
    (secondary, project)
}

/// 记录本进程的角色(启动时调用一次)。
pub fn init(info: (bool, Option<String>)) {
    let _ = INFO.set(info);
}

/// 是不是从属窗口进程。
pub fn is() -> bool {
    INFO.get().is_some_and(|i| i.0)
}

/// 启动时要打开的项目路径。
pub fn project() -> Option<String> {
    INFO.get().and_then(|i| i.1.clone())
}

/// 新进程的命令行参数:带上运行模式,可选带项目。
pub fn spawn_args(demo: bool, project: Option<&str>) -> Vec<String> {
    let mut a = vec!["--new-window".to_string(), if demo { "--demo" } else { "--real" }.to_string()];
    if let Some(p) = project {
        a.push("--project".into());
        a.push(p.into());
    }
    a
}

/// 起一个新窗口进程;失败返回 false。
pub fn spawn(demo: bool, project: Option<&str>) -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    match std::process::Command::new(exe).args(spawn_args(demo, project)).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn() {
        Ok(mut child) => {
            loom_diag::info!("新窗口已启动(pid {})", child.id());
            // 回收子进程,免得它退出后留成僵尸
            let _ = std::thread::Builder::new().name("loom-new-window".into()).spawn(move || { let _ = child.wait(); });
            true
        }
        Err(e) => {
            loom_diag::error!("打开新窗口失败:{e}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn 解析参数() {
        assert_eq!(parse_args(&v(&["x"])), (false, None));
        assert_eq!(parse_args(&v(&["x", "--new-window"])), (true, None));
        assert_eq!(parse_args(&v(&["x", "--new-window", "--project", "/a b"])), (true, Some("/a b".into())));
        assert_eq!(parse_args(&v(&["x", "--project"])), (false, None));
    }

    #[test]
    fn 新进程参数带模式与项目() {
        assert_eq!(spawn_args(true, None), v(&["--new-window", "--demo"]));
        assert_eq!(spawn_args(false, Some("/p")), v(&["--new-window", "--real", "--project", "/p"]));
    }
}
