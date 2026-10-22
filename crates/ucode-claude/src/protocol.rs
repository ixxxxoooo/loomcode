//! 发往 claude 标准输入的消息构造。全部是纯函数,便于单测。

use crate::translate::{Pending, PendingKind};
use base64::Engine;
use serde_json::{Value, json};
use ucode_core::{Effort, PermissionMode};

/// 用户消息。`steer` 为真时带 `priority: now`,让 claude 在当前回合中途就地吸收。
pub fn user_message(text: &str, images: &[String], steer: bool) -> Value {
    let mut content: Vec<Value> = Vec::new();
    for path in images {
        match std::fs::read(path) {
            Ok(bytes) => content.push(json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": image_media_type(path),
                    "data": base64::engine::general_purpose::STANDARD.encode(bytes),
                }
            })),
            // 读不到的图片退化成文字说明,避免整条消息发送失败
            Err(_) => content.push(json!({"type": "text", "text": format!("[图片读取失败:{path}]")})),
        }
    }
    content.push(json!({"type": "text", "text": text}));
    let mut msg = json!({
        "type": "user",
        "message": {"role": "user", "content": content},
        "parent_tool_use_id": null,
        "session_id": "",
        "uuid": uuid::Uuid::new_v4().to_string(),
    });
    if steer {
        msg["priority"] = json!("now");
    }
    msg
}

/// 按扩展名推断图片 MIME。
pub fn image_media_type(path: &str) -> &'static str {
    match path.rsplit('.').next().map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => "image/png",
    }
}

/// 控制请求外壳。
pub fn control_request(request_id: &str, request: Value) -> Value {
    json!({"type": "control_request", "request_id": request_id, "request": request})
}

pub fn initialize(request_id: &str) -> Value {
    control_request(request_id, json!({"subtype": "initialize"}))
}

pub fn interrupt(request_id: &str) -> Value {
    control_request(request_id, json!({"subtype": "interrupt"}))
}

/// 切换模型;`default` 表示恢复默认,此时不带 `model` 字段。
pub fn set_model(request_id: &str, model: &str) -> Value {
    if model == "default" {
        return control_request(request_id, json!({"subtype": "set_model"}));
    }
    control_request(request_id, json!({"subtype": "set_model", "model": model}))
}

pub fn set_permission_mode(request_id: &str, mode: PermissionMode) -> Value {
    control_request(request_id, json!({"subtype": "set_permission_mode", "mode": mode.cli_value()}))
}

pub fn set_effort(request_id: &str, effort: Effort) -> Value {
    control_request(
        request_id,
        json!({"subtype": "apply_flag_settings", "settings": {"effortLevel": effort.cli_value()}}),
    )
}

/// 控制响应:成功。
pub fn success_response(request_id: &str, body: Value) -> Value {
    json!({"type": "control_response", "response": {"subtype": "success", "request_id": request_id, "response": body}})
}

/// 控制响应:错误。
pub fn error_response(request_id: &str, error: &str) -> Value {
    json!({"type": "control_response", "response": {"subtype": "error", "request_id": request_id, "error": error}})
}

fn allow_body(pending: &Pending, updated_input: Value, updated_permissions: Option<Value>) -> Value {
    let mut body = json!({"behavior": "allow", "updatedInput": updated_input});
    if let Some(id) = &pending.tool_use_id {
        body["toolUseID"] = json!(id);
    }
    if let Some(p) = updated_permissions {
        body["updatedPermissions"] = p;
    }
    body
}

fn deny_body(pending: &Pending, message: &str) -> Value {
    let mut body = json!({"behavior": "deny", "message": message});
    if let Some(id) = &pending.tool_use_id {
        body["toolUseID"] = json!(id);
    }
    body
}

/// 「总是允许」时写入的权限更新:优先采用 claude 自己给出的建议,否则按工具名生成本地规则。
pub fn always_allow_permissions(pending: &Pending) -> Value {
    if !pending.suggestions.is_empty() {
        return Value::Array(pending.suggestions.clone());
    }
    let mut rule = json!({"toolName": pending.tool_name});
    if pending.tool_name == "Bash"
        && let Some(first) = pending.input.get("command").and_then(Value::as_str).and_then(|c| c.split_whitespace().next())
    {
        rule["ruleContent"] = json!(format!("{first}:*"));
    }
    json!([{"type": "addRules", "rules": [rule], "behavior": "allow", "destination": "localSettings"}])
}

/// 权限请求的应答。`option` 取值:`allow_once` / `allow_always` / 其它视为拒绝。
pub fn permission_response(pending: &Pending, option: &str) -> Value {
    let body = match option {
        "allow_once" => allow_body(pending, pending.input.clone(), None),
        "allow_always" => allow_body(pending, pending.input.clone(), Some(always_allow_permissions(pending))),
        _ => deny_body(pending, "用户拒绝了此次操作"),
    };
    success_response(&pending.request_id, body)
}

/// AskUserQuestion 的应答:把答案以「问题 → 选中标签」写回入参的 `answers`。
pub fn question_response(pending: &Pending, answers: &[Vec<String>]) -> Value {
    let mut input = pending.input.clone();
    let mut map = serde_json::Map::new();
    if let Some(questions) = pending.input.get("questions").and_then(Value::as_array) {
        for (i, q) in questions.iter().enumerate() {
            let text = q.get("question").and_then(Value::as_str).unwrap_or_default();
            let picked = answers.get(i).map(|a| a.join(", ")).unwrap_or_default();
            map.insert(text.to_string(), json!(picked));
        }
    }
    input["answers"] = Value::Object(map);
    success_response(&pending.request_id, allow_body(pending, input, None))
}

/// ExitPlanMode 的应答:批准时沿用 claude 建议的模式切换,拒绝时让它继续规划。
pub fn plan_response(pending: &Pending, approve: bool) -> Value {
    let body = if approve {
        let perms = (!pending.suggestions.is_empty()).then(|| Value::Array(pending.suggestions.clone()));
        allow_body(pending, pending.input.clone(), perms)
    } else {
        deny_body(pending, "用户没有批准该计划,请根据反馈继续完善计划")
    };
    success_response(&pending.request_id, body)
}

/// 按挂起请求的类型生成「默认拒绝」应答(用于进程退出前清理等场景)。
pub fn reject_response(pending: &Pending) -> Value {
    match pending.kind {
        PendingKind::Permission => permission_response(pending, "reject"),
        PendingKind::Question => success_response(&pending.request_id, deny_body(pending, "用户取消了提问")),
        PendingKind::Plan => plan_response(pending, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(kind: PendingKind) -> Pending {
        Pending {
            request_id: "r1".into(),
            tool_use_id: Some("t1".into()),
            tool_name: "Bash".into(),
            input: json!({"command": "cargo test --all"}),
            suggestions: vec![],
            kind,
        }
    }

    #[test]
    fn 用户消息含图片与引导优先级() {
        let m = user_message("你好", &[], true);
        assert_eq!(m["priority"], "now");
        assert_eq!(m["message"]["content"][0]["text"], "你好");
        let m = user_message("x", &["/不存在.png".into()], false);
        assert!(m.get("priority").is_none());
        assert_eq!(m["message"]["content"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn 允许一次与拒绝() {
        let p = pending(PendingKind::Permission);
        let ok = permission_response(&p, "allow_once");
        assert_eq!(ok["response"]["response"]["behavior"], "allow");
        assert_eq!(ok["response"]["response"]["toolUseID"], "t1");
        assert_eq!(ok["response"]["request_id"], "r1");
        let no = permission_response(&p, "reject");
        assert_eq!(no["response"]["response"]["behavior"], "deny");
    }

    #[test]
    fn 总是允许生成本地规则() {
        let p = pending(PendingKind::Permission);
        let r = permission_response(&p, "allow_always");
        let perm = &r["response"]["response"]["updatedPermissions"][0];
        assert_eq!(perm["type"], "addRules");
        assert_eq!(perm["rules"][0]["toolName"], "Bash");
        assert_eq!(perm["rules"][0]["ruleContent"], "cargo:*");
        assert_eq!(perm["destination"], "localSettings");
    }

    #[test]
    fn 总是允许优先用_claude_的建议() {
        let mut p = pending(PendingKind::Permission);
        p.suggestions = vec![json!({"type": "setMode", "mode": "acceptEdits", "destination": "session"})];
        let r = permission_response(&p, "allow_always");
        assert_eq!(r["response"]["response"]["updatedPermissions"][0]["type"], "setMode");
    }

    #[test]
    fn 提问答案写回入参() {
        let mut p = pending(PendingKind::Question);
        p.tool_name = "AskUserQuestion".into();
        p.input = json!({"questions": [{"question": "用哪个库?", "options": []}, {"question": "要测试吗?"}]});
        let r = question_response(&p, &[vec!["serde".into(), "json".into()], vec!["要".into()]]);
        let answers = &r["response"]["response"]["updatedInput"]["answers"];
        assert_eq!(answers["用哪个库?"], "serde, json");
        assert_eq!(answers["要测试吗?"], "要");
    }

    #[test]
    fn 计划批准与拒绝() {
        let mut p = pending(PendingKind::Plan);
        p.suggestions = vec![json!({"type": "setMode", "mode": "default", "destination": "session"})];
        let yes = plan_response(&p, true);
        assert_eq!(yes["response"]["response"]["behavior"], "allow");
        assert_eq!(yes["response"]["response"]["updatedPermissions"][0]["mode"], "default");
        let no = plan_response(&p, false);
        assert_eq!(no["response"]["response"]["behavior"], "deny");
    }

    #[test]
    fn 控制请求外壳() {
        assert_eq!(set_model("a", "opus")["request"]["model"], "opus");
        assert!(set_model("a", "default")["request"].get("model").is_none());
        assert_eq!(set_effort("a", Effort::High)["request"]["settings"]["effortLevel"], "high");
        assert_eq!(set_permission_mode("a", PermissionMode::Plan)["request"]["mode"], "plan");
    }
}
