//! 演示沙盒:确定性生成示例项目与 Git 仓库,并提供侧栏的演示数据。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use ucode_core::{Project, SessionBadge, SessionMeta};

/// 演示沙盒的默认根目录:`<临时目录>/ucode-demo`。
pub fn default_root() -> PathBuf {
    std::env::temp_dir().join("ucode-demo")
}

/// 示例项目里的文件(相对路径, 内容)。
fn router_files() -> Vec<(&'static str, String)> {
    vec![
        ("Cargo.toml", "[package]\nname = \"router-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n".into()),
        ("README.md", "# Router Demo\n\n一个用于演示的路由库。\n\n## 用法\n\n```rust\nlet r = match_route(\"/users\");\n```\n".into()),
        ("src/main.rs", "mod router;\nmod routes;\nmod utils;\n\nfn main() {\n    println!(\"{:?}\", router::match_route(\"/users\"));\n}\n".into()),
        ("src/router.rs", "use crate::routes::{Route, ROUTES};\n\n/// 按路径匹配路由。\npub fn match_route(path: &str) -> Option<Route> {\n    ROUTES.iter().find(|r| r.pattern == path).cloned()\n}\n".into()),
        ("src/routes.rs", "#[derive(Clone, Debug)]\npub struct Route {\n    pub pattern: &'static str,\n}\n\nimpl Route {\n    pub const fn new(pattern: &'static str) -> Self {\n        Self { pattern }\n    }\n}\n\npub const ROUTES: &[Route] = &[Route::new(\"/Users\"), Route::new(\"/Posts\")];\n".into()),
        ("src/utils.rs", "pub fn normalize_path(p: &str) -> String {\n    p.trim_end_matches('/').to_string()\n}\n".into()),
        ("tests/router_test.rs", "use router_demo::match_route;\n\n#[test]\nfn finds_users() {\n    assert!(match_route(\"/users\").is_some());\n}\n".into()),
        (".gitignore", "target\n".into()),
    ]
}

fn dashboard_files() -> Vec<(&'static str, String)> {
    vec![
        ("package.json", "{\n  \"name\": \"web-dashboard\",\n  \"version\": \"1.0.0\",\n  \"scripts\": { \"dev\": \"vite\", \"build\": \"vite build\" }\n}\n".into()),
        ("src/Chart.tsx", "export function Chart({ data }: { data?: number[] }) {\n  return <svg>{data.map((v, i) => <rect key={i} height={v} />)}</svg>;\n}\n".into()),
        ("src/App.tsx", "import { Chart } from './Chart';\nexport default function App() {\n  return <Chart />;\n}\n".into()),
        ("README.md", "# Web Dashboard\n".into()),
    ]
}

fn docs_files() -> Vec<(&'static str, String)> {
    vec![
        ("docs/index.md", "# 文档站点\n\n欢迎。\n".into()),
        ("docs/guide.md", "# 指南\n\n1. 安装\n2. 配置\n".into()),
        ("README.md", "# Docs Site\n".into()),
    ]
}

fn write_files(root: &Path, files: &[(&str, String)]) -> io::Result<()> {
    for (rel, content) in files {
        let path = root.join(rel);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, content)?;
    }
    Ok(())
}

/// 在 `dir` 内执行 git 命令,失败只返回错误,不 panic。
fn git(dir: &Path, args: &[&str]) -> io::Result<()> {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Demo")
        .env("GIT_AUTHOR_EMAIL", "demo@example.com")
        .env("GIT_COMMITTER_NAME", "Demo")
        .env("GIT_COMMITTER_EMAIL", "demo@example.com")
        .env("GIT_AUTHOR_DATE", "2026-10-01T10:00:00+0800")
        .env("GIT_COMMITTER_DATE", "2026-10-01T10:00:00+0800")
        .output()?
        .status;
    if status.success() { Ok(()) } else { Err(io::Error::other(format!("git {args:?} 失败"))) }
}

/// 生成一个项目:写文件、初始化仓库、提交,并留下一处未提交改动。
fn make_project(root: &Path, name: &str, files: Vec<(&'static str, String)>, dirty: Option<(&str, &str)>) -> io::Result<PathBuf> {
    let dir = root.join(name);
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    fs::create_dir_all(&dir)?;
    write_files(&dir, &files)?;
    // git 不可用时仍然保留文件(Git 面板会显示「非仓库」)
    if git(&dir, &["init", "-q", "-b", "main"]).is_ok() {
        let _ = git(&dir, &["add", "-A"]);
        let _ = git(&dir, &["commit", "-q", "-m", "chore: 初始提交"]);
        if let Some((rel, extra)) = dirty {
            let p = dir.join(rel);
            let old = fs::read_to_string(&p).unwrap_or_default();
            fs::write(&p, format!("{old}{extra}"))?;
        }
    }
    Ok(dir)
}

/// 生成全部演示项目,返回其路径列表。
pub fn seed(root: &Path) -> io::Result<Vec<PathBuf>> {
    fs::create_dir_all(root)?;
    Ok(vec![
        make_project(root, "router-demo", router_files(), Some(("src/utils.rs", "\npub fn is_root(p: &str) -> bool {\n    p == \"/\"\n}\n")))?,
        make_project(root, "web-dashboard", dashboard_files(), None)?,
        make_project(root, "docs-site", docs_files(), Some(("docs/guide.md", "3. 部署\n")))?,
    ])
}

fn s(id: &str, title: &str, updated: &str, pinned: bool, badge: SessionBadge) -> SessionMeta {
    SessionMeta { id: id.into(), title: title.into(), updated: updated.into(), pinned, badge }
}

/// 侧栏的演示数据:路径指向 `seed` 生成的目录。
pub fn demo_projects(root: &Path) -> Vec<Project> {
    let p = |n: &str| root.join(n).to_string_lossy().to_string();
    vec![
        Project {
            name: "router-demo".into(),
            path: p("router-demo"),
            branch: Some("main".into()),
            expanded: true,
            sessions: vec![
                s("showcase-tour", "完整修复一个 Bug:路由大小写匹配", "刚刚", true, SessionBadge::Running),
                s("permissions", "权限请求:修改配置与执行命令", "5 分钟前", false, SessionBadge::Waiting),
                s("tools-files", "文件读写:梳理并整理路由层", "1 小时前", false, SessionBadge::Unread),
                s("markdown", "Markdown 与代码高亮:发布说明", "昨天", false, SessionBadge::Idle),
            ],
        },
        Project {
            name: "web-dashboard".into(),
            path: p("web-dashboard"),
            branch: Some("feature/charts".into()),
            expanded: true,
            sessions: vec![
                s("tools-browser", "内置浏览器:排查指标看板报错", "2 小时前", false, SessionBadge::Idle),
                s("subagents", "子 Agent:代码审查与调研", "昨天", false, SessionBadge::Idle),
            ],
        },
        Project {
            name: "docs-site".into(),
            path: p("docs-site"),
            branch: Some("main".into()),
            expanded: false,
            sessions: vec![s("plan-approval", "计划批准:迁移遗留接口", "3 天前", false, SessionBadge::Idle)],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 演示项目指向沙盒目录() {
        let root = Path::new("/tmp/x");
        let list = demo_projects(root);
        assert_eq!(list.len(), 3);
        assert!(list[0].path.starts_with("/tmp/x"));
        // 每个侧栏会话 id 都必须对应真实场景
        for p in &list {
            for sess in &p.sessions {
                assert!(crate::scenarios::find(&sess.id).is_some(), "会话 {} 没有对应场景", sess.id);
            }
        }
    }
}
