# UCode 开发约定

Rust + gpui-kit 0.8 的 Claude Code 工作台。只支持 Claude Code,直连本机 `claude` 的 stream-json 控制协议(不走 ACP、不依赖 Node)。

## 目录结构

| crate | 职责 |
| --- | --- |
| `ucode-core` | 与界面无关的数据模型:时间线条目、`AgentEvent`/`AgentCommand`、`SessionState` 归约器、`AgentBackend` 抽象 |
| `ucode-claude` | 真实后端:启动参数、stream-json 翻译器、权限应答、历史读取与回放、`LazyBackend` |
| `ucode-demo` | 演示后端:脚本化场景(36 个)与沙盒项目 |
| `ucode-config` | `~/.ucode/settings.json` 与 Claude 配置(权限规则、MCP、Hooks)读写 |
| `ucode-canvas` | Canvas 画布文档模型、校验与存储 |
| `ucode-automation` | 自动化的计划、存储与运行器 |
| `ucode-browser` | 内置阅读模式浏览器的抓取与正文提取 |
| `ucode-app` | gpui-kit 界面(二进制名 `ucode`) |

数据流:`AgentBackend` → `AgentEvent` → `SessionState::apply`(纯函数归约)→ `SessionModel` 实体 → 界面。界面只读 `SessionState`,所有交互都转成 `AgentCommand` 发给后端,因此演示与真实后端共用同一套界面。

## 运行

```sh
cargo run -p ucode-app -- --demo     # 演示模式(或 UCODE_DEMO=1):脚本化后端,沙盒项目
cargo run -p ucode-app               # 真实模式:读 ~/.claude/projects,按需惰性启动 claude
scripts/package.sh [--dmg]           # 打包 dist/UCode.app
```

调试用环境变量(只用于截图与验证):`UCODE_SESSION`、`UCODE_TAB`、`UCODE_FILE`、`UCODE_IDE`、`UCODE_PAGE`、`UCODE_URL`、`UCODE_SETTINGS`、`UCODE_PALETTE`、`UCODE_THEME`、`UCODE_PREFILL`、`UCODE_AUTOSEND`、`UCODE_TERM_CMD`、`UCODE_CANVAS_SKIP`。截图:`scripts/shots.sh <场景id>`(输出 `/tmp/ucode-<id>.png`)。

## 约定

- 回答、注释、文档一律中文;提交信息用中文 Conventional Commits(`feat:`、`fix:`…),一个功能一个提交。
- 新增时间线条目类型必须同时补演示场景(`ucode-demo/src/scenarios`)。
- 不读取、不输出、不提交任何密钥或钥匙串内容;环境变量只展示名称,值遮盖。
- 不提交构建产物(`target`、`dist`)。
- 在 `ucode-app` 里 `use gpui_kit::*` 会遮蔽内置 `#[test]`,单元测试请写 `#[::core::prelude::v1::test]`;其他 crate 用普通 `#[test]`。
- 不做多标签:右侧只有一个固定可折叠面板,用分段切换(改动/文件/终端/计划/输出)。
- 编译零警告;改动后跑 `cargo test --workspace`。

## Claude 协议要点(已对真实 claude 2.1.x 验证)

- 参数:`--output-format stream-json --verbose --input-format stream-json --include-partial-messages --permission-prompt-tool=stdio`,另按需加 `--resume`/`--session-id`/`--model`/`--permission-mode`/`--effort`。
- 握手:发 `initialize` 控制请求,响应里有模型列表与当前权限模式;`system init` 要等第一条提示之后才出现。
- 权限:`can_use_tool` 在 `assistant` 的 `tool_use` 之后到达,按 `tool_use_id` 对应;`AskUserQuestion` 的答案写进 `updatedInput.answers`;`ExitPlanMode` 批准时回传建议的 `updatedPermissions`。
- claude 不回显用户输入,后端自己把提示追加进时间线。
- 历史在 `~/.claude/projects/<编码后的 cwd>/*.jsonl`,目录名把所有非字母数字字符替换成 `-`(有损,真实 cwd 取自会话内容)。
