//! 子 Agent 场景。

use super::{Kind, Scenario};
use crate::script::*;
use serde_json::json;
use loom_core::*;

pub fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario { id: "subagents", title: "子 Agent:代码审查与调研", group: "子 Agent", kind: Kind::Auto, prompt: "让子 Agent 帮我审查并调研", build: basic },
        Scenario { id: "subagents-parallel", title: "并行子 Agent:同时审查三个模块", group: "子 Agent", kind: Kind::Auto, prompt: "并行审查 auth、billing、search 三个模块", build: parallel },
        Scenario { id: "subagents-lifecycle", title: "子代理生命周期:启动中、运行中与完成", group: "子 Agent", kind: Kind::Auto, prompt: "派发三个子代理", build: lifecycle },
        Scenario { id: "subagents-markdown", title: "子代理结果:长 Markdown 摘要(标题、列表、代码块)", group: "子 Agent", kind: Kind::Frozen, prompt: "让子 Agent 调研配置与任务", build: markdown_result },
        Scenario { id: "subagents-cards", title: "子代理卡片:五种状态同时停住", group: "子 Agent", kind: Kind::Frozen, prompt: "展示子代理五种状态", build: cards },
    ]
}

fn basic() -> Script {
    let mut b = ScriptBuilder::new();
    let s1 = b.subagent("Explore", "调研项目结构", SubagentStatus::Running);
    b.scope(&s1, |b| {
        b.tool("Glob", "查找 **/*.rs", json!({}), vec![ToolContent::Text("src/main.rs\nsrc/lib.rs".into())]);
        b.say("项目是一个典型的二进制 + 库结构。");
    });
    b.subagent_status(&s1, SubagentStatus::Completed);
    let s2 = b.subagent("code-reviewer", "审查 auth 模块", SubagentStatus::Running);
    b.scope(&s2, |b| {
        b.tool("Read", "读取 src/auth.rs", json!({}), vec![ToolContent::Text("pub fn login() {}".into())]);
        // 两层嵌套
        let inner = b.subagent("general-purpose", "核对依赖版本", SubagentStatus::Running);
        b.scope(&inner, |b| { b.tool("Bash", "cargo tree", json!({}), vec![ToolContent::Text("app v0.1.0".into())]); });
        b.subagent_status(&inner, SubagentStatus::Completed);
    });
    b.subagent_status(&s2, SubagentStatus::Completed);
    let s3 = b.subagent("general-purpose", "分析性能瓶颈", SubagentStatus::Running);
    b.scope(&s3, |b| { b.tool_fail("Bash", "cargo bench", json!({}), vec![ToolContent::Text("bench 目标不存在".into())]); });
    b.subagent_status(&s3, SubagentStatus::Failed);
    b.say("两个子 Agent 完成,一个失败(没有 bench 目标)。");
    b.build()
}

fn parallel() -> Script {
    let mut b = ScriptBuilder::new();
    let a = b.subagent("code-reviewer", "审查 auth", SubagentStatus::Running);
    let c = b.subagent("code-reviewer", "审查 billing", SubagentStatus::Running);
    let d = b.subagent("code-reviewer", "审查 search", SubagentStatus::Running);
    b.scope(&a, |b| { b.tool("Read", "读取 auth.rs", json!({}), vec![ToolContent::Text("…".into())]); });
    b.scope(&c, |b| { b.tool_fail("Read", "读取 billing.rs", json!({}), vec![ToolContent::Text("文件不存在".into())]); });
    b.scope(&d, |b| { b.tool("Read", "读取 search.rs", json!({}), vec![ToolContent::Text("…".into())]); });
    b.subagent_status(&a, SubagentStatus::Completed);
    b.subagent_status(&c, SubagentStatus::Failed);
    b.subagent_status(&d, SubagentStatus::Background);
    b.say("auth 通过,billing 失败,search 已转入后台继续运行。");
    b.build()
}

fn lifecycle() -> Script {
    let mut b = ScriptBuilder::new();
    let ids: Vec<String> = ["调研 A", "调研 B", "调研 C"].iter().map(|d| b.subagent("Explore", d, SubagentStatus::Starting)).collect();
    b.sleep(800);
    for id in &ids { b.subagent_status(id, SubagentStatus::Running); }
    b.sleep(1500);
    for id in &ids { b.subagent_status(id, SubagentStatus::Completed); }
    b.say("三个子代理依次完成。");
    b.build()
}

fn cards() -> Script {
    let mut b = ScriptBuilder::new();
    for (ty, desc, st) in [
        ("Explore", "启动中", SubagentStatus::Starting),
        ("code-reviewer", "运行中", SubagentStatus::Running),
        ("general-purpose", "后台运行", SubagentStatus::Background),
        ("Explore", "已完成", SubagentStatus::Completed),
        ("general-purpose", "已失败", SubagentStatus::Failed),
    ] { b.subagent(ty, desc, st); }
    b.hold();
    b.build()
}

/// 子 Agent 完成后的结果是一大段 Markdown(标题、列表、行内代码、代码块):
/// 右侧「子 Agent」标签里要按 Markdown 渲染,并且内容超出时可以滚动。固定 id `sub-md`,调试用 `LOOM_SUBAGENT=sub-md` 直接打开它的标签。
fn markdown_result() -> Script {
    let mut b = ScriptBuilder::new();
    b.subagent_with_id("sub-md", "Explore", "Explore kb config, tasks, flags", SubagentStatus::Running);
    b.scope("sub-md", |b| {
        b.tool("Grep", "搜索 IsFullTask", json!({}), vec![ToolContent::Text("service/ops/task.go:179\nservice/ops/task.go:199".into())]);
        b.say("读完了任务与配置相关的文件。");
    });
    let summary = "I've mapped all 4 areas below, with `file:line` references.\n\n\
Three things you should know before designing:\n\
- **No LLM client in the backend.** The only downstream model is embedding. `conf/conf.go:20` says Q&A goes through the frontend.\n\
- **The image→chunk link already exists.** The chunker extracts image URLs per chunk (`service/chunker/chunker.go:105`).\n\
- **Some code treats \"not 补向量\" as \"full task\".** Switch those checks to `!IsFullTask(...)`.\n\n\
---\n\n\
## 1. Per-KB settings\n\n\
**Model** — `models/kb/models.go:181-207`, `KnowledgeBase`:\n\
- Scalar flags are int 0/1 columns, one per setting: `IsPublic`, `IsOnline`, `OnlineAccess`, `Avatar`.\n\
- `ParserConfig JSONText` holds the KB default chunk template.\n\n\
```go\ntype KnowledgeBase struct {\n    IsPublic     int\n    IsOnline     int\n    ParserConfig JSONText `gorm:\"type:json\"`\n}\n```\n\n\
## 2. Tasks\n\n\
| 字段 | 含义 |\n| --- | --- |\n| `TaskType` | 任务类型 |\n| `Status` | 状态 |\n\n\
## 3. Migration convention\n\n\
- File name: `sql/kb_alter_YYYYMMDD_<topic>.sql`.\n- Existing: `kb_alter_20261028_kb_online.sql`, `kb_public`, `online_access`.\n\n\
## 4. Frontend\n\n\
1. Settings panel lives under `bigdata-kb-fe/src/pages/kb/settings`.\n2. Each switch calls `PUT /kb/:id/settings`.\n3. The list page reads the same flags.\n\n\
## 5. Risks\n\n\
- A retry would reset the doc to pending.\n- A cancel would mark the doc failed.\n- Backfill jobs reuse the same task type, so check `TaskReembed` too.\n\n\
## 6. Suggested order\n\n\
1. Add the `IsFullTask` helper and switch the two call sites.\n2. Add the new task type behind a flag.\n3. Write the migration.\n4. Wire the frontend switch.\n";
    b.subagent_done("sub-md", SubagentStatus::Completed, summary, 214_500, 43, 387);
    b.say("子代理已完成,结果见右侧标签。");
    b.build()
}
