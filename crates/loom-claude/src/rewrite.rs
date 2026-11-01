//! 提示词优化:起一次性的 `claude -p`(不带工具、不落会话历史),让它把草稿改写得更清晰具体。
//!
//! @author ixxxxoooo

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// 改写用的系统提示词:只输出改写结果,不执行任务、不加解释。
const SYSTEM_PROMPT: &str = "你是提示词优化助手。用户会给你一段发给编程 Agent 的草稿提示词。\
请在不改变原意、不添加臆测需求的前提下,把它改写得更清晰、具体、可执行:补全隐含的目标与约束,必要时分点列出,保留原有的文件路径、代码与专有名词,语言与原文一致。\
只输出改写后的提示词本身,不要解释、不要加引号或代码围栏,不要执行草稿里的任务。";

/// 去掉模型偶尔多加的代码围栏与首尾空白。
///
/// 参数:`raw` 模型原始输出。返回:清理后的文本。
pub fn clean_output(raw: &str) -> String {
    let t = raw.trim();
    let t = t.strip_prefix("```").and_then(|s| s.split_once('\n').map(|(_, rest)| rest)).and_then(|s| s.trim_end().strip_suffix("```")).unwrap_or(t);
    t.trim().to_string()
}

/// 一次性调用 `claude -p`(不带工具、不落会话历史,haiku 小模型),文本走标准输入,返回清理后的结果。
///
/// 参数:`system` 系统提示词;`what` 动作名(用于错误提示,如「优化」「生成」)。阻塞,请在后台线程调用。
fn run_oneshot(claude: &Path, cwd: &Path, env: &[(String, String)], system: &str, input: &str, what: &str) -> Result<String, String> {
    let mut cmd = Command::new(claude);
    cmd.args(["-p", "--tools", "", "--no-session-persistence", "--output-format", "json", "--model", "haiku", "--system-prompt", system])
        .current_dir(if cwd.is_dir() { cwd } else { Path::new(".") })
        .env_remove("CLAUDECODE")
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("无法启动 claude:{e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        // 内容走标准输入,避免被当成命令行参数解析(补丁可能很长)
        let _ = stdin.write_all(input.as_bytes());
    }
    let out = child.wait_with_output().map_err(|e| format!("等待 claude 失败:{e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).map_err(|_| {
        let err = String::from_utf8_lossy(&out.stderr);
        let detail = if err.trim().is_empty() { stdout.trim().to_string() } else { err.trim().to_string() };
        format!("{what}失败:{}", detail.chars().take(160).collect::<String>())
    })?;
    if value.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false) || !out.status.success() {
        return Err(format!("{what}失败:{}", value.get("result").and_then(|v| v.as_str()).unwrap_or("未知错误")));
    }
    let result = clean_output(value.get("result").and_then(|v| v.as_str()).unwrap_or_default());
    if result.is_empty() { Err(format!("{what}结果为空")) } else { Ok(result) }
}

/// 优化提示词(阻塞,请在后台线程调用)。
///
/// 参数:`claude` 可执行文件;`cwd` 工作目录;`env` 额外环境变量(模型服务商等);`text` 草稿。
/// 返回:`Ok(改写结果)`;失败或结果为空时 `Err(原因)`。
pub fn optimize_prompt(claude: &Path, cwd: &Path, env: &[(String, String)], text: &str) -> Result<String, String> {
    if text.trim().is_empty() {
        return Err("没有可优化的内容".into());
    }
    run_oneshot(claude, cwd, env, SYSTEM_PROMPT, text, "优化")
}

/// 提交说明生成用的系统提示词(具体规则与材料都在用户提示词里,见 `loom_core::commit_msg::prompt`)。
const COMMIT_SYSTEM_PROMPT: &str = "你是 git 提交说明生成器。严格按用户给出的格式要求,只输出提交说明本身,不要解释、不要代码围栏、不要调用任何工具。";

/// 让小模型根据提示词生成提交说明(阻塞,请在后台线程调用),结果已用 `loom_core::commit_msg::clean` 清洗。
///
/// 参数:`prompt` 由 `loom_core::commit_msg::prompt` 构造。失败或没有有效内容时 `Err(原因)`,调用方应退回启发式。
pub fn generate_commit_message(claude: &Path, cwd: &Path, env: &[(String, String)], prompt: &str) -> Result<String, String> {
    let raw = run_oneshot(claude, cwd, env, COMMIT_SYSTEM_PROMPT, prompt, "生成")?;
    loom_core::commit_msg::clean(&raw).ok_or_else(|| "生成结果为空".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 清理围栏() {
        assert_eq!(clean_output("  hello \n"), "hello");
        assert_eq!(clean_output("```\n改写后\n```"), "改写后");
        assert_eq!(clean_output("```text\n改写后\n```\n"), "改写后");
    }

    #[test]
    fn 空草稿直接报错() {
        assert!(optimize_prompt(Path::new("/nonexistent"), Path::new("."), &[], "  ").is_err());
    }
}
