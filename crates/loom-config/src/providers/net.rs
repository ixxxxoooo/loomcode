//! 服务商网络操作:测试连接(发一条最小的 Messages 请求)、获取模型列表。都是阻塞调用,界面在后台线程执行。
//!
//! @author ixxxxoooo

use super::{AuthHeader, Provider};
use std::time::{Duration, Instant};

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).build()
}

fn with_auth(req: ureq::Request, p: &Provider, key: &str) -> ureq::Request {
    let req = req.set("anthropic-version", "2023-06-01");
    match p.auth {
        AuthHeader::Bearer => req.set("Authorization", &format!("Bearer {key}")),
        AuthHeader::XApiKey => req.set("x-api-key", key),
    }
}

/// 把 ureq 错误转成提示(带 HTTP 状态码与响应开头,不含密钥)。
fn describe(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, resp) => {
            let body = resp.into_string().unwrap_or_default();
            format!("HTTP {code}:{}", body.chars().take(200).collect::<String>())
        }
        ureq::Error::Transport(t) => format!("网络错误:{t}"),
    }
}

/// 发一条最小请求测试连接。
///
/// 参数:`p` 服务商;`key` API Key;`model` 测试用模型。返回:(耗时毫秒, 回复开头) 或错误提示。
pub fn test_connection(p: &Provider, key: &str, model: &str) -> Result<(u128, String), String> {
    if model.trim().is_empty() {
        return Err("先添加至少一个模型".into());
    }
    let url = format!("{}/v1/messages", p.base_url.trim().trim_end_matches('/'));
    let body = serde_json::json!({ "model": model, "max_tokens": 16, "messages": [{ "role": "user", "content": "ping" }] });
    let start = Instant::now();
    let resp = with_auth(agent().post(&url), p, key).send_json(body).map_err(describe)?;
    let ms = start.elapsed().as_millis();
    let v: serde_json::Value = resp.into_json().map_err(|e| format!("响应不是 JSON:{e}"))?;
    let text = v["content"].as_array().and_then(|c| c.iter().find_map(|b| b["text"].as_str())).unwrap_or("(无文本)").chars().take(60).collect();
    Ok((ms, text))
}

/// 获取模型列表(`GET /v1/models`,不是所有服务商都支持)。
///
/// 参数:`p` 服务商;`key` API Key。返回:模型 ID 列表或错误提示。
pub fn fetch_models(p: &Provider, key: &str) -> Result<Vec<String>, String> {
    let url = format!("{}/v1/models", p.base_url.trim().trim_end_matches('/'));
    let resp = with_auth(agent().get(&url), p, key).call().map_err(describe)?;
    let v: serde_json::Value = resp.into_json().map_err(|e| format!("响应不是 JSON:{e}"))?;
    let ids: Vec<String> = v["data"].as_array().into_iter().flatten().filter_map(|m| m["id"].as_str().map(str::to_string)).collect();
    if ids.is_empty() { Err("服务商没有返回模型列表,请手动添加".into()) } else { Ok(ids) }
}
