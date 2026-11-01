//! `session_index` 的单元测试:全量扫描、增量追加、半行、文件被替换、落盘缓存。
//!
//! @author ixxxxoooo

use super::*;
use std::io::Write;

/// 每个测试一个独立的临时目录(索引是全局的,路径不同互不干扰)。
fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("loom-sidx-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 按当前元数据扫描。
fn scan_now(p: &Path) -> Scan {
    let m = std::fs::metadata(p).unwrap();
    scan(p, m.len(), m.modified().unwrap()).unwrap()
}

fn append(p: &Path, text: &str) {
    std::fs::OpenOptions::new().append(true).create(true).open(p).unwrap().write_all(text.as_bytes()).unwrap();
}

/// 一条带 cwd 的用户输入行(填充到超过 HEAD_LEN,让增量路径生效)。
fn user_line(text: &str) -> String {
    format!("{{\"type\":\"user\",\"cwd\":\"/work/demo\",\"pad\":\"{}\",\"message\":{{\"content\":\"{text}\"}}}}\n", "x".repeat(300))
}

#[test]
fn 全量扫描取出标题与目录() {
    let p = tmp("full").join("a.jsonl");
    append(&p, &user_line("第一条提示"));
    append(&p, "{\"type\":\"assistant\",\"message\":{}}\n");
    let s = scan_now(&p);
    assert_eq!(s.cwd, "/work/demo");
    assert_eq!(s.title().as_deref(), Some("第一条提示"));
    assert_eq!(s.offset, std::fs::metadata(&p).unwrap().len());
}

#[test]
fn 追加内容只扫新增部分且后到的标题生效() {
    let p = tmp("incr").join("a.jsonl");
    append(&p, &user_line("问题"));
    let first = scan_now(&p);
    append(&p, "{\"type\":\"ai-title\",\"aiTitle\":\"自动标题\"}\n");
    let second = scan_now(&p);
    assert!(second.offset > first.offset);
    assert_eq!(second.first_user.as_deref(), Some("问题"), "旧字段保留");
    assert_eq!(second.title().as_deref(), Some("自动标题"));
}

#[test]
fn 末尾半行留到下次再读() {
    let p = tmp("half").join("a.jsonl");
    append(&p, &user_line("问题"));
    append(&p, "{\"type\":\"ai-title\",\"aiTi");
    let s = scan_now(&p);
    assert_eq!(s.ai_title, None);
    assert!(s.offset < std::fs::metadata(&p).unwrap().len());
    append(&p, "tle\":\"补全的标题\"}\n");
    assert_eq!(scan_now(&p).ai_title.as_deref(), Some("补全的标题"));
}

#[test]
fn 文件被替换时从头重扫() {
    let p = tmp("replace").join("a.jsonl");
    append(&p, &user_line("旧问题"));
    append(&p, "{\"type\":\"ai-title\",\"aiTitle\":\"旧标题\"}\n");
    scan_now(&p);
    // 变短:必须重扫,旧标题不能残留
    std::fs::write(&p, user_line("新问题")).unwrap();
    let s = scan_now(&p);
    assert_eq!(s.ai_title, None);
    assert_eq!(s.first_user.as_deref(), Some("新问题"));
}

#[test]
fn 没有用户输入的会话没有标题() {
    let p = tmp("empty").join("a.jsonl");
    append(&p, "{\"type\":\"summary\",\"cwd\":\"/w\"}\n");
    assert_eq!(scan_now(&p).title(), None);
}

#[test]
fn 缓存落盘后可以重新载入() {
    let d = tmp("persist");
    let p = d.join("a.jsonl");
    append(&p, &user_line("问题"));
    let cache = d.join("index.json");
    set_cache_file(cache.clone());
    scan_now(&p);
    flush();
    let saved: HashMap<PathBuf, Scan> = serde_json::from_str(&std::fs::read_to_string(&cache).unwrap()).unwrap();
    assert_eq!(saved.get(&p).and_then(Scan::title).as_deref(), Some("问题"));
}
