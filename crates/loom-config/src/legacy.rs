//! 旧版数据迁移:项目由 UCode 更名为 LoomCode 后,把 `~/.ucode` 搬到 `~/.loomcode`。
//!
//! @author ixxxxoooo

use std::path::Path;

/// 迁移结果。
#[derive(Debug, PartialEq, Eq)]
pub enum Migration {
    /// 没有旧目录,或新目录已存在,无需迁移。
    Skipped,
    /// 已把旧目录整体改名为新目录。
    Moved,
    /// 旧目录里有 git worktree(搬走会断开 git 的登记),只复制了配置文件。
    CopiedFiles,
}

/// 启动时调用:按 `HOME` 迁移旧数据目录,失败只打印日志不中断启动。
///
/// 返回值:迁移结果。
pub fn migrate_home() -> Migration {
    let Some(home) = std::env::var_os("HOME") else { return Migration::Skipped };
    let home = Path::new(&home);
    match migrate(&home.join(".ucode"), &home.join(".loomcode")) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("迁移 ~/.ucode 失败:{e}");
            Migration::Skipped
        }
    }
}

/// 把 `old` 迁移到 `new`。
///
/// 参数:`old` 旧目录;`new` 新目录(已存在时不做任何事)。
/// 返回值:迁移结果或 IO 错误。
pub fn migrate(old: &Path, new: &Path) -> std::io::Result<Migration> {
    if !old.is_dir() || new.exists() {
        return Ok(Migration::Skipped);
    }
    if !old.join("worktrees").exists() {
        std::fs::rename(old, new)?;
        return Ok(Migration::Moved);
    }
    std::fs::create_dir_all(new)?;
    for name in ["settings.json", "state.json"] {
        let src = old.join(name);
        if src.is_file() {
            std::fs::copy(&src, new.join(name))?;
        }
    }
    Ok(Migration::CopiedFiles)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 生成独立的临时目录。
    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("loom-legacy-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn moves_whole_dir_and_skips_second_time() {
        let d = tmp("move");
        let (old, new) = (d.join(".ucode"), d.join(".loomcode"));
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("settings.json"), "{}").unwrap();
        assert_eq!(migrate(&old, &new).unwrap(), Migration::Moved);
        assert!(new.join("settings.json").is_file() && !old.exists());
        assert_eq!(migrate(&old, &new).unwrap(), Migration::Skipped);
    }

    #[test]
    fn keeps_worktrees_in_place() {
        let d = tmp("wt");
        let (old, new) = (d.join(".ucode"), d.join(".loomcode"));
        std::fs::create_dir_all(old.join("worktrees")).unwrap();
        std::fs::write(old.join("state.json"), "{}").unwrap();
        assert_eq!(migrate(&old, &new).unwrap(), Migration::CopiedFiles);
        assert!(new.join("state.json").is_file() && old.join("worktrees").is_dir());
    }
}
