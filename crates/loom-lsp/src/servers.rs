//! 语言服务器注册表:按文件扩展名选服务器,在 PATH 与常见目录里找可执行文件。

use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// 一种语言服务器的启动方式。
pub struct ServerSpec {
    /// 稳定标识(也是复用键的一部分)
    pub id: &'static str,
    /// 界面显示名
    pub label: &'static str,
    /// 可执行文件名
    pub cmd: &'static str,
    pub args: &'static [&'static str],
    /// 未安装时给用户的安装提示
    pub install: &'static str,
    /// 这个服务器管理的扩展名
    pub exts: &'static [&'static str],
}

pub const SERVERS: &[ServerSpec] = &[
    ServerSpec { id: "rust-analyzer", label: "rust-analyzer", cmd: "rust-analyzer", args: &[], install: "rustup component add rust-analyzer", exts: &["rs"] },
    ServerSpec { id: "pyright", label: "Pyright", cmd: "pyright-langserver", args: &["--stdio"], install: "npm i -g pyright", exts: &["py", "pyi"] },
    ServerSpec { id: "gopls", label: "gopls", cmd: "gopls", args: &[], install: "go install golang.org/x/tools/gopls@latest", exts: &["go"] },
    ServerSpec { id: "sourcekit-lsp", label: "SourceKit-LSP", cmd: "sourcekit-lsp", args: &[], install: "随 Xcode / Swift 工具链提供", exts: &["swift"] },
    ServerSpec { id: "clangd", label: "clangd", cmd: "clangd", args: &[], install: "brew install llvm,或随 Xcode 命令行工具提供", exts: &["c", "h", "cc", "cpp", "cxx", "hpp", "m", "mm"] },
    ServerSpec { id: "typescript", label: "TypeScript", cmd: "typescript-language-server", args: &["--stdio"], install: "npm i -g typescript typescript-language-server", exts: &["ts", "tsx", "js", "jsx", "mjs", "cjs"] },
];

/// 按文件扩展名选语言服务器。
pub fn spec_for_path(path: &Path) -> Option<&'static ServerSpec> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    SERVERS.iter().find(|s| s.exts.contains(&ext.as_str()))
}

/// LSP 的 `languageId`(按扩展名)。
pub fn language_id(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "rs" => "rust",
        "py" | "pyi" => "python",
        "go" => "go",
        "swift" => "swift",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" => "cpp",
        "m" => "objective-c",
        "mm" => "objective-cpp",
        "ts" => "typescript",
        "tsx" => "typescriptreact",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "javascriptreact",
        _ => "plaintext",
    }
}

/// 图形应用的 PATH 很短,补上常见的工具安装目录。
fn base_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for d in [".cargo/bin", "go/bin", ".local/bin", ".bun/bin"] {
            dirs.push(home.join(d));
        }
        // nvm 装的 node 全局包(pyright、typescript-language-server)
        if let Ok(rd) = std::fs::read_dir(home.join(".nvm/versions/node")) {
            let mut vs: Vec<PathBuf> = rd.flatten().map(|e| e.path().join("bin")).collect();
            vs.sort();
            dirs.extend(vs.into_iter().rev());
        }
    }
    for d in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
        dirs.push(PathBuf::from(d));
    }
    dirs
}

/// rustup 各工具链的 `bin` 目录(新的在前)。`~/.cargo/bin` 里的 rust-analyzer 只是代理,
/// 当前工具链没装该组件时会直接报错退出;真正的可执行文件在装了组件的工具链目录里。
fn rustup_toolchain_bins() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else { return vec![] };
    let Ok(rd) = std::fs::read_dir(home.join(".rustup/toolchains")) else { return vec![] };
    let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path().join("bin")).collect();
    v.sort();
    v.reverse();
    v
}

/// 是不是 rustup 的代理(`~/.cargo/bin/rust-analyzer` 这类指向 `rustup` 的符号链接)。
/// 代理在当前工具链没装该组件时会直接报错退出(`Unknown binary 'rust-analyzer' in official toolchain`),不能当真实服务器用。
fn is_rustup_proxy(p: &Path) -> bool {
    std::fs::read_link(p).ok().and_then(|t| t.file_name().map(|n| n == "rustup")).unwrap_or(false)
}

/// 找可执行文件的完整路径:先找 rustup 工具链里的真实文件(绕开代理),再按 PATH 与常见目录。
/// rustup 代理一律跳过:没装组件时找不到就如实报「未安装」,而不是启动一个必然退出的代理。
pub fn find_binary(cmd: &str) -> Option<PathBuf> {
    let mut dirs = if cmd == "rust-analyzer" { rustup_toolchain_bins() } else { vec![] };
    dirs.extend(base_dirs());
    dirs.into_iter().map(|d| d.join(cmd)).find(|p| p.is_file() && !is_rustup_proxy(p))
}

/// 子进程的 PATH:原 PATH 加上补充目录,让服务器自己再去找 node、cargo、go 等。
pub fn child_path() -> std::ffi::OsString {
    std::env::join_paths(base_dirs()).unwrap_or_default()
}

/// 判定「项目根」的标记文件。
const ROOT_MARKERS: &[&str] = &[".git", "Cargo.toml", "go.mod", "package.json", "pyproject.toml", "Package.swift", "compile_commands.json"];

/// 某个文件的语言服务器工作区根:`hint`(IDE 当前项目根)包含该文件就用它,否则向上找最近的标记文件,都没有就用文件所在目录。
/// 结果是规范化路径(解开符号链接,如 macOS 的 `/var` → `/private/var`),文档 URI 也要用同样的规范化路径,
/// 否则服务器会把文件当成工作区之外的游离文件。
pub fn project_root(file: &Path, hint: &str) -> PathBuf {
    let file = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    if !hint.is_empty() {
        if let Ok(h) = Path::new(hint).canonicalize() {
            if file.starts_with(&h) {
                return h;
            }
        }
    }
    for a in file.ancestors().skip(1) {
        if ROOT_MARKERS.iter().any(|m| a.join(m).exists()) {
            return a.to_path_buf();
        }
    }
    file.parent().map(Path::to_path_buf).unwrap_or(file)
}

/// 启动某服务器要额外设置的环境变量。gopls 需要用户的 `GOPRIVATE`(图形界面启动拿不到 shell 里导出的值),
/// 有了它 gopls 才不会给私有模块生成 pkg.go.dev 链接。
pub fn extra_env(id: &str) -> Vec<(&'static str, String)> {
    match id {
        "gopls" => {
            let p = crate::golinks::private_patterns();
            if p.is_empty() || std::env::var_os("GOPRIVATE").is_some() { vec![] } else { vec![("GOPRIVATE", p.to_string())] }
        }
        _ => vec![],
    }
}

/// 服务器的 `initializationOptions`。
pub fn init_options(spec: &ServerSpec) -> Option<Value> {
    match spec.id {
        // 用独立的 target 目录,避免与用户在终端里的 cargo 构建抢同一把目录锁
        "rust-analyzer" => Some(json!({ "cargo": { "targetDir": true }, "checkOnSave": true })),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn rustup代理被识别() {
        let dir = std::env::temp_dir().join(format!("loom-lsp-proxy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (real, link) = (dir.join("rustup"), dir.join("rust-analyzer"));
        std::fs::write(&real, b"").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(super::is_rustup_proxy(&link));
        assert!(!super::is_rustup_proxy(&real));
        let _ = std::fs::remove_dir_all(&dir);
    }

    use super::*;

    #[test]
    fn 按扩展名选服务器() {
        assert_eq!(spec_for_path(Path::new("/a/b.rs")).unwrap().id, "rust-analyzer");
        assert_eq!(spec_for_path(Path::new("x.PY")).unwrap().id, "pyright");
        assert_eq!(spec_for_path(Path::new("m.tsx")).unwrap().id, "typescript");
        assert!(spec_for_path(Path::new("README.md")).is_none());
        assert!(spec_for_path(Path::new("Makefile")).is_none());
    }

    #[test]
    fn language_id_覆盖() {
        assert_eq!(language_id(Path::new("a.tsx")), "typescriptreact");
        assert_eq!(language_id(Path::new("a.hpp")), "cpp");
        assert_eq!(language_id(Path::new("a.txt")), "plaintext");
    }

    #[test]
    fn 项目根优先用提示再向上找标记() {
        let base = std::env::temp_dir().join(format!("loom-lsp-root-{}", std::process::id()));
        let sub = base.join("a/b");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(base.join("a/Cargo.toml"), "").unwrap();
        std::fs::write(sub.join("x.rs"), "").unwrap();
        let canon = |p: &Path| p.canonicalize().unwrap();
        // 提示根包含文件:用提示根
        assert_eq!(project_root(&sub.join("x.rs"), base.to_str().unwrap()), canon(&base));
        // 没有提示:向上找到带 Cargo.toml 的 a
        assert_eq!(project_root(&sub.join("x.rs"), ""), canon(&base.join("a")));
        // 提示根不包含文件:同样向上找
        assert_eq!(project_root(&sub.join("x.rs"), "/nonexistent-root"), canon(&base.join("a")));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn 每个扩展名只归一个服务器() {
        let mut seen = std::collections::HashSet::new();
        for s in SERVERS {
            for e in s.exts {
                assert!(seen.insert(*e), "扩展名 {e} 重复");
            }
        }
    }
}
