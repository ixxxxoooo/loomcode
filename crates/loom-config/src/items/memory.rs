//! 记忆文件清单:按 Claude Code 的加载顺序列出用户级、项目级、本地级 CLAUDE.md 与规则文件。
//!
//! @author ixxxxoooo

use std::path::{Path, PathBuf};

/// 一个记忆文件。
#[derive(Debug, Clone, PartialEq)]
pub struct MemoryFile {
    /// 分组:用户 / 项目 / 本地 / 规则
    pub group: &'static str,
    /// 说明
    pub label: String,
    pub path: PathBuf,
    pub exists: bool,
}

fn file(group: &'static str, label: &str, path: PathBuf) -> MemoryFile {
    MemoryFile { group, label: label.to_string(), exists: path.is_file(), path }
}

/// 某目录下的 `*.md` 规则(按文件名排序)。
fn rules(dir: &Path, group: &'static str) -> Vec<MemoryFile> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "md")).collect();
    files.sort();
    files.into_iter().map(|p| file(group, &format!("规则 {}", p.file_name().unwrap_or_default().to_string_lossy()), p)).collect()
}

/// 列出记忆文件:不存在的标准位置也列出(保存时新建),规则只列已存在的。
///
/// 加载顺序(后加载的优先):用户 `~/.claude/CLAUDE.md` 与 `~/.claude/rules/*.md` →
/// 项目 `CLAUDE.md` / `.claude/CLAUDE.md` 与 `.claude/rules/*.md` → 本地 `CLAUDE.local.md`。
///
/// 参数:`home` 家目录;`project` 当前项目。返回:按加载顺序排列的文件。
pub fn memory_files(home: &Path, project: Option<&Path>) -> Vec<MemoryFile> {
    let mut out = vec![file("用户", "所有项目共用", home.join(".claude/CLAUDE.md"))];
    out.extend(rules(&home.join(".claude/rules"), "用户"));
    if let Some(p) = project {
        out.push(file("项目", "提交到仓库,团队共享", p.join("CLAUDE.md")));
        let alt = p.join(".claude/CLAUDE.md");
        if alt.is_file() {
            out.push(file("项目", ".claude 目录下的写法", alt));
        }
        out.extend(rules(&p.join(".claude/rules"), "项目"));
        out.push(file("本地", "只在本机生效,应加入 .gitignore", p.join("CLAUDE.local.md")));
    }
    out
}
