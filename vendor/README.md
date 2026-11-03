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

**改动一处**:`src/elements/list.rs` 的 `List::prepaint`:首次布局或列表宽度变化时,原实现把所有行的高度提示清成 `None`(未测量的行按 0 高度计);改为保留旧高度(`item.size_hint()`)。
用途:会话流虚拟列表。按 0 高度计时,往上滚进还没渲染过的历史,「滚动位置 → 行」的换算会塌缩,一次跨过十几行,表现为滚着滚着突然跳走;配合 `chat_scroll.rs` 给每行设的估高(`EST_ROW_H`)才有效。

升级 gpui 时:重做这一处;若上游已保留提示,直接删掉本目录与 `[patch]`。

@author ixxxxoooo
