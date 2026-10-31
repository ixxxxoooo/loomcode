//! 文件树的图标:按扩展名 / 文件名区分类型,配合主题色(参考 VSCode 的文件图标主题,但只用线性图标)。
//!
//! @author ixxxxoooo

use gpui_kit::assets::IconName;
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;

/// 文件图标与着色。
///
/// 参数:`name` 文件名。返回:(图标, 颜色)。
pub fn file_icon(name: &str, cx: &App) -> (IconName, Hsla) {
    let t = cx.theme();
    let lower = name.to_lowercase();
    let ext = lower.rsplit('.').next().filter(|e| *e != lower).unwrap_or("");
    match (lower.as_str(), ext) {
        ("cargo.lock" | "package-lock.json" | "yarn.lock" | "pnpm-lock.yaml", _) => (IconName::FileLock, t.muted_foreground),
        (".gitignore" | ".gitattributes" | ".gitmodules", _) => (IconName::FileDiff, t.muted_foreground),
        ("claude.md" | "agents.md", _) => (IconName::FileText, t.primary),
        (_, "rs" | "py" | "js" | "mjs" | "ts" | "tsx" | "jsx" | "go" | "swift" | "java" | "c" | "h" | "cpp" | "kt") => (IconName::FileCode, t.info),
        (_, "sh" | "zsh" | "bash") => (IconName::FileTerminal, t.success),
        (_, "json" | "jsonc") => (IconName::FileBraces, t.warning),
        (_, "toml" | "yml" | "yaml" | "ini" | "env") => (IconName::FileCog, t.warning),
        (_, "md" | "markdown" | "txt") => (IconName::FileText, t.muted_foreground),
        (_, "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ico") => (IconName::FileImage, t.success),
        (_, "css" | "html" | "htm" | "scss") => (IconName::FileCode, t.primary),
        (_, "sql" | "db" | "sqlite") => (IconName::Database, t.info),
        _ => (IconName::File, t.muted_foreground),
    }
}
