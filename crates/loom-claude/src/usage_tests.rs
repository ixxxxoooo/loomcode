//! 用量统计的测试。
//!
//! @author ixxxxoooo

use super::*;

const JSONL: &str = r#"{"type":"user","timestamp":"2026-10-30T10:00:00Z","message":{"role":"user","content":"帮我看看"}}
{"type":"assistant","timestamp":"2026-10-30T10:00:01Z","message":{"id":"m1","model":"claude-sonnet-5-5","content":[{"type":"text","text":"好"}],"usage":{"input_tokens":10,"output_tokens":1,"cache_read_input_tokens":100}}}
{"type":"assistant","timestamp":"2026-10-30T10:00:02Z","message":{"id":"m1","model":"claude-sonnet-5-5","content":[{"type":"tool_use","id":"t1"}],"usage":{"input_tokens":10,"output_tokens":20,"cache_read_input_tokens":100}}}
{"type":"user","timestamp":"2026-10-30T10:00:03Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1"}]}}
{"type":"assistant","timestamp":"2026-10-31T09:00:00Z","message":{"id":"m2","model":"claude-opus-5-5","content":[],"usage":{"input_tokens":5,"output_tokens":5,"cache_creation_input_tokens":50}}}
{"type":"assistant","timestamp":"2026-10-31T09:00:01Z","message":{"id":"m3","model":"<synthetic>","content":[],"usage":{"input_tokens":999}}}
不是 json"#;

#[test]
fn 按消息去重并统计工具与提问() {
    let e = parse_file(JSONL);
    assert_eq!(e.rows.len(), 2);
    let s = e.rows.iter().find(|r| r.model == "claude-sonnet-5-5").unwrap();
    assert_eq!((s.input, s.output, s.cache_read, s.replies, s.tools), (10, 20, 100, 1, 1));
    assert_eq!(e.prompts.get("2026-10-30"), Some(&1));
    assert!(!e.rows.iter().any(|r| r.model == "<synthetic>"));
}

#[test]
fn 日期换算() {
    assert_eq!(civil(0), "1970-01-01");
    assert_eq!(civil(20_757), "2026-10-31");
}

#[test]
fn 汇总报告() {
    let e = parse_file(JSONL);
    let r = build_report(&[&e], 20_757);
    assert_eq!(r.days.len(), 30);
    assert_eq!(r.days.last().unwrap().date, "2026-10-31");
    assert_eq!(r.recent(7).replies, 2);
    assert_eq!(r.recent(7).sessions, 2);
    assert_eq!(r.recent(1).tokens, 60);
    assert_eq!(r.by_model[0].0, "claude-sonnet-5-5");
    assert!((r.cache_hit_rate() - 100. / 165.).abs() < 1e-9);
}

#[test]
fn 增量缓存() {
    let dir = std::env::temp_dir().join(format!("loom-usage-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("projects/-a")).unwrap();
    std::fs::write(dir.join("projects/-a/s.jsonl"), JSONL).unwrap();
    let cache = dir.join("usage-index.json");
    let r1 = scan(&dir.join("projects"), &cache);
    assert_eq!(r1.files, 1);
    assert!(cache.exists());
    assert_eq!(scan(&dir.join("projects"), &cache).total_output, r1.total_output);
    let _ = std::fs::remove_dir_all(dir);
}
