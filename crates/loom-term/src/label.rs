//! 终端标签名(参考 VS Code 的「进程名 · 目录」):前台进程名加当前目录的最后一段,
//! 例如 `zsh · router-demo`;命令运行时变成 `cargo · router-demo`。
//!
//! @author ixxxxoooo

use std::process::Command;

/// 取进程的可执行文件名(去掉路径与登录 shell 的前导 `-`)。
///
/// 参数:`pid` 进程号。返回:拿不到时为 `None`。
pub fn process_name(pid: i32) -> Option<String> {
    let out = Command::new("ps").args(["-o", "comm=", "-p", &pid.to_string()]).output().ok()?;
    let comm = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!comm.is_empty()).then(|| base_name(&comm))
}

/// `/bin/zsh`、`-zsh` → `zsh`。
pub fn base_name(comm: &str) -> String {
    comm.rsplit('/').next().unwrap_or(comm).trim_start_matches('-').to_string()
}

/// 从 shell 设置的标题(常见为 `用户@主机:路径`)里取出路径的最后一段;不是这种格式返回 `None`。
///
/// `user@host:~/a/b` → `b`;`user@host:~` → `~`;`user@host:/` → `/`。
pub fn folder_from_title(title: &str) -> Option<String> {
    let (head, path) = title.split_once(':')?;
    if !head.contains('@') || head.contains(' ') {
        return None;
    }
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return Some("/".into());
    }
    trimmed.rsplit('/').next().map(str::to_string)
}

/// 拼标签名:`进程 · 目录`;没有进程名时只显示目录。
///
/// 参数:`process` 前台进程名;`title` 终端标题;`fallback_folder` 标题里取不到目录时用的目录名(项目目录名)。
pub fn tab_label(process: Option<&str>, title: &str, fallback_folder: &str) -> String {
    let folder = folder_from_title(title).unwrap_or_else(|| fallback_folder.to_string());
    match process.filter(|p| !p.is_empty()) {
        Some(p) => format!("{p} · {folder}"),
        None => folder,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 进程名去路径与登录前缀() {
        assert_eq!(base_name("/bin/zsh"), "zsh");
        assert_eq!(base_name("-zsh"), "zsh");
        assert_eq!(base_name("cargo"), "cargo");
    }

    #[test]
    fn 从标题取目录() {
        assert_eq!(folder_from_title("liwenjiao@liwenjiaodeMac:~/UCode/router-demo").as_deref(), Some("router-demo"));
        assert_eq!(folder_from_title("me@host:~").as_deref(), Some("~"));
        assert_eq!(folder_from_title("me@host:/").as_deref(), Some("/"));
        assert_eq!(folder_from_title("me@host:/tmp/x/").as_deref(), Some("x"));
        assert_eq!(folder_from_title("cargo test"), None);
        assert_eq!(folder_from_title("vim: notes"), None);
        assert_eq!(folder_from_title(""), None);
    }

    #[test]
    fn 拼标签() {
        assert_eq!(tab_label(Some("zsh"), "me@h:~/a/proj", "root"), "zsh · proj");
        assert_eq!(tab_label(Some("cargo"), "cargo test", "root"), "cargo · root");
        assert_eq!(tab_label(None, "", "root"), "root");
    }

    #[test]
    fn 取得自己的进程名() {
        let name = process_name(std::process::id() as i32).expect("ps 应能取到当前进程");
        assert!(!name.is_empty());
    }
}
