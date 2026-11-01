//! 状态、错误、用量与模式场景。

use super::{Kind, Scenario};
use crate::script::*;
use serde_json::json;
use loom_core::*;

pub fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario { id: "states-working", title: "运行中:长思考、引导与排队", group: "状态与错误", kind: Kind::Frozen, prompt: "深入分析一下这个架构", build: working },
        Scenario { id: "states-frozen", title: "冻结态:待处理与进行中的工具", group: "状态与错误", kind: Kind::Frozen, prompt: "冻结在运行中的工具", build: frozen },
        Scenario { id: "states-notices", title: "提示与用量:额度回退与上下文告急", group: "状态与错误", kind: Kind::Auto, prompt: "展示各类提示", build: notices },
        Scenario { id: "states-errors", title: "失败:工具、子 Agent 与错误提示", group: "状态与错误", kind: Kind::Auto, prompt: "展示各类失败", build: failures },
        Scenario { id: "states-streaming", title: "长输出:流式渲染与滚动跟随", group: "状态与错误", kind: Kind::Auto, prompt: "写一篇很长的说明", build: streaming },
        Scenario { id: "errors-refusal", title: "模型拒绝:无法协助该请求", group: "状态与错误", kind: Kind::Abnormal, prompt: "(触发拒绝)", build: refusal },
        Scenario { id: "errors-max-tokens", title: "输出上限:回复被截断", group: "状态与错误", kind: Kind::Abnormal, prompt: "(触发截断)", build: max_tokens },
        Scenario { id: "errors-rate-limit", title: "上游限流:请求过于频繁", group: "状态与错误", kind: Kind::Abnormal, prompt: "(触发限流)", build: rate_limit },
        Scenario { id: "errors-auth", title: "登录失效:需要重新登录 Claude Code", group: "状态与错误", kind: Kind::Abnormal, prompt: "(触发鉴权失败)", build: auth },
        Scenario { id: "usage-limits", title: "套餐限额:5 小时与每周额度逐步逼近", group: "状态与错误", kind: Kind::Auto, prompt: "展示额度变化", build: usage_limits },
        Scenario { id: "context-compact", title: "上下文压缩:接近上限后自动摘要", group: "状态与错误", kind: Kind::Auto, prompt: "展示上下文压缩", build: compact },
        Scenario { id: "modes-matrix", title: "权限模式:六种模式依次切换", group: "输入区与模式", kind: Kind::Auto, prompt: "依次切换权限模式", build: modes },
    ]
}

fn working() -> Script {
    let mut b = ScriptBuilder::new();
    b.think(&"正在深入分析这个架构的各个层次,权衡模块边界与依赖方向。".repeat(8));
    b.hold();
    b.build()
}

fn frozen() -> Script {
    let mut b = ScriptBuilder::new();
    b.tool_start(ToolCall::new("fz-1", "Bash", "运行长时间任务").with_input(json!({"command": "sleep 999"})));
    b.tool_start({ let mut t = ToolCall::new("fz-2", "Read", "待处理的读取"); t.status = ToolStatus::Pending; t });
    b.subagent("Explore", "仍在运行的调研", SubagentStatus::Running);
    b.hold();
    b.build()
}

fn notices() -> Script {
    let mut b = ScriptBuilder::new();
    b.notice(NoticeLevel::Info, "已切换为 Haiku(额度回退)");
    b.notice(NoticeLevel::Warning, "上下文已使用 92%,即将自动压缩");
    b.notice(NoticeLevel::Error, "上游返回 529:服务过载,已自动重试");
    b.title("新的标题").mode(PermissionMode::Plan);
    b.usage(186_000, 200_000, Some(82.0), Some(40.0));
    b.build()
}

fn failures() -> Script {
    let mut b = ScriptBuilder::new();
    b.tool_fail("Bash", "cargo build", json!({}), vec![ToolContent::Terminal { command: "cargo build".into(), output: "error[E0432]: unresolved import `foo`".into(), exit_code: Some(101) }]);
    b.tool_fail("Edit", "编辑 src/x.rs", json!({}), vec![ToolContent::Text("old_string 未找到".into())]);
    b.tool_fail("mcp__github__get_pr", "GitHub · 获取 PR", json!({}), vec![ToolContent::Text("连接超时".into())]);
    let s = b.subagent("general-purpose", "失败的子 Agent", SubagentStatus::Running);
    b.subagent_status(&s, SubagentStatus::Failed);
    b.notice(NoticeLevel::Error, "本回合有 4 项失败");
    b.build()
}

fn streaming() -> Script {
    let mut b = ScriptBuilder::new();
    let text: String = (1..=40).map(|i| format!("### 第 {i} 节\n这是流式输出的第 {i} 段内容,用来检查滚动跟随与渲染性能。包含 `行内代码` 与 **加粗**。\n\n")).collect();
    b.say(&text);
    b.build()
}

fn refusal() -> Script {
    let mut b = ScriptBuilder::new();
    b.say("抱歉,我无法协助这个请求。");
    b.notice(NoticeLevel::Warning, "模型拒绝了该请求");
    b.stops(StopReason::Refusal);
    b.build()
}

fn max_tokens() -> Script {
    let mut b = ScriptBuilder::new();
    b.say("这是一段很长的回复,写到一半就被……");
    b.notice(NoticeLevel::Warning, "已达到输出上限,回复被截断。发送「继续」可接着写");
    b.stops(StopReason::MaxTokens);
    b.build()
}

fn rate_limit() -> Script {
    let mut b = ScriptBuilder::new();
    b.notice(NoticeLevel::Error, "上游触发限流(429):请求过于频繁,请稍后再试");
    b.stops(StopReason::Error);
    b.build()
}

fn auth() -> Script {
    let mut b = ScriptBuilder::new();
    b.notice(NoticeLevel::Error, "登录已失效:请在终端运行 `claude` 并执行 /login 重新登录");
    b.stops(StopReason::Error);
    b.build()
}

fn usage_limits() -> Script {
    let mut b = ScriptBuilder::new();
    for (i, (ctx, five)) in [(90_000u64, 82.0f32), (120_000, 88.0), (160_000, 93.0), (188_000, 97.0)].into_iter().enumerate() {
        b.say(&format!("第 {} 轮:额度逐步逼近。", i + 1));
        b.usage(ctx, 200_000, Some(five), Some(35.0 + i as f32 * 3.0)).sleep(200);
    }
    b.build()
}

fn compact() -> Script {
    let mut b = ScriptBuilder::new();
    b.usage(190_000, 200_000, None, None);
    b.notice(NoticeLevel::Warning, "上下文接近上限,正在自动压缩…");
    b.sleep(800);
    b.usage(32_000, 200_000, None, None);
    b.notice(NoticeLevel::Info, "上下文已压缩:190k → 32k tokens");
    b.build()
}

fn modes() -> Script {
    let mut b = ScriptBuilder::new();
    for m in PermissionMode::ALL {
        b.mode(m).say(&format!("已切换到「{}」模式。", m.label())).sleep(300);
    }
    b.build()
}
