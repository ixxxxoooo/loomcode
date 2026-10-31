//! 按文件扩展名推断语法高亮语言。
//!
//! @author ixxxxoooo

use std::path::{Path, PathBuf};

/// 按扩展名推断语法高亮语言。
pub fn language_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "rs" => "rust",
        "py" => "python",
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "ts" => "typescript",
        "tsx" => "tsx",
        "sh" | "zsh" | "bash" => "bash",
        "md" | "markdown" => "markdown",
        "toml" => "toml",
        "yml" | "yaml" => "yaml",
        "json" | "jsonc" => "json",
        "css" => "css",
        "html" | "htm" => "html",
        "go" => "go",
        "diff" | "patch" => "diff",
        "swift" => "swift",
        _ => "text",
    }
}

/// 目录里的一项。
#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}


/// 不在树里显示的目录与文件。
fn hidden(name: &str) -> bool {
    matches!(name, ".git" | ".DS_Store" | "target" | "node_modules" | ".build" | "DerivedData")
}

/// 读取一个目录:文件夹在前,按名称排序。
pub fn read_dir_sorted(dir: &Path) -> Vec<Node> {
    let mut nodes: Vec<Node> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    if hidden(&name) {
                        return None;
                    }
                    let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                    Some(Node { name, path: e.path(), is_dir })
                })
                .collect()
        })
        .unwrap_or_default();
    nodes.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    nodes
}

