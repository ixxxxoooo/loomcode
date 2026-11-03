//! 补全库里「只有解析器、高亮查询是空串」的语言(Swift、C#、CMake、Protobuf):
//! 用各语法 crate 自带(或随仓库附带)的 highlights 查询重新注册,编辑器、迷你地图、Markdown 代码块、Diff 共用一个注册表,注册一次处处生效。
//!
//! 已知没有高亮的:GraphQL(语法 crate 不带查询)、XML / Dockerfile / INI(库里没有对应语法)。
//!
//! @author ixxxxoooo

use gpui_kit::component::highlighter::{LanguageConfig, LanguageRegistry};

/// Protobuf 的高亮查询(取自 tree-sitter-proto 0.2.0,MIT;crate 本身没有导出常量)。
const PROTO_HIGHLIGHTS: &str = include_str!("queries/proto_highlights.scm");

/// 注册带高亮查询的语法;启动时调用一次(重复调用只会覆盖同名项)。
pub fn register() {
    let reg = LanguageRegistry::singleton();
    macro_rules! add {
        ($name:expr, $lang:expr, $hl:expr, $inj:expr, $loc:expr) => {
            reg.register($name, &LanguageConfig::new($name, $lang.into(), vec![], $hl, $inj, $loc))
        };
    }
    add!("swift", tree_sitter_swift::LANGUAGE, tree_sitter_swift::HIGHLIGHTS_QUERY, tree_sitter_swift::INJECTIONS_QUERY, tree_sitter_swift::LOCALS_QUERY);
    add!("csharp", tree_sitter_c_sharp::LANGUAGE, tree_sitter_c_sharp::HIGHLIGHTS_QUERY, "", "");
    add!("cmake", tree_sitter_cmake::LANGUAGE, tree_sitter_cmake::HIGHLIGHTS_QUERY, tree_sitter_cmake::INJECTIONS_QUERY, "");
    add!("proto", tree_sitter_proto::LANGUAGE, PROTO_HIGHLIGHTS, "", "");
}

#[cfg(test)]
mod tests {
    use gpui_kit::component::highlighter::{HighlightTheme, SyntaxHighlighter};

    /// 对一段代码用某语言做高亮,返回片段数。
    fn spans(lang: &str, code: &str) -> usize {
        super::register();
        let mut hl = SyntaxHighlighter::new(lang);
        hl.update(None, &code.into(), None);
        hl.styles(&(0..code.len()), &*HighlightTheme::default_light()).len()
    }

    #[::core::prelude::v1::test]
    fn 补全的语言都有高亮片段() {
        assert!(spans("swift", "import Foundation\nlet x = 1\n") > 0);
        assert!(spans("csharp", "class A { public int X = 1; }\n") > 0);
        assert!(spans("cmake", "cmake_minimum_required(VERSION 3.20)\nproject(x)\n") > 0);
        assert!(spans("proto", "syntax = \"proto3\";\nmessage A { string b = 1; }\n") > 0);
    }

    #[::core::prelude::v1::test]
    fn sql有高亮() {
        assert!(spans("sql", "SELECT id, name FROM users WHERE age > 18 ORDER BY id; -- 注释\n") > 0);
    }

    #[::core::prelude::v1::test]
    fn 常用语言本来就有高亮() {
        for (lang, code) in [
            ("rust", "fn main() {}\n"), ("python", "def f():\n    return 1\n"), ("javascript", "const a = 1;\n"), ("typescript", "let a: number = 1;\n"),
            ("go", "package main\nfunc main() {}\n"), ("c", "int main() { return 0; }\n"), ("cpp", "class A { int x; };\n"), ("java", "class A { int x; }\n"),
            ("kotlin", "fun main() {}\n"), ("ruby", "def f; end\n"), ("php", "<?php function f() {}\n"), ("lua", "local a = 1\n"), ("bash", "echo hi\n"),
            ("json", "{\"a\": 1}\n"), ("yaml", "a: 1\n"), ("toml", "a = 1\n"), ("css", "a { color: red; }\n"), ("html", "<div></div>\n"), ("zig", "const a = 1;\n"), ("scala", "object A\n"), ("make", "all:\n\techo\n"),
        ] {
            assert!(spans(lang, code) > 0, "{lang} 应该有高亮");
        }
    }
}
