//! 与真实 rust-analyzer 的端到端验证(需要本机装有可运行的 rust-analyzer,所以默认忽略):
//! `cargo test -p loom-lsp --test rust_analyzer -- --ignored --nocapture`

use loom_lsp::{hub, servers, uri};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

#[test]
#[ignore = "需要本机的 rust-analyzer,首次索引较慢"]
fn 悬停与跳转到定义() {
    let spec = servers::SERVERS.iter().find(|s| s.id == "rust-analyzer").unwrap();
    if servers::find_binary(spec.cmd).is_none() {
        eprintln!("未安装 rust-analyzer,跳过");
        return;
    }
    let dir = std::env::temp_dir().join(format!("loom-lsp-ra-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n").unwrap();
    let main = "/// 打招呼。\nfn greet(name: &str) -> String {\n    format!(\"你好 {name}\")\n}\n\nfn main() {\n    let 变量 = greet(\"x\");\n    println!(\"{}\", 变量);\n}\n";
    std::fs::write(dir.join("src/main.rs"), main).unwrap();
    let root = dir.canonicalize().unwrap();
    let file = root.join("src/main.rs");

    let client = hub::get_or_start(root.to_str().unwrap(), spec).expect("启动 rust-analyzer");
    let u = uri::from_path(&file);
    client.notify("textDocument/didOpen", json!({ "textDocument": { "uri": u, "languageId": "rust", "version": 1, "text": main } }));

    // 光标在第 6 行 `greet(` 的 g 上(第 6 行 `    let 变量 = greet`,UTF-16 列:4 + 3("let") + 1 + 2("变量") + 1 + 1("=") + 1 = 13)
    let pos = json!({ "line": 6, "character": 13 });
    let ask = |method: &str| -> Value {
        let t = Instant::now();
        loop {
            let v = client.request_blocking(method, json!({ "textDocument": { "uri": u }, "position": pos }), Duration::from_secs(30)).unwrap_or(Value::Null);
            if !v.is_null() || t.elapsed() > Duration::from_secs(90) {
                return v;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    };
    let hover = ask("textDocument/hover");
    let text = hover.to_string();
    assert!(text.contains("greet"), "悬停应包含函数名:{text}");
    let def = ask("textDocument/definition");
    let loc = def.as_array().and_then(|a| a.first()).cloned().unwrap_or(def.clone());
    let target = loc.get("targetUri").or_else(|| loc.get("uri")).and_then(Value::as_str).unwrap_or("");
    assert_eq!(uri::to_path(target).unwrap(), file, "定义应在同一文件:{def}");
    let line = loc.pointer("/targetSelectionRange/start/line").or_else(|| loc.pointer("/range/start/line")).and_then(Value::as_i64);
    assert_eq!(line, Some(1), "greet 定义在第 2 行:{def}");

    // 补全:在 `greet(` 之后的位置请求应返回条目
    let comp = client.request_blocking("textDocument/completion", json!({ "textDocument": { "uri": u }, "position": { "line": 7, "character": 24 } }), Duration::from_secs(30)).unwrap();
    assert!(!comp.is_null(), "补全应有结果");

    client.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}
