//! 运行标记:启动时写 `~/.loomcode/run/<pid>.json`,正常退出时删除。
//! 下次启动若发现标记还在、进程却已不存在,就说明上次是异常退出(崩溃、被杀、卡死后强退)。
//!
//! @author ixxxxoooo

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 运行标记的内容。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Marker {
    pub pid: u32,
    pub run_id: String,
    /// 启动时间(Unix 秒)
    pub started: u64,
    pub version: String,
    /// demo / real
    pub mode: String,
    /// 最近一次心跳(Unix 秒)
    pub heartbeat: u64,
    /// 最近一次采样的常驻内存(MB)
    pub rss_mb: u64,
}

/// 标记文件路径。
pub fn marker_path(run_dir: &Path, pid: u32) -> PathBuf {
    run_dir.join(format!("{pid}.json"))
}

/// 原生崩溃处理器预先打开的记录文件(崩溃时往里写固定文本)。
pub fn native_path(run_dir: &Path, pid: u32) -> PathBuf {
    run_dir.join(format!("{pid}.native"))
}

/// 原子写入标记(先写临时文件再改名)。
pub fn write(run_dir: &Path, m: &Marker) {
    let path = marker_path(run_dir, m.pid);
    let tmp = path.with_extension("json.tmp");
    if let Ok(text) = serde_json::to_string(m)
        && std::fs::write(&tmp, text).is_ok()
    {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// 进程是否还活着(`kill(pid, 0)`;没有权限发信号也说明它存在)。
pub fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    // SAFETY: 信号 0 只做存在性检查,不会向进程投递任何信号。
    let r = unsafe { libc::kill(pid as libc::pid_t, 0) };
    r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// 找出进程已不存在的残留标记(`my_pid` 自己的不算)。
pub fn scan_dead(run_dir: &Path, my_pid: u32) -> Vec<Marker> {
    let Ok(rd) = std::fs::read_dir(run_dir) else { return Vec::new() };
    rd.filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .filter_map(|p| serde_json::from_str::<Marker>(&std::fs::read_to_string(p).ok()?).ok())
        .filter(|m| m.pid != my_pid && !pid_alive(m.pid))
        .collect()
}

/// 删除标记及其配套的原生崩溃记录文件。
pub fn remove(run_dir: &Path, pid: u32) {
    let _ = std::fs::remove_file(marker_path(run_dir, pid));
    let _ = std::fs::remove_file(native_path(run_dir, pid));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker(pid: u32) -> Marker {
        Marker { pid, run_id: "r".into(), started: 1, version: "0".into(), mode: "real".into(), heartbeat: 2, rss_mb: 3 }
    }

    #[test]
    fn 自己存活自己不算残留() {
        let dir = std::env::temp_dir().join(format!("loom-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let me = std::process::id();
        write(&dir, &marker(me));
        assert!(pid_alive(me));
        assert!(scan_dead(&dir, me).is_empty());
        // 另一个调用方视角:存活进程的标记也不算残留
        assert!(scan_dead(&dir, me + 1).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn 已死进程的标记被发现并可清理() {
        let dir = std::env::temp_dir().join(format!("loom-run-dead-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // 起一个立刻结束的子进程,拿到一个确定已不存在的 pid
        let mut c = std::process::Command::new("/usr/bin/true").spawn().unwrap();
        let pid = c.id();
        c.wait().unwrap();
        write(&dir, &marker(pid));
        std::fs::write(native_path(&dir, pid), "x").unwrap();
        let dead = scan_dead(&dir, std::process::id());
        assert_eq!(dead.len(), 1);
        assert_eq!(dead[0].pid, pid);
        remove(&dir, pid);
        assert!(scan_dead(&dir, std::process::id()).is_empty());
        assert!(!native_path(&dir, pid).exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
