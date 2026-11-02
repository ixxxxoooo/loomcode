//! 会话流样式类场景。

use super::{Kind, Scenario};
use crate::script::*;
use serde_json::json;
use loom_core::*;

pub fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario { id: "roles", title: "角色样式:所有条目同屏", group: "会话流样式", kind: Kind::Auto, prompt: "把所有条目样式各展示一遍", build: roles },
        Scenario { id: "markdown", title: "Markdown 与代码高亮:发布说明", group: "会话流样式", kind: Kind::Auto, prompt: "写一份 v2.0 发布说明", build: markdown },
        Scenario { id: "markdown-edge", title: "Markdown 边界:嵌套、宽表格与回退", group: "会话流样式", kind: Kind::Auto, prompt: "测试 Markdown 的边界情况", build: markdown_edge },
        Scenario { id: "mermaid", title: "Mermaid 图表:架构文档", group: "会话流样式", kind: Kind::Auto, prompt: "画出整体架构", build: mermaid },
        Scenario { id: "mermaid-flow", title: "Mermaid 流程图:形状、方向、子图与样式", group: "会话流样式", kind: Kind::Auto, prompt: "把发布流程画成流程图,各种节点形状都用上", build: mermaid_flow },
        Scenario { id: "mermaid-seq", title: "Mermaid 时序图:激活、便签与循环框", group: "会话流样式", kind: Kind::Auto, prompt: "画出登录与刷新令牌的时序", build: mermaid_seq },
        Scenario { id: "mermaid-state", title: "Mermaid 状态图与饼图", group: "会话流样式", kind: Kind::Auto, prompt: "画出任务状态机和耗时占比", build: mermaid_state },
        Scenario { id: "mermaid-edge", title: "Mermaid 边界:不支持的类型、语法错误与未闭合", group: "会话流样式", kind: Kind::Auto, prompt: "测试 Mermaid 的边界情况", build: mermaid_edge },
        Scenario { id: "chart", title: "内联图表:折线与 K 线", group: "会话流样式", kind: Kind::Auto, prompt: "画一下近 30 天的访问量", build: chart },
        Scenario { id: "tool-runs", title: "工具调用折叠:读 / 搜 / 改 / 跑混合的一轮", group: "会话流样式", kind: Kind::Auto, prompt: "帮我把 router 里的大小写匹配问题修一下", build: tool_runs },
        Scenario { id: "thinking-time", title: "思考用时:已思考 N 秒与回复操作条", group: "会话流样式", kind: Kind::Auto, prompt: "分析一下这个方案的利弊", build: thinking_time },
        Scenario { id: "tool-runs-live", title: "工具调用折叠:有失败与未结束(默认展开)", group: "会话流样式", kind: Kind::Frozen, prompt: "跑一下完整的检查", build: tool_runs_live },
        Scenario { id: "working-state", title: "处理中:工具跑完后等模型继续", group: "会话流样式", kind: Kind::Frozen, prompt: "继续", build: working_state },
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
    b.say("# v2.0 发布说明\n\n## 亮点\n- **全新**界面\n- 性能提升 *3×*\n- [x] 支持暗色\n- [ ] 插件市场(开发中)\n\n## 对比\n\n| 指标 | v1 | v2 |\n| --- | ---: | ---: |\n| 启动 | 1.2s | 0.4s |\n| 内存 | 320MB | 110MB |\n\n> 提示:升级前请备份配置。\n\n### 代码\n```rust\nfn main() {\n    println!(\"Hello, LoomCode!\");\n}\n```\n\n```python\ndef greet(name: str) -> str:\n    return f\"你好,{name}\"\n```\n\n```bash\ncargo install loomcode\n```\n");
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

fn mermaid_flow() -> Script {
    let mut b = ScriptBuilder::new();
    b.say("## 发布流程(自上而下,含分支、回环与子图)\n\n```mermaid\nflowchart TD\n  A([提交代码]) --> B{CI 通过?}\n  B -- 是 --> C[[构建产物]]\n  B -- 否 --> D[修复问题]\n  D --> A\n  subgraph 发布阶段\n    C --> E[(上传制品库)]\n    E --> F{{灰度 10%}}\n    F -->|指标正常| G((全量))\n    F -.->|异常| H[/回滚/]\n  end\n  G ==> I>完成]\n```\n");
    b.say("## 从左到右 + 自定义样式 + 各种连线\n\n```mermaid\ngraph LR\n  A[需求] --> B[设计] --- C[开发]\n  C -.-> D[测试]\n  D ==> E[上线]\n  E --o F[监控]\n  F --x G[下线]\n  C <--> D\n  A & B --> H[评审]\n  style A fill:#f9d,stroke:#c36\n  classDef good fill:#2d7,stroke:#183,color:#fff\n  class E,F good\n```\n");
    b.say("## 其余方向与特殊情况\n\n```mermaid\nflowchart RL\n  X[\"带引号 (括号) 的标签\"] --> Y[第一行<br/>第二行]\n  Y --> Y\n  Y --> Z[(终点)]\n```\n\n```mermaid\nflowchart BT\n  底层 --> 中层 --> 顶层\n  底层 --> 顶层\n```\n");
    b.build()
}

fn mermaid_seq() -> Script {
    let mut b = ScriptBuilder::new();
    b.say("## 登录与刷新令牌\n\n```mermaid\nsequenceDiagram\n  title 登录流程\n  autonumber\n  actor U as 用户\n  participant W as 网页\n  participant S as 服务端\n  U->>W: 输入账号密码\n  W->>+S: POST /login\n  S-->>-W: 令牌\n  Note over W,S: 令牌 15 分钟过期\n  loop 每 10 分钟\n    W->>S: 刷新令牌\n    alt 刷新成功\n      S-->>W: 新令牌\n    else 已失效\n      S--xW: 401\n      W-)U: 要求重新登录\n    end\n  end\n  S->>S: 写审计日志\n  Note right of S: 保留 90 天\n  Note left of U: 用户视角\n```\n");
    b.say("## 并行与可选\n\n```mermaid\nsequenceDiagram\n  participant A\n  participant B\n  participant C\n  par 并行请求\n    A->>B: 查询库存\n  and\n    A->>C: 查询价格\n  end\n  opt 有优惠券\n    A->>C: 校验优惠券\n  end\n  activate A\n  B-->>A: 完成\n  deactivate A\n```\n");
    b.build()
}

fn mermaid_state() -> Script {
    let mut b = ScriptBuilder::new();
    b.say("## 任务状态机\n\n```mermaid\nstateDiagram-v2\n  [*] --> 待处理\n  待处理 --> 进行中 : 开始\n  进行中 --> 已完成 : 通过评审\n  进行中 --> 待处理 : 退回\n  state \"已取消(终态)\" as 取消\n  待处理 --> 取消\n  已完成 --> [*]\n  取消 --> [*]\n  note right of 进行中\n    同时最多 3 个\n  end note\n```\n");
    b.say("## 耗时占比\n\n```mermaid\npie showData title 一次请求的耗时(ms)\n  \"渲染\" : 30\n  \"网络\" : 50\n  \"解析\" : 12\n  \"其他\" : 8\n```\n\n只有一项的饼图:\n\n```mermaid\npie title 全部\n  \"唯一\" : 1\n```\n");
    b.build()
}

fn mermaid_edge() -> Script {
    let mut b = ScriptBuilder::new();
    b.say("暂不支持的类型会回退为带原因的源码卡片:\n\n```mermaid\ngantt\n  title 排期\n  section 开发\n  编码 :a1, 2026-01-01, 7d\n```\n\n```mermaid\nclassDiagram\n  Animal <|-- Dog\n```\n\n语法错误:\n\n```mermaid\nflowchart TD\n  A --> \n  ???\n```\n\n饼图数值不是数字:\n\n```mermaid\npie\n  \"a\" : x\n```\n\n空内容:\n\n```mermaid\n```\n\n未闭合的围栏(按普通 Markdown 显示):\n\n```mermaid\nflowchart LR\n  A --> B");
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
    // 行数不多但每行很长(粘贴的 curl 命令):窄栏里折成很多行,也只显示固定高度的窗口
    b.user("不对啊。我下载的没有啊 curl --url 'https://example.com/api/chart/sql_data?logID=log_7253789162753282' \\\n  -H 'accept: application/json, text/plain, */*' \\\n  -H 'accept-language: zh-CN,zh;q=0.9,en;q=0.8' \\\n  -b '__debug__=1; SESSIONID=6d691bbcb4c8d13500edb4f74d9f4c4b1793083676; USERID=6c6977656e6a69616f; TOKEN=eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJhdWQiOiIyNDFmZDBhODFkN2M0YWUyOWZkODUzYTQxNzI5NWUzZCIsImlzcyI6Inp5YilsInN1Yil6Ijk5ODczMzIzNTgyNjYiLCJleHAiOjE3OTM3ODk3NjN9' \\\n  -H 'dnt: 1' \\\n  -H 'priority: u=1, i'");
    b.say("以上是各类用户消息的样式。");
    b.build()
}

/// 一轮真实感的对话:先思考,再连续读 / 搜 / 跑命令(折成摘要),再改文件,最后总结。
fn tool_runs() -> Script {
    let mut b = ScriptBuilder::new();
    b.think("先看看路由匹配的实现,再决定在哪一层做大小写折叠。");
    b.say("我先梳理一下路由层的结构。");
    b.tool("Read", "读取 src/router.rs", json!({}), vec![ToolContent::Text("pub fn match_route(path: &str) -> Option<&Route> { ... }".into())]);
    b.tool("Read", "读取 src/route_table.rs", json!({}), vec![ToolContent::Text("pub struct RouteTable { ... }".into())]);
    b.tool("Grep", "搜索 match_route", json!({}), vec![ToolContent::Text("src/router.rs:12\nsrc/server.rs:48".into())]);
    b.tool("Bash", "cargo test router", json!({}), vec![ToolContent::Terminal { command: "cargo test router".into(), output: "test router::case_sensitive ... FAILED".into(), exit_code: Some(0) }]);
    b.say("定位到了:`match_route` 直接用 `==` 比较路径。我把比较改成忽略 ASCII 大小写,并补一个测试。");
    b.tool("Edit", "编辑 src/router.rs", json!({}), vec![ToolContent::Diff { path: "src/router.rs".into(), old: "if route.path == path {".into(), new: "if route.path.eq_ignore_ascii_case(path) {".into() }]);
    b.tool("Write", "新建 tests/case_insensitive.rs", json!({}), vec![ToolContent::Text("#[test]\nfn matches_any_case() {}".into())]);
    b.tool("Bash", "cargo test", json!({}), vec![ToolContent::Terminal { command: "cargo test".into(), output: "test result: ok. 14 passed".into(), exit_code: Some(0) }]);
    b.say("改好了,全部测试通过:\n\n- `src/router.rs`:路径比较改为 `eq_ignore_ascii_case`\n- `tests/case_insensitive.rs`:新增大小写测试\n\n注意:查询串仍然区分大小写。");
    b.build()
}

/// 带失败、还有未结束工具的一段:默认展开,方便看到问题。
fn tool_runs_live() -> Script {
    let mut b = ScriptBuilder::new();
    b.say("开始检查。");
    b.tool("Read", "读取 Cargo.toml", json!({}), vec![ToolContent::Text("[workspace]".into())]);
    b.tool_fail("Bash", "cargo clippy", json!({}), vec![ToolContent::Terminal { command: "cargo clippy".into(), output: "error: this could be simplified".into(), exit_code: Some(1) }]);
    b.tool("Grep", "搜索 TODO", json!({}), vec![ToolContent::Text("src/lib.rs:3".into())]);
    b.tool_start(ToolCall::new("live-1", "Bash", "cargo test --workspace").with_input(json!({"command": "cargo test --workspace"})));
    b.hold();
    b.build()
}

/// 工具都结束了,回合还在进行:末尾显示「处理中」提示。
fn working_state() -> Script {
    let mut b = ScriptBuilder::new();
    b.say("我先读一下配置。");
    b.tool("Read", "读取 settings.json", json!({}), vec![ToolContent::Text("{}".into())]);
    b.tool("Read", "读取 Cargo.toml", json!({}), vec![ToolContent::Text("[package]".into())]);
    b.hold();
    b.build()
}

/// 实时思考:两段思考分别约 3 秒和 7 秒,结束后回复下面有操作条。
fn thinking_time() -> Script {
    let mut b = ScriptBuilder::new();
    b.think_for("先把方案拆成几个维度:成本、风险、可维护性,再逐个权衡。", 3000);
    b.say("先说结论:**值得做**,但要分两步走。");
    b.think_for("再想想落地顺序:先做可回滚的小步改动,验证没问题再扩大范围,最后清理旧实现。", 7000);
    b.say("具体建议:\n\n1. 先灰度一周\n2. 观察错误率\n3. 再全量切换");
    b.build()
}
