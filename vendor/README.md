# vendor

## gpui-component 0.8.0(补丁版)

来源:crates.io 的 `gpui-component 0.8.0`,经根 `Cargo.toml` 的 `[patch.crates-io]` 替换。

**唯一改动**:`src/highlighter/input_adapter.rs` 与 `src/highlighter/mod.rs` 里把 `input_highlighter_factory` 从 `pub(crate)` 改为 `pub`。
用途:`loom-app/src/ide_rainbow.rs` 要在 tree-sitter 语法高亮外面包一层,把括号字符按嵌套深度改字色(对标 Zed `colorize_brackets` / VS Code)。

升级 gpui-kit 时:换成新版本源码,重做上面这一处改动;若上游已公开该工厂,直接删掉本目录与 `[patch]`。

@author ixxxxoooo
