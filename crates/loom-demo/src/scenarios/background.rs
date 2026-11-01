//! 后台任务与子 Agent 场景:后台命令、Monitor、后台子代理、并行与嵌套、恢复运行、任务通知。
//!
//! 每个场景都拆成「往构建器里写内容」的函数(`*_b`),既能单独成场景,也能被「全场景会话」拼接。

use super::{Kind, Scenario};
use crate::script::*;
use serde_json::json;
use loom_core::*;

pub fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario { id: "showcase-all", title: "全场景:会话流里的所有展示一次看完", group: "全景", kind: Kind::Auto, prompt: "把会话流里的所有展示都演示一遍", build: showcase_all },
        Scenario { id: "bg-dock", title: "任务坞:按类别分组的悬浮列表(全部类型、终态、停滞与滚动)", group: "后台任务", kind: Kind::Auto, prompt: "同时开一堆后台任务,检验任务坞的分组与悬浮列表", build: dock },
        Scenario { id: "bg-shell", title: "后台命令:开发服务器、构建与失败的 lint", group: "后台任务", kind: Kind::Auto, prompt: "把开发服务器、release 构建和 clippy 都放到后台跑", build: shell },
        Scenario { id: "bg-monitor", title: "Monitor:持续监听日志与结束", group: "后台任务", kind: Kind::Auto, prompt: "监听服务日志,出现错误就告诉我", build: monitor },
        Scenario { id: "bg-agent", title: "后台子代理:盘点、审查、失败与停滞", group: "后台任务", kind: Kind::Auto, prompt: "让几个子代理在后台并行盘点设置页和审查模块", build: agent },
        Scenario { id: "bg-parallel", title: "并行子代理:前台、后台与各种终态", group: "后台任务", kind: Kind::Auto, prompt: "并行派发七个子代理", build: parallel },
        Scenario { id: "bg-nested", title: "嵌套子代理:三层嵌套与内部后台任务", group: "后台任务", kind: Kind::Auto, prompt: "让子代理再派发子代理去调研", build: nested },
        Scenario { id: "bg-resume", title: "恢复运行:同一个任务编号再次启动", group: "后台任务", kind: Kind::Auto, prompt: "让刚才完成的审查子代理继续补充意见", build: resume },
        Scenario { id: "bg-notes", title: "任务通知:完成、失败与停止的各类任务", group: "后台任务", kind: Kind::Auto, prompt: "展示各类后台任务的通知", build: notes },
        Scenario { id: "tools-status", title: "工具状态:成功、失败、被拒绝与被中断", group: "工具卡", kind: Kind::Auto, prompt: "展示工具调用的各种结果状态", build: status },
        Scenario { id: "notices-system", title: "系统提示:钩子失败、模型回退与 MCP 输入请求", group: "状态与错误", kind: Kind::Auto, prompt: "展示各类系统提示", build: system_notices },
    ]
}

fn dock() -> Script {
    let mut b = ScriptBuilder::new();
    b.title("任务坞:分组与悬浮列表");
    b.say("同时开启**所有类型**的后台任务:子代理、后台命令、监听、工作流和其他任务,用来检验输入框上方任务坞的分组、悬浮展开与收起。");
    shell_b(&mut b);
    monitor_b(&mut b);
    agent_b(&mut b);
    dock_extra_b(&mut b);
    b.build()
}
fn shell() -> Script {
    let mut b = ScriptBuilder::new();
    shell_b(&mut b);
    b.build()
}
fn monitor() -> Script {
    let mut b = ScriptBuilder::new();
    monitor_b(&mut b);
    b.build()
}
fn agent() -> Script {
    let mut b = ScriptBuilder::new();
    agent_b(&mut b);
    b.build()
}
fn parallel() -> Script {
    let mut b = ScriptBuilder::new();
    parallel_b(&mut b);
    b.build()
}
fn nested() -> Script {
    let mut b = ScriptBuilder::new();
    nested_b(&mut b);
    b.build()
}
fn resume() -> Script {
    let mut b = ScriptBuilder::new();
    resume_b(&mut b);
    b.build()
}
fn notes() -> Script {
    let mut b = ScriptBuilder::new();
    notes_b(&mut b);
    b.build()
}
fn status() -> Script {
    let mut b = ScriptBuilder::new();
    status_b(&mut b);
    b.build()
}
fn system_notices() -> Script {
    let mut b = ScriptBuilder::new();
    system_notices_b(&mut b);
    b.build()
}

/// 全场景会话:基础工具卡 + 全部后台任务 / 子 Agent / 状态 / 系统提示片段,一个会话看完。
fn showcase_all() -> Script {
    let mut b = ScriptBuilder::new();
    b.title("全场景:会话流展示总览");
    b.think("这是一个覆盖会话流全部展示的会话:先是基础工具,再是后台任务、子 Agent、各种状态与系统提示。");
    b.say("## 一、基础工具\n先看读取、搜索、编辑、命令与待办。");
    b.tool("Read", "读取 src/main.rs · 42 行", json!({"file_path": "src/main.rs"}), vec![ToolContent::Text("fn main() {\n    println!(\"hi\");\n}".into())]);
    b.tool("Grep", "搜索 TODO", json!({"pattern": "TODO"}), vec![ToolContent::Text("src/lib.rs:8: // TODO".into())]);
    b.tool_at("Edit", "编辑 src/lib.rs", json!({"file_path": "src/lib.rs"}), "src/lib.rs", vec![ToolContent::Diff { path: "src/lib.rs".into(), old: "// TODO\n".into(), new: "// 已处理\n".into() }]);
    b.tool("Bash", "cargo test", json!({"command": "cargo test"}), vec![ToolContent::Terminal { command: "cargo test".into(), output: "test result: ok. 12 passed".into(), exit_code: Some(0) }]);
    b.todos("showcase-todos", &[("基础工具", TodoStatus::Completed), ("后台任务", TodoStatus::InProgress), ("子 Agent", TodoStatus::Pending)]);
    b.say("## 二、后台任务与子 Agent");
    all_parts(&mut b);
    b.say("## 三、总结\n以上覆盖了会话流的全部展示;点开输入框上方的任务条可以看到所有后台任务。");
    b.build()
}

/// 供「全场景会话」拼接的全部片段(顺序即会话里的顺序)。
pub fn all_parts(b: &mut ScriptBuilder) {
    shell_b(b);
    monitor_b(b);
    agent_b(b);
    parallel_b(b);
    nested_b(b);
    resume_b(b);
    notes_b(b);
    status_b(b);
    system_notices_b(b);
}

// ---------- 小工具 ----------

/// 后台命令 / Monitor:工具卡立即返回,任务进入后台表,之后的输出与终态由时间线推进。
fn bg_bash(b: &mut ScriptBuilder, task_id: &str, title: &str, command: &str, kind: TaskKind, job: BgJob) {
    let id = b.next_id("bash");
    let name = if kind == TaskKind::Monitor { "Monitor" } else { "Bash" };
    b.tool_start(ToolCall::new(&id, name, title).with_input(json!({"command": command, "description": title, "run_in_background": true})));
    b.sleep(200);
    b.task_started(BackgroundTask::new(task_id, kind, title).with_tool_use(&id).with_output(&format!("/tmp/loom-demo/tasks/{task_id}.output")));
    b.tool_end(&id, ToolStatus::Completed, vec![], 200);
    b.spawn(job);
}

/// 后台子代理:卡片先出现,随后任务启动并转入后台。
fn bg_agent(b: &mut ScriptBuilder, card_id: &str, task_id: &str, agent_type: &str, desc: &str, job: BgJob) {
    b.subagent_with_id(card_id, agent_type, desc, SubagentStatus::Running);
    b.sleep(150);
    b.task_started(BackgroundTask::new(task_id, TaskKind::Agent, desc).with_tool_use(card_id));
    b.spawn(job);
}

/// 子代理内部的一次工具调用:`at` 开始,`dur` 毫秒后完成。
fn job_tool(job: BgJob, at: u64, dur: u64, card: &str, tid: &str, name: &str, title: &str, out: &str) -> BgJob {
    let call = ToolCall::new(tid, name, title);
    job.child(at, card, TimelineItem::Tool(call)).at(
        at + dur,
        AgentEvent::ToolUpdate {
            parent: Some(card.into()),
            id: tid.into(),
            patch: ToolPatch { status: Some(ToolStatus::Completed), append_content: vec![ToolContent::Text(out.into())], duration_ms: Some(dur), ..Default::default() },
        },
    )
}

fn say_child(job: BgJob, at: u64, card: &str, id: &str, text: &str) -> BgJob {
    job.child(at, card, TimelineItem::Assistant { id: id.into(), text: text.into(), streaming: false })
}

/// 任务坞专用的补充任务:工作流、其他类型,以及足够多的长期运行命令(让悬浮列表超过最大高度、出现滚动)。
fn dock_extra_b(b: &mut ScriptBuilder) {
    b.say("再补充工作流、其他任务和几个长期运行的命令。");
    bg_bash(
        b, "wf-release", "发布流程(三阶段)", "workflow run release", TaskKind::Workflow,
        BgJob::new("wf-release").output(1000, "阶段 1/3:构建\n").output(6000, "阶段 2/3:测试\n").stoppable("发布流程已停止"),
    );
    bg_bash(
        b, "ot-clean", "清理临时目录", "cleanup --tmp", TaskKind::Other,
        BgJob::new("ot-clean").output(1500, "removed 128 files\n").stoppable("清理已停止"),
    );
    for (id, title, cmd) in [
        ("sh-story", "启动 Storybook", "pnpm storybook"),
        ("sh-docker", "docker compose up", "docker compose up"),
        ("sh-watch", "监视测试", "cargo watch -x test"),
        ("sh-tail", "跟踪构建日志", "tail -f build.log"),
    ] {
        bg_bash(b, id, title, cmd, TaskKind::Shell, BgJob::new(id).output(900, "ready\n").stoppable(&format!("{title}已停止")));
    }
    b.say("现在每一类都有任务:点输入框上方的胶囊,列表会在胶囊上方展开;再点胶囊、点列表外、按 Esc 都会收起。");
}

// ---------- 后台命令 ----------

fn shell_b(b: &mut ScriptBuilder) {
    b.think("开发服务器、release 构建和 clippy 互不依赖,都放到后台跑,不阻塞对话;完成后再汇总。");
    b.say("我把三件事都放到**后台**运行:开发服务器、release 构建、clippy 检查。你可以在输入框上方的任务条里查看数量、输出,也可以随时停止。");
    bg_bash(
        b, "sh-dev", "启动开发服务器", "pnpm dev", TaskKind::Shell,
        BgJob::new("sh-dev")
            .output(600, "  VITE v5.4.0  ready in 312 ms\n\n  ➜  Local:   http://localhost:3000/\n")
            .output(5000, "[hmr] update /src/App.tsx\n")
            .output(11000, "[hmr] update /src/Chart.tsx\n")
            .output(18000, "GET /api/metrics 200 12ms\n")
            .stoppable("开发服务器已停止"),
    );
    bg_bash(
        b, "sh-build", "编译 release 并打包", "cargo build --release", TaskKind::Shell,
        BgJob::new("sh-build")
            .output(800, "   Compiling router-demo v0.1.0\n")
            .output(3500, "    Finished `release` profile [optimized] target(s) in 6.82s\n")
            .finish(7000, TaskStatus::Completed, "后台命令「编译 release 并打包」已完成(退出码 0)")
            .followup(7300, "release 构建完成,产物在 `target/release/router-demo`。")
            .stoppable("编译已停止"),
    );
    bg_bash(
        b, "sh-lint", "运行 clippy 严格检查", "cargo clippy -- -D warnings", TaskKind::Shell,
        BgJob::new("sh-lint")
            .output(1200, "warning: unused variable: `x`\n --> src/utils.rs:4:9\n")
            .output(3000, "error: could not compile `router-demo` (lib) due to 3 previous errors\n")
            .finish(4500, TaskStatus::Failed, "后台命令「运行 clippy 严格检查」失败(退出码 101)"),
    );
    b.sleep(500);
    let mut out = ToolCall::new("bgout-1", "BashOutput", "读取后台输出").with_input(json!({"bash_id": "sh-dev"}));
    out.task_id = Some("sh-dev".into());
    b.tool_start(out);
    b.sleep(240);
    b.tool_end("bgout-1", ToolStatus::Completed, vec![ToolContent::Text("VITE v5.4.0  ready in 312 ms\n➜  Local:   http://localhost:3000/".into())], 240);
    // 一个已经不需要的旧任务被主动结束
    b.task_started(BackgroundTask::new("sh-old", TaskKind::Shell, "旧的文件监听").with_tool_use("kill-1"));
    let mut kill = ToolCall::new("kill-1", "KillShell", "结束后台任务").with_input(json!({"shell_id": "sh-old"}));
    kill.task_id = Some("sh-old".into());
    b.tool_start(kill);
    b.sleep(240);
    b.tool_end("kill-1", ToolStatus::Completed, vec![ToolContent::Text("已结束 sh-old".into())], 240);
    b.event(AgentEvent::TaskNotification { task_id: "sh-old".into(), status: TaskStatus::Stopped, summary: "后台命令「旧的文件监听」已停止".into(), output_file: None });
    b.say("开发服务器已就绪(3000 端口);release 构建和 clippy 仍在后台跑,结束后会通知我。");
}

// ---------- Monitor ----------

fn monitor_b(b: &mut ScriptBuilder) {
    b.say("用 **Monitor** 持续监听服务日志,只在出现 ERROR 时才打断我。");
    let mut job = BgJob::new("mon-log").stoppable("日志监听已停止");
    for (i, line) in [
        "[12:01:03] INFO  GET /api/users 200",
        "[12:01:07] INFO  GET /api/posts 200",
        "[12:01:12] WARN  slow query 1.4s",
        "[12:01:19] ERROR upstream timeout: payments",
        "[12:01:25] INFO  GET /health 200",
    ]
    .iter()
    .enumerate()
    {
        job = job.output(1500 + i as u64 * 2500, &format!("{line}\n"));
    }
    bg_bash(b, "mon-log", "监听服务日志", "tail -f logs/app.log | grep --line-buffered ERROR", TaskKind::Monitor, job);
    bg_bash(
        b, "mon-deploy", "监听部署状态直到完成", "watch-deploy --until done", TaskKind::Monitor,
        BgJob::new("mon-deploy")
            .output(1200, "deploy: building\n")
            .output(4000, "deploy: rolling out 3/5\n")
            .output(7000, "deploy: done\n")
            .finish(7200, TaskStatus::Completed, "监听「监听部署状态直到完成」已结束:部署完成")
            .followup(7500, "部署已完成,监听结束。"),
    );
    b.say("两个监听都已启动:日志监听会一直运行;部署监听在部署完成时自动结束。");
}

// ---------- 后台子代理 ----------

fn agent_b(b: &mut ScriptBuilder) {
    b.think("盘点和审查互不依赖,各派一个后台子代理;再起一个长时间调研。");
    b.say("派发四个**后台子代理**,它们并行运行,卡片上能看到用时、工具数和当前动作。");
    // A:盘点设置页,会成功
    let mut a = BgJob::new("t-ag-a").stoppable("盘点设置页已被停止");
    a = job_tool(a, 800, 600, "ag-a", "ag-a-t1", "Glob", "查找 **/settings/*.rs", "settings/mod.rs\nsettings/nav.rs");
    a = a.progress(1500, 6200, 1, "Glob", "正在列出设置页文件");
    a = job_tool(a, 2200, 900, "ag-a", "ag-a-t2", "Grep", "搜索 SettingsTab", "mod.rs:12: pub enum SettingsTab");
    a = a.progress(3300, 14800, 2, "Grep", "正在统计设置分类");
    a = job_tool(a, 4000, 800, "ag-a", "ag-a-t3", "Read", "读取 settings/mod.rs", "pub enum SettingsTab { General, Appearance, ... }");
    a = say_child(a, 5200, "ag-a", "ag-a-say", "共盘点 22 个设置页,其中 3 个缺少说明文案。");
    a = a.progress(5400, 23100, 3, "Read", "整理盘点结果");
    a = a.finish(6200, TaskStatus::Completed, "盘点完成:22 个设置页,3 个缺少说明").followup(6500, "设置页盘点完成:共 22 页,有 3 页缺少说明文案,需要补齐。");
    bg_agent(b, "ag-a", "t-ag-a", "Explore", "盘点所有设置页", a);
    // B:审查失败
    let mut r = BgJob::new("t-ag-r").stoppable("审查已被停止");
    r = job_tool(r, 900, 500, "ag-r", "ag-r-t1", "Read", "读取 src/auth.rs", "pub fn login() {}");
    r = r.progress(1600, 9100, 1, "Read", "审查 auth 模块");
    r = r.finish(4000, TaskStatus::Failed, "审查失败:无法读取 src/billing.rs(文件不存在)");
    bg_agent(b, "ag-r", "t-ag-r", "code-reviewer", "审查 auth 与 billing", r);
    // C:长时间调研,一直在跑(可手动停止)
    let mut c = BgJob::new("t-ag-c").stoppable("长时间调研已被停止");
    c = job_tool(c, 1000, 700, "ag-c", "ag-c-t1", "WebSearch", "搜索网页:Rust 异步运行时对比", "tokio / async-std / smol");
    c = c.progress(2000, 11000, 1, "WebSearch", "收集资料");
    c = job_tool(c, 6000, 900, "ag-c", "ag-c-t2", "WebFetch", "抓取 tokio.rs", "…");
    c = c.progress(7000, 24000, 2, "WebFetch", "阅读运行时文档");
    c = job_tool(c, 16000, 900, "ag-c", "ag-c-t3", "Read", "读取 Cargo.toml", "[dependencies]");
    c = c.progress(17000, 38000, 3, "Read", "对照现有依赖");
    bg_agent(b, "ag-c", "t-ag-c", "general-purpose", "调研异步运行时选型", c);
    // D:停滞——最后一次活动在 90 秒前
    let mut stalled = BackgroundTask::new("t-ag-d", TaskKind::Agent, "批量迁移旧接口").with_tool_use("ag-d");
    stalled.started_ms = stalled.started_ms.saturating_sub(185_000);
    stalled.last_activity_ms = stalled.last_activity_ms.saturating_sub(95_000);
    stalled.tokens = 61_200;
    stalled.tool_uses = 14;
    stalled.last_tool = "Bash".into();
    stalled.summary = "重试 cargo test(第 4 次)".into();
    b.subagent_with_id("ag-d", "general-purpose", "批量迁移旧接口", SubagentStatus::Running);
    b.sleep(150);
    b.task_started(stalled);
    b.spawn(BgJob::new("t-ag-d").stoppable("批量迁移已被停止"));
    b.say("四个子代理已在后台运行:盘点会成功,审查会失败,调研会持续运行,**批量迁移已经 95 秒没有进展**(疑似停滞),可以直接停止它。");
}

// ---------- 并行子代理 ----------

fn parallel_b(b: &mut ScriptBuilder) {
    b.say("并行派发 **7 个**子代理:3 个前台、2 个后台、1 个失败、1 个被停止。");
    let ids: Vec<String> = ["审查 auth", "审查 billing", "审查 search"].iter().map(|d| b.subagent("code-reviewer", d, SubagentStatus::Running)).collect();
    for (i, id) in ids.iter().enumerate() {
        b.scope(id, |b| {
            b.tool("Read", &format!("读取 module{i}.rs"), json!({}), vec![ToolContent::Text("…".into())]);
            b.tool("Grep", "搜索 unwrap()", json!({}), vec![ToolContent::Text("2 处".into())]);
        });
    }
    // 前台子代理结束带统计
    b.subagent_done(&ids[0], SubagentStatus::Completed, "auth 模块没有发现问题", 18_400, 6, 12);
    b.subagent_done(&ids[2], SubagentStatus::Completed, "search 有 2 处 unwrap 需要处理", 21_900, 7, 15);
    // 失败
    let f = b.subagent("general-purpose", "核对依赖许可证", SubagentStatus::Running);
    b.scope(&f, |b| { b.tool_fail("Bash", "cargo deny check", json!({}), vec![ToolContent::Text("cargo-deny 未安装".into())]); });
    b.subagent_done(&f, SubagentStatus::Failed, "无法完成:cargo-deny 未安装", 4_200, 1, 3);
    // 被停止
    let st = b.subagent("Explore", "全仓库扫描", SubagentStatus::Running);
    b.scope(&st, |b| { b.tool("Glob", "查找 **/*", json!({}), vec![ToolContent::Text("3,214 个文件".into())]); });
    b.subagent_status(&st, SubagentStatus::Stopped);
    // 两个后台
    for (cid, tid, d, ty) in [("pb-1", "t-pb-1", "生成接口文档", "general-purpose"), ("pb-2", "t-pb-2", "迁移测试夹具", "general-purpose")] {
        let job = BgJob::new(tid)
            .progress(1500, 8_000, 2, "Write", "写入文档草稿")
            .progress(4000, 16_000, 4, "Edit", "补充示例")
            .stoppable("已被停止");
        bg_agent(b, cid, tid, ty, d, job);
    }
    // billing 仍在前台运行(进行中的并行项)
    b.scope(&ids[1], |b| { b.tool("Bash", "cargo test billing", json!({}), vec![ToolContent::Text("running 12 tests".into())]); });
    b.subagent_done(&ids[1], SubagentStatus::Completed, "billing 测试全部通过", 25_300, 9, 18);
    b.say("七个子代理的状态各不相同,主线程只保留摘要,完整对话点开卡片在右侧查看。");
}

// ---------- 嵌套 ----------

fn nested_b(b: &mut ScriptBuilder) {
    b.say("让调研子代理再派发子代理,形成三层嵌套;最内层还会启动一个后台任务。");
    let l1 = b.subagent("general-purpose", "调研渲染性能(第 1 层)", SubagentStatus::Running);
    b.scope(&l1, |b| {
        b.say("先拆成两个方向并行调研。");
        let l2 = b.subagent("Explore", "分析列表虚拟化(第 2 层)", SubagentStatus::Running);
        b.scope(&l2, |b| {
            b.tool("Grep", "搜索 virtual_list", json!({}), vec![ToolContent::Text("chat_scroll.rs:42".into())]);
            let l3 = b.subagent("general-purpose", "核对测量缓存(第 3 层)", SubagentStatus::Running);
            b.scope(&l3, |b| {
                b.tool("Read", "读取 view_cache.rs", json!({}), vec![ToolContent::Text("pub struct ViewCache".into())]);
                b.tool("Bash", "cargo test view_cache", json!({}), vec![ToolContent::Terminal { command: "cargo test view_cache".into(), output: "test result: ok. 6 passed".into(), exit_code: Some(0) }]);
            });
            b.subagent_done(&l3, SubagentStatus::Completed, "测量缓存命中率 94%", 9_800, 2, 6);
        });
        b.subagent_done(&l2, SubagentStatus::Completed, "虚拟化已覆盖所有长列表", 15_600, 3, 9);
        b.tool("Read", "读取 smooth_scroll.rs", json!({}), vec![ToolContent::Text("…".into())]);
    });
    b.subagent_done(&l1, SubagentStatus::Completed, "渲染性能瓶颈在重内容行的首次测量", 31_000, 7, 22);
    b.say("三层嵌套都已完成,结论只在最外层卡片上汇总。");
}

// ---------- 恢复运行 ----------

fn resume_b(b: &mut ScriptBuilder) {
    b.say("先让审查子代理在后台跑完,再让它继续补充意见(同一个任务编号会再次启动)。");
    let job = BgJob::new("t-res")
        .progress(900, 7_000, 2, "Read", "审查 router.rs")
        .finish(2200, TaskStatus::Completed, "审查完成:发现 1 处大小写问题");
    bg_agent(b, "res-1", "t-res", "code-reviewer", "审查路由层", job);
    b.sleep(3200);
    b.tool("SendMessage", "继续补充 · 审查路由层", json!({"to": "t-res", "summary": "补充安全方面的意见"}), vec![ToolContent::Text("已向子代理发送消息".into())]);
    // 恢复运行:复用 t-res,重新启动
    b.task_started(BackgroundTask::new("t-res", TaskKind::Agent, "审查路由层(补充意见)").with_tool_use("res-1"));
    b.spawn(
        BgJob::new("t-res")
            .progress(1200, 12_000, 3, "Grep", "检查路径遍历")
            .finish(3200, TaskStatus::Completed, "补充意见:路径需要规范化,防止 `..` 逃逸")
            .stoppable("补充审查已被停止"),
    );
    b.say("同一个子代理被恢复运行:卡片回到后台运行态,结束后再次出现通知。");
}

// ---------- 任务通知 ----------

fn notes_b(b: &mut ScriptBuilder) {
    b.say("下面是不同类型后台任务的结束通知。");
    let defs = [
        ("n-sh-ok", TaskKind::Shell, "运行集成测试", TaskStatus::Completed, "后台命令「运行集成测试」已完成(退出码 0)"),
        ("n-sh-fail", TaskKind::Shell, "数据库迁移", TaskStatus::Failed, "后台命令「数据库迁移」失败(退出码 1)"),
        ("n-mon", TaskKind::Monitor, "监听构建输出", TaskStatus::Completed, "监听「监听构建输出」已结束"),
        ("n-ag-ok", TaskKind::Agent, "整理发布说明", TaskStatus::Completed, "子代理「整理发布说明」已完成:草稿写入 RELEASE.md"),
        ("n-ag-stop", TaskKind::Agent, "扫描大文件", TaskStatus::Stopped, "子代理「扫描大文件」已被停止"),
        ("n-wf", TaskKind::Workflow, "三阶段发布流程", TaskStatus::Completed, "工作流「三阶段发布流程」已完成:3 个阶段,5 个子代理"),
    ];
    for (id, kind, desc, ..) in &defs {
        b.task_started(BackgroundTask::new(id, *kind, desc));
    }
    b.sleep(400);
    for (id, _, _, status, summary) in defs {
        b.event(AgentEvent::TaskNotification { task_id: id.into(), status, summary: summary.into(), output_file: None });
        b.sleep(150);
    }
    b.say("通知行可以点击查看对应任务的输出;已结束的任务保留在任务列表里。");
}

// ---------- 工具状态 ----------

fn status_b(b: &mut ScriptBuilder) {
    b.say("工具调用的几种结果:成功、失败、被拒绝、被中断。");
    b.tool("Read", "读取 README.md · 12 行", json!({}), vec![ToolContent::Text("# Router Demo".into())]);
    b.tool_fail("Bash", "cargo build", json!({}), vec![ToolContent::Terminal { command: "cargo build".into(), output: "error[E0432]: unresolved import `foo`\n --> src/main.rs:1:5".into(), exit_code: Some(101) }]);
    let mut denied = ToolCall::new("st-denied", "Bash", "删除构建缓存").with_input(json!({"command": "rm -rf target"}));
    denied.status = ToolStatus::Denied;
    denied.content.push(ToolContent::Terminal { command: "rm -rf target".into(), output: "用户拒绝了这次操作".into(), exit_code: None });
    b.append(TimelineItem::Tool(denied));
    let mut cut = ToolCall::new("st-cut", "Bash", "运行全量测试").with_input(json!({"command": "cargo test --workspace"}));
    cut.status = ToolStatus::Interrupted;
    cut.content.push(ToolContent::Terminal { command: "cargo test --workspace".into(), output: "running 120 tests\ntest a ... ok\n^C".into(), exit_code: None });
    b.append(TimelineItem::Tool(cut));
    let denied_perm = ToolCall { status: ToolStatus::Denied, ..ToolCall::new("st-perm", "Edit", "编辑 .env").with_input(json!({"file_path": ".env"})).with_content(ToolContent::Diff { path: ".env".into(), old: "KEY=1\n".into(), new: "KEY=2\n".into() }) };
    b.append(TimelineItem::Permission(PermissionRequest { id: "st-perm".into(), tool: denied_perm, options: standard_options(), resolved: Some("deny".into()) }));
    b.say("被拒绝和被中断的卡片用不同的颜色和文案区分,不会和失败混在一起。");
}

// ---------- 系统提示 ----------

fn system_notices_b(b: &mut ScriptBuilder) {
    b.notice(NoticeLevel::Warning, "钩子 Stop 执行失败(退出码 2):lint 未通过,3 个文件需要格式化");
    b.notice(NoticeLevel::Warning, "模型 claude-opus-5-5 暂不可用,已回退到 claude-sonnet-5-5");
    b.notice(NoticeLevel::Warning, "模型拒绝了这次请求,将以更保守的方式重试一次");
    b.notice(NoticeLevel::Info, "MCP 服务 github 请求输入「个人访问令牌」,已自动拒绝(暂不支持在对话里填写)");
    b.notice(NoticeLevel::Info, "权限模式已切换为「计划模式」");
    b.notice(NoticeLevel::Warning, "请求失败,正在重试(2/10)");
    b.notice(NoticeLevel::Info, "已压缩上下文");
    b.notice(NoticeLevel::Error, "回合失败:上游返回 529,服务过载");
}
