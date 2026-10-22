//! 会话流样式类场景。

use super::{Kind, Scenario};
use crate::script::*;
use serde_json::json;
use ucode_core::*;

pub fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario { id: "roles", title: "角色样式:所有条目同屏", group: "会话流样式", kind: Kind::Auto, prompt: "把所有条目样式各展示一遍", build: roles },
        Scenario { id: "markdown", title: "Markdown 与代码高亮:发布说明", group: "会话流样式", kind: Kind::Auto, prompt: "写一份 v2.0 发布说明", build: markdown },
        Scenario { id: "markdown-edge", title: "Markdown 边界:嵌套、宽表格与回退", group: "会话流样式", kind: Kind::Auto, prompt: "测试 Markdown 的边界情况", build: markdown_edge },
        Scenario { id: "mermaid", title: "Mermaid 图表:架构文档", group: "会话流样式", kind: Kind::Auto, prompt: "画出整体架构", build: mermaid },
        Scenario { id: "chart", title: "内联图表:折线与 K 线", group: "会话流样式", kind: Kind::Auto, prompt: "画一下近 30 天的访问量", build: chart },
        Scenario { id: "user-variants", title: "用户消息:图片、@文件与长文折叠", group: "会话流样式", kind: Kind::Auto, prompt: "展示各种用户消息", build: user_variants },
    ]
}

fn roles() -> Script {
    let mut b = ScriptBuilder::new();
    b.think("这是一段思考:先分析问题,再决定调用什么工具。");
    b.say("这是助手的正文,支持 **加粗**、*斜体*、`行内代码` 与 [链接](https://example.com)。");
    b.tool("Read", "读取 src/main.rs", json!({}), vec![ToolContent::Text("fn main() {}".into())]);
    b.todos("t", &[("进行中", TodoStatus::InProgress), ("待处理", TodoStatus::Pending), ("已完成", TodoStatus::Completed)]);
    b.notice(NoticeLevel::Info, "信息提示:已切换到 Sonnet");
    b.notice(NoticeLevel::Warning, "警告提示:上下文使用已超过 80%");
    b.notice(NoticeLevel::Error, "错误提示:请求失败");
    b.build()
}

fn markdown() -> Script {
    let mut b = ScriptBuilder::new();
    b.say("# v2.0 发布说明\n\n## 亮点\n- **全新**界面\n- 性能提升 *3×*\n- [x] 支持暗色\n- [ ] 插件市场(开发中)\n\n## 对比\n\n| 指标 | v1 | v2 |\n| --- | ---: | ---: |\n| 启动 | 1.2s | 0.4s |\n| 内存 | 320MB | 110MB |\n\n> 提示:升级前请备份配置。\n\n### 代码\n```rust\nfn main() {\n    println!(\"Hello, UCode!\");\n}\n```\n\n```python\ndef greet(name: str) -> str:\n    return f\"你好,{name}\"\n```\n\n```bash\ncargo install ucode\n```\n");
    b.build()
}

fn markdown_edge() -> Script {
    let mut b = ScriptBuilder::new();
    b.say("1. 第一层\n   - 第二层\n     - 第三层 `code`\n       1. 第四层\n\n> 引用里的列表:\n> - 条目 A\n> - 条目 B\n\n| a | b | c | d | e | f | g | h | i | j | k | l |\n| - | - | - | - | - | - | - | - | - | - | - | - |\n| 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 |\n\n超长单词:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n\n未闭合围栏:\n```js\nconsole.log('没有结束围栏');");
    b.build()
}

fn mermaid() -> Script {
    let mut b = ScriptBuilder::new();
    b.say("## 架构\n\n```mermaid\nflowchart LR\n  UI --> Session --> Claude\n  Session --> Store\n```\n\n```mermaid\nsequenceDiagram\n  User->>UI: 发送\n  UI->>Claude: stream-json\n  Claude-->>UI: 事件\n```\n\n```mermaid\npie title 耗时\n  \"渲染\" : 30\n  \"网络\" : 50\n  \"其他\" : 20\n```\n");
    b.build()
}

fn chart() -> Script {
    let mut b = ScriptBuilder::new();
    b.say("近 30 天访问量:\n\n```chart\n{\"type\":\"line\",\"title\":\"访问量\",\"data\":[120,132,101,134,190,230,210,260,240,300,280,320]}\n```\n\n某股 K 线:\n\n```chart\n{\"type\":\"candle\",\"title\":\"示例 K 线\",\"data\":[[10,12,9,11],[11,13,10,12],[12,12,8,9],[9,11,8,10]]}\n```\n");
    b.build()
}

fn user_variants() -> Script {
    let mut b = ScriptBuilder::new();
    b.user_rich("看看这张图", &["error.png"], &[]);
    b.user_rich("参考 @src/main.rs 和 @Cargo.toml 修改", &[], &["src/main.rs", "Cargo.toml"]);
    b.user("第一行\n第二行\n第三行\n第四行\n第五行\n第六行\n第七行(超过 6 行应自动折叠)\n第八行");
    b.user_rich("", &["a.png", "b.png", "c.png"], &[]);
    b.user("**不会被渲染成 Markdown** 的原样文本 `code`");
    b.user("中英混排 mixed text 12345 测试");
    b.say("以上是各类用户消息的样式。");
    b.build()
}
