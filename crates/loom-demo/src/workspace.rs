//! 演示沙盒:确定性生成示例项目与 Git 仓库,并提供侧栏的演示数据。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use loom_core::{Project, SessionBadge, SessionMeta};

/// 演示沙盒的默认根目录:`<临时目录>/loom-demo`。
pub fn default_root() -> PathBuf {
    std::env::temp_dir().join("loom-demo")
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
        ("CLAUDE.md", "# 项目约定\n\n- 使用 Rust 2024 edition\n- 提交信息使用中文\n- 修改路由逻辑后必须补测试\n".into()),
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

fn lab_files() -> Vec<(&'static str, String)> {
    vec![("README.md", "# 场景实验室\n\n每个演示场景一个会话,用来逐个检查会话流里的展示。\n".into())]
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
    git_at(dir, args, "2026-10-01T10:00:00+0800")
}

/// 同 `git`,但指定作者与提交时间(演示历史里让提交有先后)。
fn git_at(dir: &Path, args: &[&str], date: &str) -> io::Result<()> {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Demo")
        .env("GIT_AUTHOR_EMAIL", "demo@example.com")
        .env("GIT_COMMITTER_NAME", "Demo")
        .env("GIT_COMMITTER_EMAIL", "demo@example.com")
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
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

/// 给 router-demo 铺出「源代码管理」各组都有内容的状态:
/// 暂存区(README、测试文件)、工作区(utils 已改、测试文件暂存后又改)、未跟踪(新文件、新目录)、已删除。
/// 给演示仓库造一段有分支与合并的历史(「图形」区演示):一条已合并的功能分支、一条未合并的修复分支、一个 tag。
fn seed_history(dir: &Path) {
    let commit = |msg: &str, date: &str| {
        let _ = git_at(dir, &["add", "-A"], date);
        let _ = git_at(dir, &["commit", "-q", "-m", msg], date);
    };
    let _ = git(dir, &["checkout", "-q", "-b", "feature/case-insensitive"]);
    let _ = fs::write(dir.join("src/case.rs"), "pub fn fold(p: &str) -> String {\n    p.to_lowercase()\n}\n");
    commit("feat(router): 路由匹配忽略大小写\n\n- 新增 `case::fold`,匹配前统一转小写。", "2026-10-02T10:00:00+0800");
    let _ = fs::write(dir.join("src/case.rs"), "pub fn fold(p: &str) -> String {\n    p.to_lowercase()\n}\n\n#[test]\nfn folds() {\n    assert_eq!(fold(\"/USERS\"), \"/users\");\n}\n");
    commit("test: 补充大小写折叠用例", "2026-10-03T10:00:00+0800");
    let _ = git(dir, &["checkout", "-q", "main"]);
    let _ = fs::write(dir.join("CHANGELOG.md"), "# 变更记录\n\n- 0.1.0 初始版本\n");
    commit("docs: 补充变更记录", "2026-10-03T15:00:00+0800");
    let _ = git_at(dir, &["merge", "-q", "--no-ff", "feature/case-insensitive", "-m", "merge: 合并 feature/case-insensitive"], "2026-10-04T10:00:00+0800");
    let _ = git(dir, &["tag", "v0.1.0"]);
    let _ = git(dir, &["checkout", "-q", "-b", "fix/trailing-slash"]);
    let _ = fs::write(dir.join("src/slash.rs"), "pub fn trim(p: &str) -> &str {\n    p.trim_end_matches('/')\n}\n");
    commit("fix: 去掉路径末尾的斜杠", "2026-10-05T10:00:00+0800");
    let _ = git(dir, &["checkout", "-q", "main"]);
}

fn seed_scm_state(dir: &Path) {
    if !dir.join(".git").exists() {
        return;
    }
    seed_history(dir);
    let append = |rel: &str, extra: &str| {
        let p = dir.join(rel);
        let old = fs::read_to_string(&p).unwrap_or_default();
        let _ = fs::write(&p, format!("{old}{extra}"));
    };
    append("README.md", "\n## 许可\n\nMIT\n");
    append("tests/router_test.rs", "\n#[test]\nfn ignores_case() {\n    assert!(match_route(\"/USERS\").is_some());\n}\n");
    let _ = git(dir, &["add", "README.md", "tests/router_test.rs"]);
    // 暂存之后再改一次:同一个文件同时出现在「暂存的更改」与「更改」
    append("tests/router_test.rs", "\n// TODO: 补充 404 用例\n");
    let _ = fs::write(dir.join("src/middleware.rs"), "pub fn log(path: &str) {\n    println!(\"GET {path}\");\n}\n");
    let _ = fs::create_dir_all(dir.join("examples"));
    let _ = fs::write(dir.join("examples/basic.rs"), "fn main() {\n    println!(\"{:?}\", router_demo::match_route(\"/users\"));\n}\n");
    let _ = fs::remove_file(dir.join("CLAUDE.md"));
}

/// 生成全部演示项目,返回其路径列表。
pub fn seed(root: &Path) -> io::Result<Vec<PathBuf>> {
    fs::create_dir_all(root)?;
    let router = make_project(root, "router-demo", router_files(), Some(("src/utils.rs", "\npub fn is_root(p: &str) -> bool {\n    p == \"/\"\n}\n")))?;
    seed_scm_state(&router);
    Ok(vec![
        router,
        make_project(root, "web-dashboard", dashboard_files(), None)?,
        make_project(root, "docs-site", docs_files(), Some(("docs/guide.md", "3. 部署\n")))?,
        make_project(root, "scenario-lab", lab_files(), None)?,
    ])
    .and_then(|paths| seed_claude_home(root).map(|_| paths))
}

fn s(id: &str, title: &str, updated: &str, pinned: bool, badge: SessionBadge) -> SessionMeta {
    SessionMeta { id: id.into(), title: title.into(), updated: updated.into(), pinned, archived: false, badge }
}

/// 侧栏的演示数据:路径指向 `seed` 生成的目录。
pub fn demo_projects(root: &Path) -> Vec<Project> {
    let p = |n: &str| root.join(n).to_string_lossy().to_string();
    let mut projects = vec![
        Project {
            name: "router-demo".into(),
            path: p("router-demo"),
            branch: Some("main".into()),
            expanded: true,
            sessions: vec![
                s("showcase-all", "全场景:会话流展示总览", "刚刚", true, SessionBadge::Idle),
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
    ];
    // 场景实验室:其余每个场景一个会话,标题带分组前缀,按分组顺序排列
    let used: Vec<String> = projects.iter().flat_map(|p| p.sessions.iter().map(|x| x.id.clone())).collect();
    let lab: Vec<SessionMeta> = crate::scenarios::all()
        .into_iter()
        .filter(|sc| !used.iter().any(|u| u == sc.id))
        .map(|sc| s(sc.id, &format!("{} · {}", sc.group, sc.title), "场景", false, SessionBadge::Idle))
        .collect();
    projects.push(Project { name: "scenario-lab".into(), path: p("scenario-lab"), branch: Some("main".into()), expanded: false, sessions: lab });
    projects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 演示项目指向沙盒目录() {
        let root = Path::new("/tmp/x");
        let list = demo_projects(root);
        assert_eq!(list.len(), 4);
        // 场景实验室 + 其他项目合起来覆盖全部场景
        let covered: std::collections::HashSet<_> = list.iter().flat_map(|p| p.sessions.iter().map(|x| x.id.clone())).collect();
        for sc in crate::scenarios::all() {
            assert!(covered.contains(sc.id), "场景 {} 没有侧栏会话", sc.id);
        }
        assert!(list[0].path.starts_with("/tmp/x"));
        // 每个侧栏会话 id 都必须对应真实场景
        for p in &list {
            for sess in &p.sessions {
                assert!(crate::scenarios::find(&sess.id).is_some(), "会话 {} 没有对应场景", sess.id);
            }
        }
    }
}

/// 演示模式的家目录:`<沙盒>/home`,其中放假的 `~/.claude/settings.json` 与 `~/.claude.json`,
/// 设置页的读写都落在这里,不碰真实的 `~/.claude`。
pub fn claude_home(root: &Path) -> PathBuf {
    root.join("home")
}

/// 演示用的用户层 `~/.claude/settings.json`。
pub const DEMO_CLAUDE_SETTINGS: &str = r#"{
  "model": "sonnet",
  "env": { "ANTHROPIC_BASE_URL": "***", "HTTPS_PROXY": "***" },
  "permissions": {
    "allow": ["Bash(git status)", "Bash(git diff:*)", "Bash(cargo test:*)", "Read(src/**)"],
    "deny": ["Bash(rm -rf:*)", "Read(.env)"],
    "ask": ["Bash(git push:*)"]
  },
  "enabledPlugins": { "rust-analyzer-lsp@claude-plugins-official": true, "statusline-pro@demo-market": true },
  "hooks": {
    "PreToolUse": [{ "matcher": "Bash", "hooks": [{ "type": "command", "command": "echo \"即将执行命令\"" }] }],
    "PostToolUse": [{ "matcher": "Edit|Write", "hooks": [{ "type": "command", "command": "cargo fmt", "timeout": 30 }] }]
  }
}"#;

/// 演示用的自定义命令、子 Agent、Skill 与用户级 CLAUDE.md(相对 `~/.claude`)。
const DEMO_EXTENSIONS: &[(&str, &str)] = &[
    ("CLAUDE.md", "# 个人偏好\n\n- 回答用中文\n- 改完代码先跑测试\n"),
    ("commands/review-pr.md", "---\ndescription: 审查当前分支相对 main 的改动\nargument-hint: [关注点]\n---\n\n审查 `git diff main...HEAD`,重点关注 $ARGUMENTS。\n"),
    ("commands/git/changelog.md", "---\ndescription: 根据最近提交生成更新日志\n---\n\n读取最近 20 条提交,按 feat / fix 分组写成更新日志。\n"),
    ("agents/code-reviewer.md", "---\nname: code-reviewer\ndescription: 代码审查专家,改完代码后主动使用\ntools: Read, Grep, Glob\nmodel: sonnet\n---\n\n你是资深代码审查员……\n"),
    ("skills/release-notes/SKILL.md", "---\nname: release-notes\ndescription: 发版时整理发布说明\n---\n\n# 发布说明\n\n1. 汇总本版本的提交\n2. 按模块分组\n"),
];

/// 公历日期(UTC):距 1970-01-01 的天数 → `YYYY-MM-DD`。
fn civil_date(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{m:02}-{d:02}", yoe + era * 400 + i64::from(m <= 2))
}

/// 演示用的会话记录:最近 30 天里每天若干回复,两个模型,用来填充用量页。
fn demo_usage_jsonl() -> String {
    let today = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(20_000, |d| d.as_secs() as i64 / 86_400);
    let mut lines = Vec::new();
    for back in 0..30i64 {
        // 工作日多、周末少,数值用确定性公式,每次启动一致
        let load = [3u64, 5, 2, 6, 4, 1, 0][(back % 7) as usize];
        let date = civil_date(today - back);
        for n in 0..load {
            let ts = format!("{date}T{:02}:{:02}:00Z", 9 + n, (back * 7 + n as i64 * 13) % 60);
            let model = if (back + n as i64) % 3 == 0 { "claude-opus-5-5" } else { "claude-sonnet-5-5" };
            let id = format!("m-{back}-{n}");
            lines.push(format!(r#"{{"type":"user","timestamp":"{ts}","message":{{"role":"user","content":"示例提问 {back}-{n}"}}}}"#));
            lines.push(format!(
                r#"{{"type":"assistant","timestamp":"{ts}","message":{{"id":"{id}","model":"{model}","content":[{{"type":"tool_use","id":"t-{id}"}}],"usage":{{"input_tokens":{},"output_tokens":{},"cache_creation_input_tokens":{},"cache_read_input_tokens":{}}}}}}}"#,
                1_200 + n * 300, 600 + back as u64 * 40, 2_000 + n * 150, 20_000 + n * 4_000 + back as u64 * 500
            ));
        }
    }
    lines.join("\n")
}

/// 演示用的插件与插件市场(只写文件,不会真的安装)。
fn seed_plugins(home: &Path) -> io::Result<()> {
    let dir = home.join(".claude/plugins");
    let mut installed = serde_json::Map::new();
    for (id, name, ver, desc, files) in [
        ("rust-analyzer-lsp@claude-plugins-official", "rust-analyzer-lsp", "1.0.0", "Rust 语言服务:跳转、补全与诊断", &[][..]),
        ("statusline-pro@demo-market", "statusline-pro", "1.3.2", "状态栏显示上下文用量、token 与会话时长", &["commands/stats.md", "commands/reset.md"][..]),
        ("gopls-lsp@claude-plugins-official", "gopls-lsp", "1.0.0", "Go 语言服务", &[][..]),
    ] {
        let market = id.split('@').nth(1).unwrap_or("");
        let path = dir.join("cache").join(market).join(name).join(ver);
        fs::create_dir_all(path.join(".claude-plugin"))?;
        fs::write(path.join(".claude-plugin/plugin.json"), serde_json::json!({ "name": name, "description": desc, "author": { "name": "示例作者" } }).to_string())?;
        for f in files {
            fs::create_dir_all(path.join(f).parent().unwrap_or(&path))?;
            fs::write(path.join(f), "---\ndescription: 示例\n---\n")?;
        }
        installed.insert(id.to_string(), serde_json::json!([{ "scope": "user", "version": ver, "installPath": path.to_string_lossy() }]));
    }
    fs::write(dir.join("installed_plugins.json"), serde_json::json!({ "version": 2, "plugins": installed }).to_string())?;
    let markets = serde_json::json!({
        "claude-plugins-official": { "source": { "source": "github", "repo": "anthropics/claude-plugins-official" }, "lastUpdated": "2026-11-01T04:54:26Z" },
        "demo-market": { "source": { "source": "github", "repo": "example/demo-market" }, "lastUpdated": "2026-10-17T09:58:26Z" }
    });
    fs::write(dir.join("known_marketplaces.json"), markets.to_string())
}

/// 生成演示家目录(每次启动重置):用户层 MCP 在 `~/.claude.json` 顶层,
/// router-demo 另有一个本地层 MCP 并禁用了 playwright。
pub fn seed_claude_home(root: &Path) -> io::Result<PathBuf> {
    let home = claude_home(root);
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(home.join(".claude"))?;
    fs::write(home.join(".claude/settings.json"), DEMO_CLAUDE_SETTINGS)?;
    for (rel, text) in DEMO_EXTENSIONS {
        let path = home.join(".claude").join(rel);
        fs::create_dir_all(path.parent().unwrap_or(&home))?;
        fs::write(path, text)?;
    }
    let router = root.join("router-demo").to_string_lossy().to_string();
    let claude_json = serde_json::json!({
        "mcpServers": {
            "filesystem": { "type": "stdio", "command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "."] },
            "github": { "type": "http", "url": "https://api.githubcopilot.com/mcp/" },
            "playwright": { "type": "stdio", "command": "npx", "args": ["@playwright/mcp@latest"] }
        },
        "projects": { router: {
            "mcpServers": { "router-docs": { "type": "sse", "url": "http://localhost:4000/sse" } },
            "disabledMcpServers": ["playwright"]
        } }
    });
    fs::write(home.join(".claude.json"), serde_json::to_string_pretty(&claude_json).map_err(io::Error::other)?)?;
    seed_plugins(&home)?;
    let sessions = home.join(".claude/projects/-demo-router");
    fs::create_dir_all(&sessions)?;
    fs::write(sessions.join("demo-usage.jsonl"), demo_usage_jsonl())?;
    // 演示用的模型服务商:DeepSeek 缺密钥(选择器里显示「去设置」),智谱 GLM 带占位密钥(可直接切换);当前用 Claude 官方
    let providers = serde_json::json!({
        "providers": [
            { "id": "deepseek", "name": "DeepSeek", "base_url": "https://api.deepseek.com/anthropic", "auth": "bearer", "models": ["deepseek-chat", "deepseek-reasoner"] },
            { "id": "glm", "name": "智谱 GLM", "base_url": "https://open.bigmodel.cn/api/anthropic", "auth": "bearer", "models": ["glm-4.6", "glm-4.5-air"] }
        ],
        "mapping": { "enabled": false, "provider": "deepseek", "default": "deepseek-chat", "haiku": "deepseek-chat" }
    });
    fs::write(root.join("providers.json"), serde_json::to_string_pretty(&providers).map_err(io::Error::other)?)?;
    fs::write(root.join("provider-keys.json"), r#"{ "glm": "demo-placeholder" }"#)?;
    Ok(home)
}
