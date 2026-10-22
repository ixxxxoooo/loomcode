//! 性能实验室场景:长会话与混合内容。

use super::{Kind, Scenario};
use crate::script::*;
use serde_json::json;
use ucode_core::*;

pub fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario { id: "perf-long-2000", title: "2000 轮长会话:滚动、加载与跨叶子复制", group: "性能实验室", kind: Kind::Auto, prompt: "生成 2000 轮长会话", build: long_2000 },
        Scenario { id: "heavy-content", title: "重内容压测:长代码、2000 行输出与大 Diff", group: "性能实验室", kind: Kind::Auto, prompt: "重内容压测", build: heavy },
    ]
}

fn long_2000() -> Script {
    let mut b = ScriptBuilder::new();
    for i in 1..=2000u32 {
        b.user(&format!("第 {i} 个问题:这个函数是做什么的?"));
        b.append(TimelineItem::Assistant { id: format!("perf-a-{i}"), text: format!("第 {i} 个回答:这个函数负责处理第 {i} 号任务,**返回结果**后写入缓存。"), streaming: false });
        if i % 10 == 0 {
            b.append(TimelineItem::Tool(ToolCall {
                status: ToolStatus::Completed,
                content: vec![ToolContent::Terminal { command: format!("echo {i}"), output: format!("{i}"), exit_code: Some(0) }],
                ..ToolCall::new(&format!("perf-t-{i}"), "Bash", &format!("echo {i}"))
            }));
        }
    }
    b.build()
}

fn heavy() -> Script {
    let mut b = ScriptBuilder::new();
    let code: String = (1..=60).map(|i| format!("fn func_{i}(x: i32) -> i32 {{\n    x + {i}\n}}\n\n")).collect();
    b.say(&format!("```rust\n{code}```"));
    let out: String = (1..=2000).map(|i| format!("\u{1b}[32m[{i:04}]\u{1b}[0m 编译单元 {i}\n")).collect();
    b.tool("Bash", "构建(2000 行输出)", json!({}), vec![ToolContent::Terminal { command: "make".into(), output: out, exit_code: Some(0) }]);
    let old: String = (1..=220).map(|i| format!("line {i}\n")).collect();
    let new: String = (1..=220).map(|i| if i % 7 == 0 { format!("LINE {i} (changed)\n") } else { format!("line {i}\n") }).collect();
    b.tool("Edit", "编辑 big.txt(220 行 Diff)", json!({}), vec![ToolContent::Diff { path: "big.txt".into(), old, new }]);
    b.build()
}
