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
| `loom-sheet` | 表格文件(csv / tsv / xlsx / xls / ods)的读取与浏览逻辑(与界面无关):CSV 解析(引号 / 转义 / 分隔符识别 / BOM / GBK 等编码识别)、`calamine` 读工作簿(按需读单张表)、列筛选与排序(`FilterState`,首行可作表头)、内容搜索;上限 10 万行 × 512 列 |
| `loom-canvas` | Canvas 画布文档模型、校验与存储 |
| `loom-automation` | 自动化的计划、存储与运行器 |
| `loom-browser` | 内置阅读模式浏览器的抓取与正文提取 |
| `loom-lsp` | 语言服务器(LSP)客户端内核(不依赖 gpui):`framing`(Content-Length 帧)、`client`(读 / 写线程、按 id 配对请求、通知分发、服务端请求自动应答、`$/progress` 进度、`initialize` 握手与能力判断)、`hub`(按「项目根 + 服务器 id」复用,首次使用才启动,失败结果缓存;诊断经全局回调送出;`kill_all` 退出时结束全部)、`servers`(按扩展名选服务器:rust-analyzer / Pyright / gopls / SourceKit-LSP / clangd / typescript-language-server;在 PATH 与常见目录找可执行文件,rust-analyzer 优先找 rustup 工具链里的真实文件以绕开 `~/.cargo/bin` 代理;`project_root`)、`uri`、`position`(UTF-16 ↔ 字符列换算) |
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

调试用环境变量(只用于截图与验证):`LOOM_TASKS`(启动即展开输入框上方的任务坞;取值 agent / shell / monitor / workflow / other / finished 指定展开哪一类,其余值取子代理)、`LOOM_AUTOSWITCH=1`(启动 3 秒后自动切换演示/真实并重启,验证重启链路)、`LOOM_SESSION`、`LOOM_BG_SESSION`(演示模式:先在后台打开该会话让它播放,再切到 `LOOM_SESSION`,用于验证后台会话不连带重绘)、`LOOM_TAB`、`LOOM_FILE`、`LOOM_IDE`、`LOOM_PAGE`(canvas / automation / browser / customize)、`LOOM_URL`、`LOOM_SETTINGS`(取值见 `SettingsTab::key`:general / appearance / editor / terminal / trust 等 22 个)、`LOOM_PALETTE`、`LOOM_POPUP`(model / context / mode,输入框弹层)、`LOOM_THEME`(light / dark / system / one-light / one-dark)、`LOOM_DIALOG`(rename / workspace / switch-mode)、`LOOM_TERM_SELECT`、`LOOM_CHANGES`、`LOOM_CUSTOMIZE`(plugins / mcp / skills / commands / agents / rules / hooks,选中自定义页标签)、`LOOM_IDE_TERM`、`LOOM_WINDOW_SIZE`、`LOOM_PREFILL`、`LOOM_REFS`(逗号分隔的路径,启动即在输入框正文里插入文件引用标记)、`LOOM_AUTOSEND`、`LOOM_TERM_CMD`、`LOOM_CANVAS_SKIP`、`LOOM_EDIT`(第几条用户消息,自动进入编辑)、`LOOM_DENSITY`(compact / standard / detailed,工具调用详细程度)、`LOOM_DEV`(开发版标识,由 `scripts/dev.sh` 设置)、`LOOM_LOG`(off / error / warn / info / debug,临时覆盖日志级别)、`LOOM_CRASH_TEST`(panic / segv / abort / kill / hang,启动 2 秒后主动制造故障,验证崩溃与卡死记录)、`LOOM_NO_CRASH_HANDLER=1`(关掉原生崩溃处理器,调试器附加时用)、`LOOM_NOTIFY_TEST=1`(启动 4 秒后发一条系统通知,验证通知图标与归属)。截图:`scripts/shots.sh <场景id>`(输出 `/tmp/loom-<id>.png`);`scripts/shots-matrix.sh [场景id…]` 批量出「浅色 / 深色 / 窄窗 / 精简 / 详细」五个变体(`/tmp/loom-<id>-<变体>.png`)。

**演示 ↔ 真实一键切换**:命令面板「切换到…(重启)」、设置 → 关于的「运行模式」按钮、点击左下角徽章,都会弹确认框,确认后用新模式重启自己(`mode_switch.rs`),始终只留一个实例。启动模式优先级:`--demo` / `--real` / `LOOM_DEMO=1|0` > 开发版(`LOOM_DEV=1`)记住的上次选择(`~/.loomcode/dev-mode`)> 真实;`scripts/dev.sh [--demo|--real]` 不带开关时沿用记忆,重启后 `target/dev.pid` 会被更新。

## 开发流程规则(每次改动都必须执行)

1. **改完即提交并推送 main**:每完成一处修改,先 `cargo test --workspace` 通过,再把全部改动(含文档、脚本)`git add` 并提交,随后必须 `git push origin main` 推送到远端 main,不留未提交、未推送的改动。一个功能一个提交,提交信息用中文 Conventional Commits。
2. **改完即重启开发版**:提交后运行 `scripts/dev.sh`(演示模式加 `--demo`),让用户立刻看到效果。
3. **始终只保留一个开发版实例**:脚本先编译,成功后杀掉所有旧的开发版(pid 文件 `target/dev.pid` + 按 `target/debug/loomcode`、`LoomCode Dev.app/Contents/MacOS/loomcode` 路径兜底),再启动新实例;不要用 `cargo run` 另起实例,也不要动 `dist/LoomCode.app` 的打包版。
4. **开发版跑在最小 `.app` 包里**(`target/dev/LoomCode Dev.app`,bundle id `dev.loomcode.app.dev`,由 `scripts/dev.sh` 每次组装):裸二进制没有包,程序坞退出时图标会退回终端 / exec 图标、系统通知拿不到应用图标;开发版里 `updater::current_bundle()` 恒为 `None`(不会被更新替换)。
5. **开发版有标识**:开发版以 `LOOM_DEV=1` 启动,左下角设置按钮右侧(IDE 窗口在底部状态栏最左)显示主题色徽章:演示模式显示「演示模式」,真实模式的开发版显示「开发版」,一次只显示一个;正式版 / 打包版的真实模式不显示。日志在 `target/dev.log`。
6. 编译失败时旧开发版保持运行,先修好再重试,不要留下半成品状态。

## 约定

- 回答、注释、文档一律中文;提交信息用中文 Conventional Commits(`feat:`、`fix:`…),一个功能一个提交。
- 新增时间线条目类型必须同时补演示场景(`loom-demo/src/scenarios`)。
- 不读取、不输出、不提交任何密钥或钥匙串内容;环境变量只展示名称,值遮盖。
- 不提交构建产物(`target`、`dist`)。
- 在 `loom-app` 里 `use gpui_kit::*` 会遮蔽内置 `#[test]`,单元测试请写 `#[::core::prelude::v1::test]`;其他 crate 用普通 `#[test]`。
- 后台任务与子 Agent(`loom-core::task` + `session.rs` 的 `task_*` 归约):`task_notification` 是终态的权威信号;`background_tasks_changed` 是存活集合的全量快照(不在其中的运行中任务视为已停止);前台子 Agent 才会随取消变「已停止」,后台的继续跑;子 Agent 的结果写在自己的卡片上,不再额外插通知行,后台命令 / 监听才有 `TaskNote` 通知行。界面:输入框上方「任务坞」(`task_dock.rs`:每个类别一个胶囊排成一排(子代理 / 后台命令 / 监听 / 工作流 / 其他 / 已结束,由 `loom-core::task_group` 分组),固定不动;点哪个胶囊就在上方悬浮展开那一类的列表,再点同一个胶囊 / 点外部 / Esc 收起,点另一个直接切换;`task_ui.rs` 放公共样式与通知行,演示场景 `bg-dock` 覆盖全部类别)、右侧「任务」标签(`tasks_tab.rs`,输出读 `output_file` 尾部)、连续子 Agent 的「并行 N 个」汇总行(`agent_group.rs`)、子 Agent 卡片 v2(用时 / 工具数 / token / 当前动作 / 摘要 / 停止 / 停滞提示,失败默认展开)。后台 / 全场景演示会话按真实节奏播放(`workbench.rs` 里的 `live`),其余历史会话瞬时回放。
- 设置页分四组(客户端 / Claude Code / 工作台 / 其他),分类枚举与导航在 `settings/mod.rs`、`settings/nav.rs`;编辑器 / 终端 / 浏览器 / 源代码管理 / 代码智能偏好存在 `AppSettings` 里,界面用全局 `crate::prefs(cx)` 读取。每个项目的 IDE 标签记忆存 `~/.loomcode/state.json` 的 `ide`(`ide_memory.rs`);迷你地图的纯模型在 `minimap_model.rs`(色块按「语法色 + 字形墨量档位」合并:`Weight::{Light,Normal,Heavy}`,标点淡、字母适中、大写与符号 / 中日韩浓),视图 `ide_minimap.rs`(视口首行 / 行数取连续浮点值:滚动偏移 ÷ 行高,在画布 paint 闭包里读编辑器,整体偏移对齐设备像素,所以滚动时与编辑器同帧、连续移动不闪)另画选区色带、诊断(红 / 黄)刻度、Git 刻度与光标刻度。编辑器库自带淡色缩进线,`ide_guides.rs` 在光标所在缩进块上叠一条更亮的当前块高亮线(纯函数 `active_guide` 只看缩进判块:块头行取子块,线画在上一级缩进列,不含头尾两行;叠层定位同 `ide_blame` 行内追溯,取自 `range_to_bounds`)。括号:`pair_scan.rs` 做配对扫描(按语言规则跳过字符串 / 注释 / Rust 字符字面量,栈配对并记深度);`ide_pairs.rs` 做配对竖线(扫描结果按「文本长度 + 编辑序号」缓存,超过 512 KB 不处理;竖线是盖住编辑区的画布,跨行配对画在开括号所在行的缩进列,包住光标的一对更亮,对标 VS Code `bracketPairs` 参考线,无第三方库、Zed 也没有);`ide_rainbow.rs` 做括号字符着色(对标 Zed `colorize_brackets`:包一层 tree-sitter 高亮器,在样式段里把括号字符按深度改成金 / 紫 / 蓝 / 青字色,深浅色各一套,开关或深浅色变化时由 `IdeView` 渲染里 `sync` 报告并 `defer_in` 重装高亮工厂)。能包高亮器依赖 `vendor/gpui-component`(补丁版,仅把 `input_highlighter_factory` 改为 pub,见 `vendor/README.md`,根 `Cargo.toml` 的 `[patch.crates-io]`)。光标行背景与行号色在 `theme.rs` 的高亮主题里(`editor.active_line.background` / `editor.active_line_number`,库自己绘制)。偏好 `editor.bracket_colors` / `bracket_guides`(默认开)。
- Agent 窗口右侧是一个可折叠面板,以动态标签呈现(`right_tabs.rs`:改动/文件/终端/计划/输出可开关,「+」新增;「文件」标签是左内容 + 右树的工作区,树里单击原地预览,`files_workspace.rs`);收起时有快捷竖条(`right_rail.rs`);「终端」分段允许多个终端标签(`term_store.rs` 按项目根分组、各窗口共用,标签条在 `term_panel.rs`,Claude 的命令通过工具卡片的「在终端面板查看」为单条命令开只读标签);IDE 编辑器支持多标签(逻辑在 `ide_tabs.rs`,参考 Zed Pane:预览标签、就近插入、关闭回到最近使用;标签条放不下时横向滚动,`tabs_scroll` 句柄 + 当前标签 / 标签数变化时 `scroll_to_item` 保证焦点标签可见,竖向滚轮经 `tab_style::wheel_to_horizontal` 换成横向;右栏标签条同理)。
- **日志与诊断**(`loom-diag`):所有 crate 一律用 `loom_diag::{error!, warn!, info!, debug!}` 记日志,**不再用 `eprintln!`**;日志在 `~/.loomcode/logs/loomcode.log`(设置 → 诊断可调级别,环境变量 `LOOM_LOG` 优先)。崩溃分层:① panic 钩子写 `~/.loomcode/crashes/<时间>-<序号>-<类别>-<pid>.json`(含调用栈、面包屑、日志尾部);② 原生异常处理器(`crash-handler`,Mach 异常 + SIGABRT)只用预开的 fd 与栈缓冲写 `~/.loomcode/run/<pid>.native`,再链回系统处理器,所以 macOS 的 `.ips` 照常产生;③ 运行标记 `run/<pid>.json` 定时心跳,正常退出(⌘Q、托盘退出、更新重启、模式切换、SIGTERM / SIGINT / SIGHUP)才删除,下次启动扫到残留就归类(panic / 原生崩溃 / 卡死 / 无线索如 kill -9)并补一条记录;④ 启动后导入 `~/Library/Logs/DiagnosticReports/loomcode-*.ips` 的摘要(去重)。看门狗:界面线程每 100 ms 心跳(`hang::beat`),停滞超过阈值记 WARN,更久则用 `/usr/bin/sample` 抓栈到 `crashes/hang-*.txt` 并写 `hang` 记录。资源采样每秒一次(内存、CPU,留 5 分钟曲线)。新增「正常退出」路径(调用 `cx.quit()` 之前)必须先调 `loom_diag::crash::clean_exit(原因)`,否则下次启动会被误判为崩溃。
- **fork 与 libc 时间函数互斥**:`localtime_r` 等 libc 函数内部会 `os_once` 初始化 libnotify,若此刻别的线程正在 `fork`(起终端子进程),子进程会在 exec 前以 "os_once_t is corrupt" 崩溃。所以日志时间戳不再每次调 libc(缓存时区偏移,每小时在读锁下刷新,公历换算自己算),所有会 `fork` 的启动代码(目前是 `Terminal::spawn`)要持有 `loom_diag::fork_guard()`;新增 fork / 子进程启动路径同理。
- **源代码管理(对标 VS Code SCM)**:`loom-core/git_scm.rs`(暂存 / 取消暂存 / 放弃 / 提交 / 修改上次提交 / 拉取 / 推送 / 同步 / 同步信息,全走 git CLI,`GIT_TERMINAL_PROMPT=0`,状态用 `--no-optional-locks`);`loom-app/scm_model.rs`(暂存 / 更改分组与树的纯模型)、`scm.rs`(`ScmView` 状态与操作,事件 `ScmEvent::{OpenFile(路径, 是否固定), Changed}`)、`scm_view.rs`(渲染)、`git_changes.rs`(右栏「Claude 改动 / Git 更改」切换)。**全工作台唯一**:`Workbench.scm`(`Entity<ScmView>`)由 Agent 右栏与 IDE 左栏(活动栏「源代码管理」,⌃⇧G)共同嵌入,与 `Workbench.editor` 同理;打开文件事件由 Workbench 订阅后走 `open_in_editor`。宿主在渲染时设置 `open_on_click`(IDE 单击开文件,Agent 右栏就地展开 diff)。演示数据:`loom-demo/workspace.rs` 的 `seed_scm_state` 给 `router-demo` 造出暂存 / 未暂存 / 未跟踪 / 删除等状态;`LOOM_SCM=list` 强制列表视图。**Git 图谱 / AI 提交说明 / 储藏(对标 OhMyAgent)**:`loom-core/git_log.rs`(提交列表解析、泳道布局 `layout`、提交涉及的文件、某版本的文件内容、储藏的增删应用、相对时间格式化)、`commit_msg.rs`(提交说明的提示词、素材收集 `collect`、清洗 `clean`、无 AI 时的启发式兜底);一次性 AI 通道在 `loom-claude/rewrite.rs` 的 `run_oneshot`(`claude -p --model haiku`,输入走 stdin,与提示词优化共用);界面在 `scm_graph.rs`(状态:每个项目一份提交列表、分页、仅当前分支开关、展开的提交)与 `scm_graph_view.rs`(「图形」分区:泳道画布 + 提交行 + 展开详情,提交框的星标按钮生成说明)。
- **Git 追溯(blame,对标 Zed inline blame / GitLens)**:`loom-core/git_blame.rs`(`git blame --porcelain -w`,未保存内容走 `--contents -`,结果 `Arc` 可跨线程;在文件所在目录按文件名追溯,不依赖仓库根路径写法)、`loom-app/ide_blame.rs`(行内追溯:`deferred(anchored())` 叠在光标行末尾,位置取自 `range_to_bounds`;追溯栏:行号左侧、每个可见行一个绝对定位标签,同一提交的连续行只在首行显示作者与时间,左侧 2px 色条按提交哈希取色;自动换行时不显示追溯栏)。数据存在 `IdeDoc.blame`,打开 / 保存立即刷新,编辑后 0.7 秒防抖刷新,刷新前 `blame_fresh=false`(行内隐藏、追溯栏变淡,避免错位)。偏好 `editor.blame_inline`(默认开)/ `blame_gutter`(默认关),面包屑栏按钮可临时切换追溯栏。
- **语言智能(LSP,对标 Cursor / Zed)**:编辑器(`gpui-base`)自带悬停气泡 / ⌘ 悬停下划线 / 补全菜单 / 诊断波浪线的脚手架,`loom-app/lsp.rs` 只实现它的 `HoverProvider` / `DefinitionProvider` / `CompletionProvider` 并接上 `loom-lsp`。`IdeView.lsp_attach` 在打开 / 重新载入文件时按扩展名选服务器、后台启动(同项目复用),就绪后给文档装上提供者与 `show_document`(跨文件跳转:推迟到本次事件之后调 `IdeView::open_at`,避免在编辑器更新期间回读它);F12 绑定 `GoToDefinition`,⌘ 点击是编辑器内置。**位置换算**:编辑器的 `Position.character` 按字符计,LSP 按 UTF-16,所有进出位置都经 `loom_lsp::position` 换算;文档 URI 与工作区根一律用规范化路径(解开 `/var` → `/private/var` 这类符号链接)。同步:`didOpen`(整篇)→ 编辑后 0.25 秒 `didChange`(整篇)→ 每个请求前先补同步 → 保存 `didSave` → 文档释放 `didClose`(`DocSync::drop`)。诊断:`publishDiagnostics` 经 `async_channel` 回到界面线程,版本落后于已同步版本的丢弃,铺到 `DiagnosticSet`,错误 / 警告计数与服务器名 / 索引进度显示在 IDE 状态栏。偏好 `editor.lsp`(默认开)。新增服务器在 `servers::SERVERS` 加一行。真实 rust-analyzer 的端到端测试:`cargo test -p loom-lsp --test rust_analyzer -- --ignored`。
- **渲染隔离(避免整窗重建)**:gpui 标脏一个视图会连带它的全部祖先,任何动画每帧都会让 `Workbench::render` 重跑,所以重的子树要做成独立视图并 `.cached()` 嵌入(目前是时间线 `timeline_view.rs`,`SessionModel.timeline`)。缓存视图只在自己被 notify 时重建,渲染里读到的模型 / 全局不会让它失效,必须显式 `cx.observe` / `observe_global`;子视图(输入框、Markdown)的通知会自动连带标脏。渲染里不要读非激活会话的 `SessionModel`(读过的实体 notify 会让窗口失效),侧栏状态点等读 `Workbench.live`(`live_sig.rs`,由 observe 回调维护,非激活会话只在摘要变化时才通知工作台)。诊断里的「界面构建次数」与 debug 日志「界面构建累计 N 次」可用来观察多余的重绘。
- **窗口必须始终有焦点元素**:窗口级快捷键(⌃`、⌘J、⌃⇧G 等)依赖焦点链;面板被隐藏时其中的焦点会消失,导致后续快捷键无响应。`Workbench` / `IdeWindow` 的根节点在 render 里保证有焦点,收起带焦点的面板(`toggle_term`、`toggle_terminal`)时要把焦点还给根。
- **不要在工作台更新期间回读它**:`Workbench` 的方法、`Workbench::new`、`IdeWindow::new` 里创建子视图 / 调用子实体方法时,子实体不能再 `wb.read(cx)` / `wb.update(cx)`(重复借用会 panic,在 `extern "C"` 回调里直接 abort);需要的数据由调用方作为参数传入,或用 `cx.defer` 推迟。`ide_window.rs` 里有源码级回归测试。
- **编辑器与终端全工作台唯一**:`Workbench.editor`(`Entity<IdeView>`)是唯一的编辑器,Agent 右栏「文件」标签(`files_workspace.rs`)、融合 IDE、独立 IDE 窗口都嵌入它,标签 / 预览标签 / 面包屑 / Markdown / 迷你地图 / 导航历史只有一份实现;项目切换与标签记忆由 `Workbench::sync_editor_root`、`save_editor_memory` 统一处理;打开文件统一走 `Workbench::open_file` / `open_in_editor`。独立 IDE 窗口开着时 Agent「文件」标签只放占位(不让同一个编辑器在两个窗口同时渲染)。新建任何文本编辑器一律用 `ide_prefs::new_editor`(偏好一致)。终端由全局 `ProjectTerminals` 按**规范化的项目根**(`term_store::normalize_root`)分组,Agent 与 IDE 共用;标签样式在 `term_tabs.rs`;同一终端被两个窗口同时显示时,只听 `claim_size` 认领的窗口的尺寸;切换运行模式的确认框会提示正在运行命令的终端(`Terminal::is_busy`)。
- **表格视图**:csv / tsv / xlsx / xls / ods 在编辑器区域里以网格打开(`sheet_view.rs` 状态与读取、`sheet_grid.rs` 虚拟网格、`sheet_bar.rs` 编辑栏与底栏、`sheet_find.rs` ⌘F 查找、`sheet_filter_panel.rs` 列筛选面板,标签接入见 `ide_sheet.rs`);csv 底栏有「以文本打开」改用代码编辑器。Diff 默认左右对比(`diff_split.rs`,偏好 `scm.diff_split`,差异块顶部可切换)。悬停卡片自绘(`ide_hover.rs`,对齐 Zed:300ms 延迟、粘性收起、光标移动 / 滚动收起)。语法高亮:库里 Swift / C# / CMake / Protobuf 的高亮查询是空串,由 `lang_hl.rs` 补注册;富文本行内代码跟随主题色(`md_theme.rs`);Markdown 分栏同步滚动见 `ide_md_sync.rs`。编辑器右键菜单对标 VS Code(`ide_menu.rs`,原生菜单;转到定义 / 类型定义 / 实现 / 引用、重命名、更改所有匹配项、格式化、重构 / 源代码操作、比较、Git Blame、显示历史、自动换行、命令面板),LSP 动作在 `lsp_actions.rs` / `lsp_refs.rs`,结果浮层在 `ide_overlay.rs`。
- **系统通知**(`app_icon::notify`):从 `.app` 包运行时走 UserNotifications(`objc2-user-notifications`,通知归属本应用、图标取包图标,首次请求授权;被拒绝时不再回退,日志记「系统通知权限未授予」,到 系统设置 → 通知 里开启);不在包里运行才回退 osascript(会记在「脚本编辑器」名下)。开发版在 `LoomCode Dev.app` 里,所以也走前者。
- **代码块与长输出限高**(`code_box.rs`,参考 ohmyagent `CollapsibleOutput`):助手正文的围栏代码块由 `rich_text::split_blocks` 拆成 `Block::Code`(流式未闭合的也拆;四个以上反引号的外层围栏整段交给 Markdown),工具卡的 Text / Terminal / Diff 输出也走同一个 `fold_body`:超过 12 行限高 196px 并内部滚动,底部「展开 · 共 N 行」展开后上限 480px,运行中贴底跟随,头部复制按钮复制完整内容。展开状态存在 `SessionModel.expanded`,键 `cb:<条目 id>:<第几块>`(`remeasure_id` 认这个前缀);句柄按元素 id 存在线程局部表里。
- **输入框最小宽度**:`layout_rules::COMPOSER_MIN_W`(= 外边距 + 工具栏内边距 + 模式 chip 下限 + 模型 chip 下限 + 固定控件 + 间距,有测试保证不小于各部分之和)是 IDE 对话栏等放输入框的栏的宽度下限;工具栏的模式 / 模型 chip 窄时收缩并截断文字(`min_w` + `truncate`),图标与固定按钮不动,所以不会被挤出框外。改工具栏控件时同步改这些常量。
- 编译零警告;改动后跑 `cargo test --workspace`。

## Claude 协议要点(已对真实 claude 2.1.x 验证)

- 初始化:`initialize` 带 `perTaskStopAffordance`(否则中断会杀掉后台任务)、`agentProgressSummaries`、`forwardSubagentText`;`stop_task {task_id}` 停任务,`background_tasks` 查存活任务;`system` 的 `task_started / task_progress / task_updated / task_notification / background_tasks_changed` 见 `translate.rs`(`tool_progress` 心跳刻意忽略);后台 Bash 的 `toolUseResult.backgroundTaskId` 与后台 Agent 的 `async_launched` 都只表示「已启动」,不是完成。
- 参数:`--output-format stream-json --verbose --input-format stream-json --include-partial-messages --permission-prompt-tool=stdio`,另按需加 `--resume`/`--session-id`/`--model`/`--permission-mode`/`--effort`。
- 握手:发 `initialize` 控制请求,响应里有模型列表与当前权限模式;`system init` 要等第一条提示之后才出现。
- 权限:`can_use_tool` 在 `assistant` 的 `tool_use` 之后到达,按 `tool_use_id` 对应;`AskUserQuestion` 的答案写进 `updatedInput.answers`;`ExitPlanMode` 批准时回传建议的 `updatedPermissions`。
- claude 不回显用户输入,后端自己把提示追加进时间线。
- 历史在 `~/.claude/projects/<编码后的 cwd>/*.jsonl`,目录名把所有非字母数字字符替换成 `-`(有损,真实 cwd 取自会话内容)。

- **编辑器叠层必须在绘制阶段定位**(括号竖线 `ide_pairs::guides`、当前块高亮线 `ide_guides::overlay`、行内追溯 `ide_blame::inline_blame`):编辑器的布局(`range_to_bounds` / `visible_row_range`)在它**自己的绘制**里才更新,渲染阶段读到的是上一帧的,滚动停下后叠层会停在原地。所以叠层一律做成盖住编辑区的 `canvas`(`absolute().size_full()`,排在编辑器之后),在 paint 闭包里读编辑器实体、算位置、`paint_quad` / `shape_line().paint`。**`text_bounds()` 不是可视区**:它是随滚动平移的内容区域(滚下去后 y 是很大的负数),裁剪请用 `ide_guides::text_view(ed, 画布范围)`(左边 = 内容原点 − 横向滚动量,上下 = 画布自己的范围)。画布闭包要 `'static`,所以共享状态(如 `PairState`)内部用 `Rc` 且可克隆。
