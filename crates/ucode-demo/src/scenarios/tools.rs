//! 工具卡相关场景与全景场景。

use super::{Kind, Scenario};
use crate::script::*;
use serde_json::json;
use ucode_core::*;

pub fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario { id: "showcase-tour", title: "完整修复一个 Bug:路由大小写匹配", group: "全景", kind: Kind::Interactive,
            prompt: "路由匹配对大小写敏感,/Users 打不开 /users 页面,帮我修复并加上测试", build: showcase },
        Scenario { id: "tools-files", title: "文件读写:梳理并整理路由层", group: "工具卡", kind: Kind::Auto,
            prompt: "梳理一下路由层的文件,把重复的工具函数整理到 utils 里", build: files },
        Scenario { id: "tools-shell", title: "终端:构建、测试与后台服务", group: "工具卡", kind: Kind::Auto,
            prompt: "跑一下构建和测试,再把开发服务器启动起来", build: shell },
        Scenario { id: "tools-web-mcp", title: "联网与 MCP:调研迁移指南并核对 PR", group: "工具卡", kind: Kind::Auto,
            prompt: "查一下 v5 的迁移指南,并核对 PR #482 的改动", build: web_mcp },
        Scenario { id: "tools-image", title: "读取图片:查看图标与报错截图", group: "工具卡", kind: Kind::Auto,
            prompt: "看看 docs/error.png 这张报错截图", build: image },
        Scenario { id: "tools-plan-todo", title: "待办与计划:拆解指标上报任务", group: "工具卡", kind: Kind::Auto,
            prompt: "把指标上报功能拆成几步,逐步完成", build: plan_todo },
        Scenario { id: "tools-browser", title: "内置浏览器:排查指标看板报错", group: "工具卡", kind: Kind::Auto,
            prompt: "打开本地的指标看板,看看为什么图表是空的", build: browser },
        Scenario { id: "tools-more", title: "更多内置工具:Skill、LSP、后台输出与定时任务", group: "工具卡", kind: Kind::Auto,
            prompt: "演示一遍其余的内置工具", build: more },
        Scenario { id: "canvas", title: "画布:生成季度质量周报", group: "工具卡", kind: Kind::Auto,
            prompt: "整理成一份季度质量周报画布", build: canvas },
        Scenario { id: "terminal", title: "内嵌终端:在客户端终端运行命令", group: "工具卡", kind: Kind::Auto,
            prompt: "在终端里运行 ls -la", build: terminal },
    ]
}

fn read_input(path: &str) -> serde_json::Value {
    json!({ "file_path": path })
}

const ROUTER_RS: &str = "pub fn match_route(path: &str) -> Option<Route> {\n    ROUTES.iter().find(|r| r.pattern == path).cloned()\n}\n";
const ROUTER_FIXED: &str = "pub fn match_route(path: &str) -> Option<Route> {\n    let path = path.to_ascii_lowercase();\n    ROUTES.iter().find(|r| r.pattern == path).cloned()\n}\n";

fn showcase() -> Script {
    let mut b = ScriptBuilder::new();
    b.title("修复路由大小写匹配");
    b.think("先确认用户期望:URL 路径是否应该忽略大小写?按 RFC 3986 路径是大小写敏感的,但多数 Web 应用选择忽略。需要向用户确认。");
    b.question(vec![
        question("匹配策略", "路由匹配时应如何处理大小写?", &[
            ("忽略大小写(推荐)", "统一转成小写后再匹配"),
            ("保持敏感", "只在 404 页提示可能的拼写"),
        ], false),
    ]);
    b.say("好的,采用**忽略大小写**。先看一下现有实现。");
    b.tool_at("Grep", "搜索 match_route", json!({"pattern": "match_route"}), "src/router.rs",
        vec![ToolContent::Text("src/router.rs:12:pub fn match_route(path: &str) -> Option<Route> {\ntests/router_test.rs:8:    assert!(match_route(\"/users\").is_some());".into())]);
    b.tool_at("Read", "读取 src/router.rs", read_input("src/router.rs"), "src/router.rs",
        vec![ToolContent::Text(ROUTER_RS.into())]);
    b.todos("todos-1", &[
        ("修改 match_route 忽略大小写", TodoStatus::InProgress),
        ("补充大小写测试", TodoStatus::Pending),
        ("运行全部测试", TodoStatus::Pending),
    ]);
    let perm_tool = ToolCall::new("tool-edit-1", "Edit", "编辑 src/router.rs")
        .with_input(json!({"file_path": "src/router.rs"}))
        .with_location("src/router.rs")
        .with_content(ToolContent::Diff { path: "src/router.rs".into(), old: ROUTER_RS.into(), new: ROUTER_FIXED.into() });
    b.permission(perm_tool, standard_options());
    b.tool_at("Edit", "编辑 src/router.rs", json!({"file_path": "src/router.rs"}), "src/router.rs",
        vec![ToolContent::Diff { path: "src/router.rs".into(), old: ROUTER_RS.into(), new: ROUTER_FIXED.into() }]);
    b.todos("todos-1", &[
        ("修改 match_route 忽略大小写", TodoStatus::Completed),
        ("补充大小写测试", TodoStatus::InProgress),
        ("运行全部测试", TodoStatus::Pending),
    ]);
    let test_old = "#[test]\nfn finds_users() {\n    assert!(match_route(\"/users\").is_some());\n}\n";
    let test_new = format!("{test_old}\n#[test]\nfn ignores_case() {{\n    assert!(match_route(\"/Users\").is_some());\n}}\n");
    b.tool_at("Edit", "编辑 tests/router_test.rs", json!({"file_path": "tests/router_test.rs"}), "tests/router_test.rs",
        vec![ToolContent::Diff { path: "tests/router_test.rs".into(), old: test_old.into(), new: test_new }]);
    let id = b.tool_start(ToolCall::new("tool-test-1", "Bash", "cargo test").with_input(json!({"command": "cargo test"})));
    b.sleep(500);
    b.tool_end(&id, ToolStatus::Failed, vec![ToolContent::Terminal {
        command: "cargo test".into(),
        output: "running 2 tests\ntest finds_users ... \u{1b}[32mok\u{1b}[0m\ntest ignores_case ... \u{1b}[31mFAILED\u{1b}[0m\n\nfailures:\n    ignores_case: pattern 未转小写\n".into(),
        exit_code: Some(101) }], 480);
    b.think("测试失败:路由表里的 pattern 本身含大写(`/Users`),需要在注册时也转小写。");
    let sub = b.subagent("code-reviewer", "审查路由层改动是否有遗漏", SubagentStatus::Starting);
    b.sleep(300).subagent_status(&sub, SubagentStatus::Running);
    b.scope(&sub, |b| {
        b.tool_at("Read", "读取 src/routes.rs", read_input("src/routes.rs"), "src/routes.rs", vec![ToolContent::Text("pub const ROUTES: &[Route] = &[Route::new(\"/Users\"), Route::new(\"/Posts\")];".into())]);
        b.say("发现 `ROUTES` 中的 pattern 含大写,需要同时规范化。");
    });
    b.subagent_status(&sub, SubagentStatus::Completed);
    b.tool_at("Edit", "编辑 src/routes.rs", json!({"file_path": "src/routes.rs"}), "src/routes.rs",
        vec![ToolContent::Diff { path: "src/routes.rs".into(), old: "Route::new(\"/Users\"), Route::new(\"/Posts\")".into(), new: "Route::new(\"/users\"), Route::new(\"/posts\")".into() }]);
    b.tool("Bash", "cargo test", json!({"command": "cargo test"}), vec![ToolContent::Terminal {
        command: "cargo test".into(), output: "running 2 tests\ntest finds_users ... \u{1b}[32mok\u{1b}[0m\ntest ignores_case ... \u{1b}[32mok\u{1b}[0m\n\ntest result: ok. 2 passed".into(), exit_code: Some(0) }]);
    b.todos("todos-1", &[
        ("修改 match_route 忽略大小写", TodoStatus::Completed),
        ("补充大小写测试", TodoStatus::Completed),
        ("运行全部测试", TodoStatus::Completed),
    ]);
    b.append(TimelineItem::Changes { id: "changes-1".into(), files: vec![
        ChangedFile { path: "src/router.rs".into(), added: 1, removed: 0 },
        ChangedFile { path: "src/routes.rs".into(), added: 1, removed: 1 },
        ChangedFile { path: "tests/router_test.rs".into(), added: 5, removed: 0 },
    ]});
    b.say("## 已修复\n\n路由匹配现在**忽略大小写**:\n\n1. `match_route` 在比较前把请求路径转成小写;\n2. 路由表的 pattern 统一为小写;\n3. 新增 `ignores_case` 测试,全部 **2/2** 通过。\n\n| 文件 | 改动 |\n| --- | --- |\n| `src/router.rs` | +1 |\n| `src/routes.rs` | +1 −1 |\n| `tests/router_test.rs` | +5 |\n");
    b.usage(48_200, 200_000, Some(18.0), Some(6.0));
    b.build()
}

fn files() -> Script {
    let mut b = ScriptBuilder::new();
    b.say("先看看路由层有哪些文件。");
    b.tool("Glob", "查找 src/**/*.rs", json!({"pattern": "src/**/*.rs"}), vec![ToolContent::Text("src/main.rs\nsrc/router.rs\nsrc/routes.rs\nsrc/utils.rs\nsrc/handlers/users.rs\nsrc/handlers/posts.rs".into())]);
    let long: String = (1..=120).map(|i| format!("{i:>4}  // 第 {i} 行:路由层说明与示例代码\n")).collect();
    b.tool_at("Read", "读取 src/router.rs(120 行)", read_input("src/router.rs"), "src/router.rs", vec![ToolContent::Text(long)]);
    b.tool("Grep", "搜索 normalize_path", json!({"pattern": "normalize_path", "output_mode": "content"}),
        vec![ToolContent::Text("src/router.rs:40:fn normalize_path(p: &str) -> String {\nsrc/handlers/users.rs:9:fn normalize_path(p: &str) -> String {".into())]);
    b.tool_at("Write", "新建 src/utils.rs", json!({"file_path": "src/utils.rs"}), "src/utils.rs",
        vec![ToolContent::Diff { path: "src/utils.rs".into(), old: String::new(), new: "pub fn normalize_path(p: &str) -> String {\n    p.trim_end_matches('/').to_ascii_lowercase()\n}\n".into() }]);
    b.tool_at("Edit", "编辑 src/handlers/users.rs", json!({"file_path": "src/handlers/users.rs"}), "src/handlers/users.rs",
        vec![ToolContent::Diff { path: "src/handlers/users.rs".into(), old: "fn normalize_path(p: &str) -> String {\n    p.to_string()\n}\n".into(), new: "use crate::utils::normalize_path;\n".into() }]);
    b.tool("NotebookEdit", "编辑 notebooks/routes.ipynb", json!({"notebook_path": "notebooks/routes.ipynb"}), vec![ToolContent::Text("已替换单元格 3".into())]);
    b.tool("Bash", "移动 src/old_utils.rs → src/utils/legacy.rs", json!({"command": "mv src/old_utils.rs src/utils/legacy.rs"}), vec![ToolContent::Text("".into())]);
    b.tool("Bash", "删除 src/tmp.rs", json!({"command": "rm src/tmp.rs"}), vec![ToolContent::Text("".into())]);
    b.say("整理完成:重复的 `normalize_path` 已合并到 `src/utils.rs`。");
    b.build()
}

fn shell() -> Script {
    let mut b = ScriptBuilder::new();
    let build_out = "\u{1b}[1m\u{1b}[32m   Compiling\u{1b}[0m serde v1.0.229\n\u{1b}[1m\u{1b}[32m   Compiling\u{1b}[0m app v0.1.0\n\u{1b}[1m\u{1b}[33mwarning\u{1b}[0m: unused variable: `x`\n --> src/main.rs:4:9\n\u{1b}[1m\u{1b}[32m    Finished\u{1b}[0m `dev` profile in 3.21s";
    b.tool("Bash", "cargo build", json!({"command": "cargo build"}), vec![ToolContent::Terminal { command: "cargo build".into(), output: build_out.into(), exit_code: Some(0) }]);
    let long: String = (1..=300).map(|i| format!("\u{1b}[36m[{i:04}]\u{1b}[0m 处理文件 module_{i}.rs ... \u{1b}[32mok\u{1b}[0m\n")).collect();
    b.tool("Bash", "cargo test(超长输出)", json!({"command": "cargo test"}), vec![ToolContent::Terminal { command: "cargo test".into(), output: long, exit_code: Some(0) }]);
    b.tool_fail("Bash", "npm run lint", json!({"command": "npm run lint"}), vec![ToolContent::Terminal { command: "npm run lint".into(), output: "\u{1b}[31merror\u{1b}[0m  'foo' is assigned a value but never used  no-unused-vars\n\n✖ 1 problem (1 error, 0 warnings)".into(), exit_code: Some(1) }]);
    b.tool("Bash", "多行命令", json!({"command": "set -e\ncd app\nnpm ci\nnpm run build"}), vec![ToolContent::Terminal { command: "set -e\ncd app\nnpm ci\nnpm run build".into(), output: "added 312 packages in 8s\nbuilt in 2.4s".into(), exit_code: Some(0) }]);
    b.tool("Bash", "后台启动开发服务器", json!({"command": "npm run dev", "run_in_background": true}), vec![ToolContent::Text("后台任务已启动:shell_1".into())]);
    b.tool("BashOutput", "读取后台输出 shell_1", json!({"bash_id": "shell_1"}), vec![ToolContent::Text("VITE v6.0.0  ready in 312 ms\n  ➜  Local:   http://localhost:5173/".into())]);
    b.say("构建通过(1 个警告),lint 有 1 个错误需要处理,开发服务器已在 5173 端口就绪。");
    b.build()
}

fn web_mcp() -> Script {
    let mut b = ScriptBuilder::new();
    b.tool("WebSearch", "搜索「v5 migration guide」", json!({"query": "v5 migration guide"}), vec![ToolContent::Text("1. Migrating to v5 — docs.example.com\n2. What's new in v5 — blog.example.com".into())]);
    b.tool("WebFetch", "获取 docs.example.com/migrate", json!({"url": "https://docs.example.com/migrate"}), vec![ToolContent::Text("# 迁移到 v5\n\n- `Router.match` 改名为 `Router.resolve`\n- 移除了 `legacy` 选项".into())]);
    b.tool("mcp__github__get_pull_request", "GitHub · 获取 PR #482", json!({"owner": "acme", "repo": "app", "number": 482}), vec![ToolContent::Text("{\n  \"number\": 482,\n  \"title\": \"feat: v5 路由迁移\",\n  \"state\": \"open\",\n  \"changed_files\": 14\n}".into())]);
    b.tool("mcp__filesystem__read_file", "filesystem · 读取 README.md", json!({"path": "README.md"}), vec![ToolContent::Text("# App\n路由示例项目".into())]);
    b.tool_fail("mcp__github__create_issue", "GitHub · 创建 Issue", json!({"title": "迁移 v5"}), vec![ToolContent::Text("401 Unauthorized:令牌已过期,请在设置 → MCP 中重新授权".into())]);
    b.say("迁移指南已读完。PR #482 改了 **14** 个文件,主要是 `Router.match` → `Router.resolve` 的重命名;创建 Issue 因 MCP 授权过期失败。");
    b.build()
}

fn image() -> Script {
    let mut b = ScriptBuilder::new();
    b.tool("Read", "读取 docs/error.png", read_input("docs/error.png"), vec![ToolContent::Image { label: "error.png · 1280×720 报错截图".into(), path: None }]);
    b.tool("Read", "读取 assets/icon.svg", read_input("assets/icon.svg"), vec![ToolContent::Image { label: "icon.svg · 64×64 应用图标".into(), path: None }]);
    b.say("截图显示:**TypeError: Cannot read properties of undefined (reading 'map')**,发生在 `Chart.tsx:58`,原因是数据尚未加载就渲染了。");
    b.build()
}

fn plan_todo() -> Script {
    let mut b = ScriptBuilder::new();
    b.think("拆成四步:定义事件、实现上报器、接入页面、写测试。");
    let steps = |done: usize| -> Vec<(&'static str, TodoStatus)> {
        ["定义 Metric 事件模型", "实现批量上报器", "接入页面埋点", "编写单元测试"].iter().enumerate()
            .map(|(i, s)| (*s, if i < done { TodoStatus::Completed } else if i == done { TodoStatus::InProgress } else { TodoStatus::Pending })).collect()
    };
    for done in 0..4 {
        b.todos("todos-metrics", &steps(done)).sleep(400);
        b.tool("Write", &format!("完成:{}", steps(done)[done].0), json!({}), vec![ToolContent::Text("ok".into())]);
    }
    b.todos("todos-metrics", &steps(4).iter().map(|(s, _)| (*s, TodoStatus::Completed)).collect::<Vec<_>>());
    b.mode(PermissionMode::AcceptEdits);
    b.say("四步全部完成。");
    b.build()
}

fn browser() -> Script {
    let mut b = ScriptBuilder::new();
    b.tool("mcp__playwright__browser_navigate", "浏览器 · 打开 localhost:5173/dashboard", json!({"url": "http://localhost:5173/dashboard"}), vec![ToolContent::Text("页面已加载:指标看板".into())]);
    b.tool("mcp__playwright__browser_snapshot", "浏览器 · 页面快照", json!({}), vec![ToolContent::Text("- heading \"指标看板\"\n- region \"图表\" [ref=e3]\n- button \"刷新\" [ref=e5]".into())]);
    b.tool("mcp__playwright__browser_click", "浏览器 · 点击「刷新」", json!({"ref": "e5"}), vec![ToolContent::Text("已点击".into())]);
    b.tool("mcp__playwright__browser_take_screenshot", "浏览器 · 截图", json!({}), vec![ToolContent::Image { label: "dashboard.png · 图表区域为空".into(), path: None }]);
    b.tool("mcp__playwright__browser_console_messages", "浏览器 · 控制台", json!({}), vec![ToolContent::Text("[error] TypeError: Cannot read properties of undefined (reading 'map')\n    at Chart.tsx:58".into())]);
    b.tool("mcp__playwright__browser_network_requests", "浏览器 · 网络请求", json!({}), vec![ToolContent::Text("GET /api/metrics → 500 Internal Server Error".into())]);
    b.say("接口 `/api/metrics` 返回 **500**,前端没做空数据保护,所以图表整块为空。");
    b.build()
}

fn more() -> Script {
    let mut b = ScriptBuilder::new();
    b.tool("Glob", "查找 **/*.proto", json!({"pattern": "**/*.proto"}), vec![ToolContent::Text("(没有匹配)".into())]);
    b.tool("Grep", "搜索 TODO(内容模式)", json!({"pattern": "TODO", "output_mode": "content"}), vec![ToolContent::Text("src/main.rs:12: // TODO 处理错误\nsrc/router.rs:3: // TODO 缓存".into())]);
    b.tool("Skill", "技能 · pdf", json!({"skill": "pdf"}), vec![ToolContent::Text("已加载 pdf 技能".into())]);
    b.tool("ToolSearch", "工具搜索 · notebook", json!({"query": "notebook"}), vec![ToolContent::Text("NotebookEdit, NotebookRead".into())]);
    b.tool("LSP", "LSP · 跳转到定义 match_route", json!({"op": "definition"}), vec![ToolContent::Text("src/router.rs:12:8".into())]);
    b.tool("mcp__scheduler__cron_create", "定时任务 · 每日 09:00 跑测试", json!({"cron": "0 9 * * *"}), vec![ToolContent::Text("已创建 cron_7".into())]);
    b.tool_fail("Bash", "沙盒拦截:写入 /etc/hosts", json!({"command": "echo x >> /etc/hosts"}), vec![ToolContent::Text("操作被沙盒拦截:不允许写入工作区以外的路径".into())]);
    b.tool("Bash", "进入 Worktree feature-x", json!({"command": "git worktree add ../feature-x"}), vec![ToolContent::Text("Preparing worktree (new branch 'feature-x')".into())]);
    b.say("以上是其余内置工具的呈现。");
    b.build()
}

fn canvas() -> Script {
    let mut b = ScriptBuilder::new();
    b.tool("mcp__ucode__canvas_create", "画布 · 创建「季度质量周报」", json!({"title": "季度质量周报"}), vec![ToolContent::Text("canvas-q3".into())]);
    b.append(TimelineItem::Canvas { id: "canvas-q3".into(), title: "季度质量周报".into() });
    b.tool("mcp__ucode__canvas_update", "画布 · 更新图表", json!({"id": "canvas-q3"}), vec![ToolContent::Text("已更新 3 个块".into())]);
    b.say("画布已生成,点击卡片可在右侧面板打开。");
    b.build()
}

fn terminal() -> Script {
    let mut b = ScriptBuilder::new();
    b.tool("Bash", "在终端运行 ls -la", json!({"command": "ls -la"}), vec![ToolContent::Terminal {
        command: "ls -la".into(), output: "total 24\ndrwxr-xr-x  6 demo  staff  192 Oct 22 13:00 .\n-rw-r--r--  1 demo  staff  412 Oct 22 13:00 Cargo.toml\ndrwxr-xr-x  4 demo  staff  128 Oct 22 13:00 src".into(), exit_code: Some(0) }]);
    b.say("命令已在客户端终端中执行。");
    b.build()
}
