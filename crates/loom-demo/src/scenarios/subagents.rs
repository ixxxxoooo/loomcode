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
