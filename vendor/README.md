# vendor

## gpui-component 0.8.0(补丁版)

来源:crates.io 的 `gpui-component 0.8.0`,经根 `Cargo.toml` 的 `[patch.crates-io]` 替换。

**改动两处**:
1. `src/highlighter/input_adapter.rs` 与 `src/highlighter/mod.rs` 里把 `input_highlighter_factory` 从 `pub(crate)` 改为 `pub`。
用途:`loom-app/src/ide_rainbow.rs` 要在 tree-sitter 语法高亮外面包一层,把括号字符按嵌套深度改字色(对标 Zed `colorize_brackets` / VS Code)。

2. 编辑器内查找替换小窗(`src/input/search.rs` 的 `render`、`src/input/input.rs` 的 `render_editor`)按 VS Code 重做:浮在编辑区右上角的紧凑小窗(宽 340px,不占行高),左侧 ▸/▾ 展开替换,输入框内带「区分大小写」,右侧「第 N 项,共 M 项 / 无结果」、上一个 / 下一个 / 关闭;替换框里 Enter = 替换当前项。匹配引擎在 `gpui-base`(Aho-Corasick 字面量,只有大小写开关),所以没有「全字匹配 / 正则」。

升级 gpui-kit 时:换成新版本源码,重做上面的改动;若上游已公开该工厂,直接删掉本目录与 `[patch]`。

## gpui-pre 0.3.9(补丁版)

来源:crates.io 的 `gpui-pre 0.3.9`,经根 `Cargo.toml` 的 `[patch.crates-io]` 替换。

**改动三处**(都在 `src/elements/list.rs`):
1. `List::prepaint`:首次布局或列表宽度变化时,原实现把所有行的高度提示清成 `None`(未测量的行按 0 高度计);改为保留旧高度(`item.size_hint()`)。
2. 同处:宽度变化时不再 `measuring_behavior.reset()`。原实现会让 `measure_all` 在每次宽度变化后重测全部行,拖动窗口 / 分栏时每帧都测一遍会卡死;现在「一次测完」只在 `reset`(载入历史)后做一次。
3. 新增 `ListState::take_measure_all_stats()`:取走最近一次「一次测完」的(行数, 耗时),`chat_scroll.rs` 在绘制阶段取走并记日志「会话流一次测完 rows=… 耗时=…ms」。
用途:会话流虚拟列表。按 0 高度计时,往上滚进还没渲染过的历史,「滚动位置 → 行」的换算会塌缩,一次跨过十几行,表现为滚着滚着突然跳走;配合 `chat_scroll.rs` 给每行设的估高(`EST_ROW_H`)才有效。

升级 gpui 时:重做这一处;若上游已保留提示,直接删掉本目录与 `[patch]`。

@author ixxxxoooo

## gpui-base 0.8.0(补丁版)

来源:crates.io 的 `gpui-base 0.8.0`,经根 `Cargo.toml` 的 `[patch.crates-io]` 替换。

**改动一处**:`src/text/state.rs` 的 `MAX_SYNC_FULL_REPLACE_BYTES` 从 4 KiB 改成 256 KiB。
用途:`TextView` 对超过阈值的 Markdown 在后台线程解析,首次布局只有占位高度,解析完才突然变高。会话流里长回复进入视口、或被列表「一次测完」测量时,行高与滚动条会跳几千像素(「滚动乱跳、滚动条乱跳」的根因)。同步解析几十 KB 的 Markdown 只要几毫秒;超过 256 KiB 的仍走后台。

升级 gpui-kit 时:换成新版本源码,重做这一处;若上游已提供更合理的策略,直接删掉本目录与 `[patch]`。


## tree-sitter-sequel 0.3.11(补丁版)

来源:crates.io 的 `tree-sitter-sequel 0.3.11`(SQL 语法),经根 `Cargo.toml` 的 `[patch.crates-io]` 替换。

**改动两处**:
1. `Cargo.toml` 的构建依赖 `cc` 从 `~1.2.1` 放宽为 `1.2.1`(即 `^1.2.1`)。上游把 `cc` 钉死在 1.2.x,而工作区里 `tree-sitter-c-sharp` 等已用到 `cc` 1.9,两者无法同时解析,导致一直没法启用 SQL 高亮。
2. `bindings/rust/build.rs` 编译 C 时加 `-Wno-sign-compare`(上游 `scanner.c` 的一条有符号比较警告,路径依赖不会像 crates.io 依赖那样被静默)。

另删去运行不需要的 `grammar.js`、`grammar.json`、`Cargo.lock`(生成文件 `parser.c` 与 `node-types.json` 保留)。
用途:编辑器、迷你地图、Markdown 代码块、Diff 的 `sql` 语法高亮(`gpui-kit` 的 `tree-sitter-sql` 特性)。

升级 gpui-kit 时:若上游放宽了 `cc` 版本要求,直接删掉本目录与 `[patch]`。

**注意**:这些 vendor 目录会被个人全局 gitignore 的 `vendor` 规则忽略,新增或修改后要 `git add -f vendor/...`(`gpui-base` 此前就漏提交了,已补上)。
