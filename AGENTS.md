# LoomCode 开发约定

Rust + gpui-kit 0.8 的 Claude Code 工作台。只支持 Claude Code,直连本机 `claude` 的 stream-json 控制协议(不走 ACP、不依赖 Node)。

## 目录结构

| crate | 职责 |
| --- | --- |
| `loom-core` | 与界面无关的数据模型:时间线条目、`AgentEvent`/`AgentCommand`、`SessionState` 归约器、`AgentBackend` 抽象;`task`(后台任务 `BackgroundTask`/`TaskKind`/`TaskStatus`、停滞阈值、用时与 token 格式化) |
| `loom-diag` | 诊断底座(位于依赖树最底层,不依赖界面与 gpui):文件日志(预开句柄、5 MB × 5 份轮转、级别过滤,`info!`/`warn!`/`error!`/`debug!` 宏)、面包屑 `crumb!`、慢操作计时 `span`、崩溃分层记录(`crash`:panic 钩子、原生异常处理器、运行标记、SIGTERM 正常退出、下次启动归类补记)、系统 `.ips` 摘要导入(`ips`)、崩溃记录文件(`records`)、资源采样(`monitor`)、主线程卡死看门狗(`hang`)、界面构建耗时(`frames`)、计数指标(`stats`) |
| `loom-claude` | 真实后端:启动参数(含额外环境变量)、stream-json 翻译器、权限应答、历史读取与回放、`LazyBackend`、用量统计 `usage`(扫描 `~/.claude/projects` 带增量缓存) |
| `loom-demo` | 演示后端:脚本化场景(55 个,含 `showcase-all` 全场景会话)、后台任务模拟器(`BgJob`:任务独立于回合继续推进、可被 `StopTask` 停止)与沙盒项目;侧栏每个场景一个会话,`scenario-lab` 项目收纳其余场景;沙盒里的 `home/` 是演示用的假 `~/.claude` 与 `~/.claude.json`,设置页的读写都落在这里 |
| `loom-config` | `~/.loomcode/settings.json` 与 Claude 配置读写;`update`(应用更新:读 GitHub Release 的 appcast.json、下载校验 sha256、挂载暂存、退出后脚本替换重启):`claude`(用户 / 项目 / 本地三层的权限、MCP、钩子、env,原子写 + 备份)、`items`(命令、子 Agent、Skills、记忆文件)、`plugins`(插件与市场的读取、启停、CLI 参数校验)、`providers`(模型服务商与映射,密钥单独 0600 文件)、`diag`(诊断包、连通性、进程资源;日志本身已移到 `loom-diag`,这里只是薄封装) |
| `loom-canvas` | Canvas 画布文档模型、校验与存储 |
| `loom-automation` | 自动化的计划、存储与运行器 |
| `loom-browser` | 内置阅读模式浏览器的抓取与正文提取 |
| `loom-term` | 伪终端内核:`portable-pty` 起 shell + `alacritty_terminal` 解析,输出画面快照与按键编码(与界面无关) |
| `loom-app` | gpui-kit 界面(二进制名 `loomcode`);`app_icon.rs` 管 Dock 图标(随主题切浅/深色)与菜单栏托盘 |

图标源文件在 `assets/icon/`:`gen.py` 生成 SVG,`build.sh` 渲染出 PNG 与 `.icns`(产物已提交,改设计后重跑 `assets/icon/build.sh`)。

数据流:`AgentBackend` → `AgentEvent` → `SessionState::apply`(纯函数归约)→ `SessionModel` 实体 → 界面。界面只读 `SessionState`,所有交互都转成 `AgentCommand` 发给后端,因此演示与真实后端共用同一套界面。

## 运行

```sh
cargo run -p loom-app -- --demo     # 演示模式(或 LOOM_DEMO=1):脚本化后端,沙盒项目
cargo run -p loom-app               # 真实模式:读 ~/.claude/projects,按需惰性启动 claude
scripts/package.sh [--dmg]           # 打包 dist/LoomCode.app(--dmg 另出 dist/LoomCode-<版本>.dmg)
scripts/publish.sh                  # 发布:先在根 Cargo.toml 提升版本并提交;打 tag、推送、gh 创建 Release(附 dmg 与 appcast.json)
```

调试用环境变量(只用于截图与验证):`LOOM_TASKS`(启动即展开输入框上方的任务坞;取值 agent / shell / monitor / workflow / other / finished 指定展开哪一类,其余值取子代理)、`LOOM_AUTOSWITCH=1`(启动 3 秒后自动切换演示/真实并重启,验证重启链路)、`LOOM_SESSION`、`LOOM_TAB`、`LOOM_FILE`、`LOOM_IDE`、`LOOM_PAGE`(canvas / automation / browser / customize)、`LOOM_URL`、`LOOM_SETTINGS`(取值见 `SettingsTab::key`:general / appearance / editor / terminal / trust 等 22 个)、`LOOM_PALETTE`、`LOOM_POPUP`(model / context / mode,输入框弹层)、`LOOM_THEME`(light / dark / system / one-light / one-dark)、`LOOM_DIALOG`(rename / workspace / switch-mode)、`LOOM_TERM_SELECT`、`LOOM_CHANGES`、`LOOM_CUSTOMIZE`(plugins / mcp / skills / commands / agents / rules / hooks,选中自定义页标签)、`LOOM_IDE_TERM`、`LOOM_WINDOW_SIZE`、`LOOM_PREFILL`、`LOOM_AUTOSEND`、`LOOM_TERM_CMD`、`LOOM_CANVAS_SKIP`、`LOOM_EDIT`(第几条用户消息,自动进入编辑)、`LOOM_DENSITY`(compact / standard / detailed,工具调用详细程度)、`LOOM_DEV`(开发版标识,由 `scripts/dev.sh` 设置)、`LOOM_LOG`(off / error / warn / info / debug,临时覆盖日志级别)、`LOOM_CRASH_TEST`(panic / segv / abort / kill / hang,启动 2 秒后主动制造故障,验证崩溃与卡死记录)、`LOOM_NO_CRASH_HANDLER=1`(关掉原生崩溃处理器,调试器附加时用)。截图:`scripts/shots.sh <场景id>`(输出 `/tmp/loom-<id>.png`);`scripts/shots-matrix.sh [场景id…]` 批量出「浅色 / 深色 / 窄窗 / 精简 / 详细」五个变体(`/tmp/loom-<id>-<变体>.png`)。

**演示 ↔ 真实一键切换**:命令面板「切换到…(重启)」、设置 → 关于的「运行模式」按钮、点击左下角徽章,都会弹确认框,确认后用新模式重启自己(`mode_switch.rs`),始终只留一个实例。启动模式优先级:`--demo` / `--real` / `LOOM_DEMO=1|0` > 开发版(`LOOM_DEV=1`)记住的上次选择(`~/.loomcode/dev-mode`)> 真实;`scripts/dev.sh [--demo|--real]` 不带开关时沿用记忆,重启后 `target/dev.pid` 会被更新。

## 开发流程规则(每次改动都必须执行)

1. **改完即提交**:每完成一处修改,先 `cargo test --workspace` 通过,再把全部改动(含文档、脚本)`git add` 并提交,不留未提交的改动。一个功能一个提交,提交信息用中文 Conventional Commits。
2. **改完即重启开发版**:提交后运行 `scripts/dev.sh`(演示模式加 `--demo`),让用户立刻看到效果。
3. **始终只保留一个开发版实例**:脚本先编译,成功后杀掉所有旧的开发版(pid 文件 `target/dev.pid` + 按 `target/debug/loomcode` 路径兜底),再启动新实例;不要用 `cargo run` 另起实例,也不要动 `dist/LoomCode.app` 的打包版。
4. **开发版有标识**:开发版以 `LOOM_DEV=1` 启动,左下角设置按钮右侧(IDE 窗口在底部状态栏最左)显示主题色徽章:演示模式显示「演示模式」,真实模式的开发版显示「开发版」,一次只显示一个;正式版 / 打包版的真实模式不显示。日志在 `target/dev.log`。
5. 编译失败时旧开发版保持运行,先修好再重试,不要留下半成品状态。

## 约定

- 回答、注释、文档一律中文;提交信息用中文 Conventional Commits(`feat:`、`fix:`…),一个功能一个提交。
- 新增时间线条目类型必须同时补演示场景(`loom-demo/src/scenarios`)。
- 不读取、不输出、不提交任何密钥或钥匙串内容;环境变量只展示名称,值遮盖。
- 不提交构建产物(`target`、`dist`)。
- 在 `loom-app` 里 `use gpui_kit::*` 会遮蔽内置 `#[test]`,单元测试请写 `#[::core::prelude::v1::test]`;其他 crate 用普通 `#[test]`。
- 后台任务与子 Agent(`loom-core::task` + `session.rs` 的 `task_*` 归约):`task_notification` 是终态的权威信号;`background_tasks_changed` 是存活集合的全量快照(不在其中的运行中任务视为已停止);前台子 Agent 才会随取消变「已停止」,后台的继续跑;子 Agent 的结果写在自己的卡片上,不再额外插通知行,后台命令 / 监听才有 `TaskNote` 通知行。界面:输入框上方「任务坞」(`task_dock.rs`:每个类别一个胶囊排成一排(子代理 / 后台命令 / 监听 / 工作流 / 其他 / 已结束,由 `loom-core::task_group` 分组),固定不动;点哪个胶囊就在上方悬浮展开那一类的列表,再点同一个胶囊 / 点外部 / Esc 收起,点另一个直接切换;`task_ui.rs` 放公共样式与通知行,演示场景 `bg-dock` 覆盖全部类别)、右侧「任务」标签(`tasks_tab.rs`,输出读 `output_file` 尾部)、连续子 Agent 的「并行 N 个」汇总行(`agent_group.rs`)、子 Agent 卡片 v2(用时 / 工具数 / token / 当前动作 / 摘要 / 停止 / 停滞提示,失败默认展开)。后台 / 全场景演示会话按真实节奏播放(`workbench.rs` 里的 `live`),其余历史会话瞬时回放。
- 设置页分四组(客户端 / Claude Code / 工作台 / 其他),分类枚举与导航在 `settings/mod.rs`、`settings/nav.rs`;编辑器 / 终端 / 浏览器 / 源代码管理 / 代码智能偏好存在 `AppSettings` 里,界面用全局 `crate::prefs(cx)` 读取。每个项目的 IDE 标签记忆存 `~/.loomcode/state.json` 的 `ide`(`ide_memory.rs`);迷你地图的纯模型在 `minimap_model.rs`。
- Agent 窗口右侧是一个可折叠面板,以动态标签呈现(`right_tabs.rs`:改动/文件/终端/计划/输出可开关,「+」新增;「文件」标签是左内容 + 右树的工作区,树里单击原地预览,`files_workspace.rs`);收起时有快捷竖条(`right_rail.rs`);「终端」分段允许多个终端标签(`term_store.rs` 按项目根分组、各窗口共用,标签条在 `term_panel.rs`,Claude 的命令通过工具卡片的「在终端面板查看」为单条命令开只读标签);IDE 编辑器支持多标签(逻辑在 `ide_tabs.rs`,参考 Zed Pane:预览标签、就近插入、关闭回到最近使用)。
- **日志与诊断**(`loom-diag`):所有 crate 一律用 `loom_diag::{error!, warn!, info!, debug!}` 记日志,**不再用 `eprintln!`**;日志在 `~/.loomcode/logs/loomcode.log`(设置 → 诊断可调级别,环境变量 `LOOM_LOG` 优先)。崩溃分层:① panic 钩子写 `~/.loomcode/crashes/<时间>-<序号>-<类别>-<pid>.json`(含调用栈、面包屑、日志尾部);② 原生异常处理器(`crash-handler`,Mach 异常 + SIGABRT)只用预开的 fd 与栈缓冲写 `~/.loomcode/run/<pid>.native`,再链回系统处理器,所以 macOS 的 `.ips` 照常产生;③ 运行标记 `run/<pid>.json` 定时心跳,正常退出(⌘Q、托盘退出、更新重启、模式切换、SIGTERM / SIGINT / SIGHUP)才删除,下次启动扫到残留就归类(panic / 原生崩溃 / 卡死 / 无线索如 kill -9)并补一条记录;④ 启动后导入 `~/Library/Logs/DiagnosticReports/loomcode-*.ips` 的摘要(去重)。看门狗:界面线程每 100 ms 心跳(`hang::beat`),停滞超过阈值记 WARN,更久则用 `/usr/bin/sample` 抓栈到 `crashes/hang-*.txt` 并写 `hang` 记录。资源采样每秒一次(内存、CPU,留 5 分钟曲线)。新增「正常退出」路径(调用 `cx.quit()` 之前)必须先调 `loom_diag::crash::clean_exit(原因)`,否则下次启动会被误判为崩溃。
- **不要在工作台更新期间回读它**:`Workbench` 的方法、`Workbench::new`、`IdeWindow::new` 里创建子视图 / 调用子实体方法时,子实体不能再 `wb.read(cx)` / `wb.update(cx)`(重复借用会 panic,在 `extern "C"` 回调里直接 abort);需要的数据由调用方作为参数传入,或用 `cx.defer` 推迟。`ide_window.rs` 里有源码级回归测试。
- **编辑器与终端全工作台唯一**:`Workbench.editor`(`Entity<IdeView>`)是唯一的编辑器,Agent 右栏「文件」标签(`files_workspace.rs`)、融合 IDE、独立 IDE 窗口都嵌入它,标签 / 预览标签 / 面包屑 / Markdown / 迷你地图 / 导航历史只有一份实现;项目切换与标签记忆由 `Workbench::sync_editor_root`、`save_editor_memory` 统一处理;打开文件统一走 `Workbench::open_file` / `open_in_editor`。独立 IDE 窗口开着时 Agent「文件」标签只放占位(不让同一个编辑器在两个窗口同时渲染)。新建任何文本编辑器一律用 `ide_prefs::new_editor`(偏好一致)。终端由全局 `ProjectTerminals` 按**规范化的项目根**(`term_store::normalize_root`)分组,Agent 与 IDE 共用;标签样式在 `term_tabs.rs`;同一终端被两个窗口同时显示时,只听 `claim_size` 认领的窗口的尺寸;切换运行模式的确认框会提示正在运行命令的终端(`Terminal::is_busy`)。
- 编译零警告;改动后跑 `cargo test --workspace`。

## Claude 协议要点(已对真实 claude 2.1.x 验证)

- 初始化:`initialize` 带 `perTaskStopAffordance`(否则中断会杀掉后台任务)、`agentProgressSummaries`、`forwardSubagentText`;`stop_task {task_id}` 停任务,`background_tasks` 查存活任务;`system` 的 `task_started / task_progress / task_updated / task_notification / background_tasks_changed` 见 `translate.rs`(`tool_progress` 心跳刻意忽略);后台 Bash 的 `toolUseResult.backgroundTaskId` 与后台 Agent 的 `async_launched` 都只表示「已启动」,不是完成。
- 参数:`--output-format stream-json --verbose --input-format stream-json --include-partial-messages --permission-prompt-tool=stdio`,另按需加 `--resume`/`--session-id`/`--model`/`--permission-mode`/`--effort`。
- 握手:发 `initialize` 控制请求,响应里有模型列表与当前权限模式;`system init` 要等第一条提示之后才出现。
- 权限:`can_use_tool` 在 `assistant` 的 `tool_use` 之后到达,按 `tool_use_id` 对应;`AskUserQuestion` 的答案写进 `updatedInput.answers`;`ExitPlanMode` 批准时回传建议的 `updatedPermissions`。
- claude 不回显用户输入,后端自己把提示追加进时间线。
- 历史在 `~/.claude/projects/<编码后的 cwd>/*.jsonl`,目录名把所有非字母数字字符替换成 `-`(有损,真实 cwd 取自会话内容)。
