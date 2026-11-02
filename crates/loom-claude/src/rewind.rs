//! 解析 `rewind_files` 控制请求的应答。
//!
//! 已对真实 claude 2.1.316 验证:
//! - 预览成功:`{canRewind: true, filesChanged: [...], insertions, deletions}`;
//! - 执行成功:`{canRewind: true, skippedLinks: 0}`(不带文件列表);
//! - 没有检查点:`{canRewind: false, error: "No file checkpoint found for this message."}`,
//!   预览时仍是 success 应答,执行时是 error 应答。
//!
//! @author ixxxxoooo

use loom_core::RewindOutcome;
use serde_json::Value;

/// 把应答翻成结果。
/// 参数:`uuid` 目标消息,`dry_run` 是否预览,`ok` 应答是否 success,`body` 应答体,`error` error 应答的原因。
/// 返回:回退结果。
pub fn parse(uuid: &str, dry_run: bool, ok: bool, body: &Value, error: &str) -> RewindOutcome {
    if !ok {
        return RewindOutcome::failed(uuid, dry_run, error);
    }
    let count = |k: &str| body.get(k).and_then(Value::as_u64).unwrap_or(0) as u32;
    RewindOutcome {
        uuid: uuid.into(),
        dry_run,
        can_rewind: body.get("canRewind").and_then(Value::as_bool).unwrap_or(false),
        files: body.get("filesChanged").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect()).unwrap_or_default(),
        insertions: count("insertions"),
        deletions: count("deletions"),
        error: body.get("error").and_then(Value::as_str).map(String::from),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn 预览应答() {
        let body = json!({"canRewind": true, "filesChanged": ["/p/b.txt", "/p/a.txt"], "insertions": 1, "deletions": 2});
        let o = parse("u1", true, true, &body, "");
        assert!(o.can_rewind && o.dry_run && o.has_changes());
        assert_eq!(o.files.len(), 2);
        assert_eq!((o.insertions, o.deletions), (1, 2));
    }

    #[test]
    fn 没有检查点与错误应答() {
        let o = parse("u1", true, true, &json!({"canRewind": false, "error": "No file checkpoint found for this message."}), "");
        assert!(!o.can_rewind);
        assert!(o.reason().contains("检查点"));
        let o = parse("u1", false, false, &Value::Null, "Failed to rewind");
        assert_eq!(o.error.as_deref(), Some("Failed to rewind"));
        assert!(!o.dry_run);
    }

    #[test]
    fn 执行成功应答不带文件列表() {
        let o = parse("u1", false, true, &json!({"canRewind": true, "skippedLinks": 0}), "");
        assert!(o.can_rewind && o.error.is_none() && o.files.is_empty());
    }
}
