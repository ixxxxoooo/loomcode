//! Swift 语法高亮:gpui-component 内置的 Swift 语法只带解析器、高亮查询是空串(只解析不着色),
//! 这里用 `tree-sitter-swift` 自带的 highlights / injections / locals 查询重新注册一遍。
//! 编辑器、迷你地图、Markdown 代码块、Diff 都走同一个语言注册表,注册一次处处生效。
//!
//! @author ixxxxoooo

use gpui_kit::component::highlighter::{LanguageConfig, LanguageRegistry};

/// 注册带高亮查询的 Swift 语法;启动时调用一次(重复调用只会覆盖同一项)。
pub fn register() {
    let config = LanguageConfig::new(
        "swift",
        tree_sitter_swift::LANGUAGE.into(),
        vec![],
        tree_sitter_swift::HIGHLIGHTS_QUERY,
        tree_sitter_swift::INJECTIONS_QUERY,
        tree_sitter_swift::LOCALS_QUERY,
    );
    LanguageRegistry::singleton().register("swift", &config);
}

#[cfg(test)]
mod tests {
    use gpui_kit::component::highlighter::{HighlightTheme, SyntaxHighlighter};

    #[::core::prelude::v1::test]
    fn 注册后_swift_关键字有高亮样式() {
        super::register();
        let code = "import Foundation\nlet x = 1\n";
        let mut hl = SyntaxHighlighter::new("swift");
        hl.update(None, &code.into(), None);
        let styles = hl.styles(&(0..code.len()), &*HighlightTheme::default_light());
        assert!(!styles.is_empty(), "Swift 应该有高亮片段");
    }
}
