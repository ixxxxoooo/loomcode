//! 本机 gopls + 本地 Go 项目的端到端排查(默认忽略,路径由 LOOM_GO_ROOT / LOOM_GO_FILE / LOOM_GO_POS 指定):
//! `LOOM_GO_ROOT=~/proj LOOM_GO_FILE=a/b.go LOOM_GO_POS=187:17 cargo test -p loom-lsp --test gopls_local -- --ignored --nocapture`

use loom_lsp::{hub, servers, uri};
use serde_json::json;
use std::time::Duration;

#[test]
#[ignore = "需要本机 gopls 与指定的 Go 项目"]
fn 定义请求() {
    let (Ok(root), Ok(file), Ok(pos)) = (std::env::var("LOOM_GO_ROOT"), std::env::var("LOOM_GO_FILE"), std::env::var("LOOM_GO_POS")) else { return };
    let spec = servers::SERVERS.iter().find(|s| s.id == "gopls").unwrap();
    let root = std::path::Path::new(&root).canonicalize().unwrap();
    let path = root.join(&file);
    let text = std::fs::read_to_string(&path).unwrap();
    let client = hub::get_or_start(root.to_str().unwrap(), spec).expect("启动 gopls");
    let u = uri::from_path(&path);
    client.notify("textDocument/didOpen", json!({ "textDocument": { "uri": u, "languageId": "go", "version": 1, "text": text } }));
    let (l, c) = pos.split_once(':').unwrap();
    let position = json!({ "line": l.parse::<u32>().unwrap() - 1, "character": c.parse::<u32>().unwrap() - 1 });
    for method in ["textDocument/definition", "textDocument/hover", "textDocument/documentSymbol"] {
        let t = std::time::Instant::now();
        let r = client.request_blocking(method, json!({ "textDocument": { "uri": u }, "position": position }), Duration::from_secs(60));
        eprintln!("{method}({} ms) => {}", t.elapsed().as_millis(), format!("{r:?}").chars().take(600).collect::<String>());
    }
    client.shutdown();
}
