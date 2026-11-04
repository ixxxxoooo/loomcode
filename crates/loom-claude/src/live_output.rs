//! 前台 Bash 的实时输出文件定位。
//!
//! claude 的 stream-json 在前台命令运行期间不给输出(只在结束时一次性给),但它把输出实时写进
//! `<临时目录>/claude-<uid>/<编码后的 cwd>/<会话 id>/tasks/<任务 id>.output`(实测 2.1.318)。
//! 这里按这条规则找文件;规则对不上时(符号链接、会话 id 变化)退回扫描 `claude-<uid>/*/*/tasks/`。

use std::path::{Path, PathBuf};

/// claude 的临时根目录:`$CLAUDE_CODE_TMPDIR` 或 `/tmp`,下面是 `claude-<uid>`。
fn tmp_root() -> PathBuf {
    let base = std::env::var("CLAUDE_CODE_TMPDIR").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| "/tmp".into());
    // SAFETY: getuid 没有前置条件,不会失败
    let uid = unsafe { libc::getuid() };
    Path::new(&base).join(format!("claude-{uid}"))
}

/// 项目目录名:cwd 里所有非字母数字字符换成 `-`(与历史目录同一规则)。
fn encode_cwd(cwd: &str) -> String {
    cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

/// 找前台任务的输出文件;文件还没创建时返回 `None`(下次轮询再试)。
///
/// 参数:`cwd` 会话工作目录;`session_id` 会话 id;`task_id` 任务 id。
pub fn find_output_file(cwd: &str, session_id: &str, task_id: &str) -> Option<String> {
    find_in(&tmp_root(), cwd, session_id, task_id)
}

/// 在指定的 `claude-<uid>` 根目录下按规则找,再退回扫描。
fn find_in(root: &Path, cwd: &str, session_id: &str, task_id: &str) -> Option<String> {
    if task_id.is_empty() || task_id.contains('/') {
        return None;
    }
    let file = format!("{task_id}.output");
    if !session_id.is_empty() {
        let p = root.join(encode_cwd(cwd)).join(session_id).join("tasks").join(&file);
        if p.is_file() {
            return Some(p.to_string_lossy().into_owned());
        }
    }
    for proj in std::fs::read_dir(root).ok()?.flatten() {
        let Ok(sessions) = std::fs::read_dir(proj.path()) else { continue };
        for s in sessions.flatten() {
            let p = s.path().join("tasks").join(&file);
            if p.is_file() {
                return Some(p.to_string_lossy().into_owned());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 项目目录编码与历史目录一致() {
        assert_eq!(encode_cwd("/Users/a b/loom.code"), "-Users-a-b-loom-code");
    }

    #[test]
    fn 先按规则找再退回扫描() {
        let root = std::env::temp_dir().join(format!("loom-live-{}", std::process::id()));
        let dir = root.join("-Users-x-proj").join("sess1").join("tasks");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("t1.output"), "hi").unwrap();
        let want = dir.join("t1.output").to_string_lossy().into_owned();
        // 规则命中
        assert_eq!(find_in(&root, "/Users/x/proj", "sess1", "t1"), Some(want.clone()));
        // 会话 id 对不上:扫描兜底
        assert_eq!(find_in(&root, "/Users/x/proj", "别的会话", "t1"), Some(want));
        // 文件不存在 / 非法 id
        assert_eq!(find_in(&root, "/Users/x/proj", "sess1", "nope"), None);
        assert_eq!(find_in(&root, "/Users/x/proj", "sess1", "../x"), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
