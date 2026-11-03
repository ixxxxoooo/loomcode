//! 交互类场景:权限、提问、计划批准、改动。

use super::{Kind, Scenario};
use crate::script::*;
use serde_json::json;
use loom_core::*;

pub fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario { id: "ask", title: "提问向导:确认技术选型", group: "交互", kind: Kind::Interactive, prompt: "帮我选一个状态管理方案", build: ask },
        Scenario { id: "ask-variants", title: "提问变体:多选项、多题向导与排队", group: "交互", kind: Kind::Interactive, prompt: "我想新建一个前端项目", build: ask_variants },
        Scenario { id: "permissions", title: "权限请求:修改配置与执行命令", group: "交互", kind: Kind::Interactive, prompt: "把 CI 配置里的 Node 版本升到 22 并跑一遍", build: permissions },
        Scenario { id: "permissions-variants", title: "权限请求变体:删除、联网、MCP 与并发堆叠", group: "交互", kind: Kind::Interactive, prompt: "清理临时文件并查一下依赖更新", build: permissions_variants },
        Scenario { id: "plan-approval", title: "计划批准:迁移遗留接口", group: "交互", kind: Kind::Interactive, prompt: "把旧的 REST 接口迁移到新网关", build: plan_approval },
        Scenario { id: "changes", title: "本轮改动:检查点与一键回退", group: "交互", kind: Kind::Interactive, prompt: "在 README 末尾加一段版权说明", build: changes },
    ]
}

fn ask() -> Script {
    let mut b = ScriptBuilder::new();
    b.think("选型需要先确认团队规模与偏好。");
    b.question(vec![
        question("状态管理", "你更倾向哪种状态管理方案?", &[("Zustand", "轻量,API 简单"), ("Redux Toolkit", "生态成熟,样板较多"), ("Jotai", "原子化状态")], false),
        question("附加能力", "需要哪些附加能力?", &[("持久化", "写入 localStorage"), ("DevTools", "时间旅行调试"), ("中间件", "日志/异步")], true),
    ]);
    b.say("收到,将按你的选择搭建。");
    b.build()
}

fn ask_variants() -> Script {
    let mut b = ScriptBuilder::new();
    b.question(vec![question("框架", "选择前端框架", &[
        ("React", "生态最大"), ("Vue", "上手快"), ("Svelte", "编译时框架"), ("Solid", "细粒度响应式"),
        ("Angular", "全家桶"), ("Qwik", "可恢复性"), ("Astro", "内容站点")], false)]);
    b.question(vec![
        question("语言", "使用哪种语言?", &[("TypeScript", "推荐"), ("JavaScript", "无类型")], false),
        question("样式", "样式方案?", &[("Tailwind", "原子类"), ("CSS Modules", "局部作用域"), ("UnoCSS", "按需生成")], true),
        question("部署", "部署到哪里?", &[("Vercel", "零配置"), ("Cloudflare", "边缘运行时")], false),
    ]);
    b.say("配置完毕。");
    b.build()
}

fn permissions() -> Script {
    let mut b = ScriptBuilder::new();
    let old = "node-version: 20\n";
    let new = "node-version: 22\n";
    let tool = ToolCall::new("perm-edit", "Edit", "编辑 .github/workflows/ci.yml").with_location(".github/workflows/ci.yml")
        .with_input(json!({"file_path": ".github/workflows/ci.yml"}))
        .with_content(ToolContent::Diff { path: ".github/workflows/ci.yml".into(), old: old.into(), new: new.into() });
    b.permission(tool, standard_options());
    b.tool("Edit", "编辑 .github/workflows/ci.yml", json!({}), vec![ToolContent::Diff { path: ".github/workflows/ci.yml".into(), old: old.into(), new: new.into() }]);
    let tool = ToolCall::new("perm-bash", "Bash", "运行 npm test").with_input(json!({"command": "npm test"}));
    b.permission(tool, standard_options());
    b.tool("Bash", "npm test", json!({"command": "npm test"}), vec![ToolContent::Terminal { command: "npm test".into(), output: "Tests: 42 passed".into(), exit_code: Some(0) }]);
    b.say("配置已升级,测试通过。");
    b.build()
}

fn permissions_variants() -> Script {
    let mut b = ScriptBuilder::new();
    b.permission(ToolCall::new("pv-del", "Bash", "删除 build/ 目录").with_input(json!({"command": "rm -rf build"})), standard_options());
    b.permission(ToolCall::new("pv-web", "WebFetch", "访问 registry.npmjs.org").with_input(json!({"url": "https://registry.npmjs.org"})), standard_options());
    b.permission(ToolCall::new("pv-mcp", "mcp__github__list_prs", "GitHub · 列出 PR").with_input(json!({"repo": "acme/app"})), standard_options());
    // 最后两个同时请求,上下堆叠
    b.permission(ToolCall::new("pv-t1", "Bash", "cargo clippy").with_input(json!({"command": "cargo clippy"})), standard_options());
    b.say("四类权限请求演示完毕。");
    b.build()
}

fn plan_approval() -> Script {
    let mut b = ScriptBuilder::new();
    b.mode(PermissionMode::Plan);
    b.think("计划模式下只读,先梳理接口清单再给出迁移步骤。");
    b.tool("Grep", "搜索旧接口 /api/v1", json!({"pattern": "/api/v1"}), vec![ToolContent::Text("src/client.ts:8\nsrc/legacy.ts:21".into())]);
    b.plan_streaming("# 迁移旧 REST 接口到新网关\n\n## 步骤\n1. 新建 `gateway/` 适配层,统一鉴权\n2. 逐个迁移 `/api/v1/*` 至 `/gw/*`\n3. 双写一周,对比响应\n4. 下线旧接口\n\n## 风险\n- 老客户端仍依赖 v1 字段\n- 需要灰度开关\n");
    b.mode(PermissionMode::AcceptEdits);
    b.todos("todos-plan", &[("新建网关适配层", TodoStatus::InProgress), ("迁移接口", TodoStatus::Pending), ("双写对比", TodoStatus::Pending)]);
    b.say("计划已批准,开始执行。");
    b.build()
}

fn changes() -> Script {
    let mut b = ScriptBuilder::new();
    b.permission(ToolCall::new("chg-edit", "Edit", "编辑 README.md").with_location("README.md")
        .with_content(ToolContent::Diff { path: "README.md".into(), old: "# App\n".into(), new: "# App\n\n© 2026 Acme. All rights reserved.\n".into() }), standard_options());
    b.tool_at("Edit", "编辑 README.md", json!({}), "README.md", vec![ToolContent::Diff { path: "README.md".into(), old: "# App\n".into(), new: "# App\n\n© 2026 Acme. All rights reserved.\n".into() }]);
    b.append(TimelineItem::Changes { id: "chg-1".into(), files: vec![ChangedFile { path: "README.md".into(), added: 2, removed: 0 }] });
    b.say("已添加。可在上方「本轮改动」中查看或回退。");
    b.build()
}
