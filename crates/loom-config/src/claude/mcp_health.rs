//! MCP 连接检测:解析 `claude mcp list` 的输出;从 Claude Desktop 配置导入服务器。
//!
//! @author ixxxxoooo

use super::{Scope, upsert_mcp_raw};
use serde_json::Value;
use std::path::Path;

/// 服务器连接状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Connected,
    NeedsAuth,
    Failed,
    Unknown,
}

/// 解析 `claude mcp list` 的输出。每行形如 `名称: 命令或地址 - ✔ Connected`。
///
/// 参数:`text` 命令输出(已去掉颜色)。返回:(名称, 状态, 原始状态文字) 列表,无法识别的行被忽略。
pub fn parse_mcp_list(text: &str) -> Vec<(String, Health, String)> {
    text.lines()
        .filter_map(|line| {
            let (head, status) = line.rsplit_once(" - ")?;
            let (name, _) = head.split_once(": ")?;
            let lower = status.to_lowercase();
            let health = if lower.contains("connected") && !lower.contains("failed") {
                Health::Connected
            } else if lower.contains("auth") {
                Health::NeedsAuth
            } else if lower.contains("fail") || lower.contains("error") {
                Health::Failed
            } else {
                Health::Unknown
            };
            Some((name.trim().to_string(), health, status.trim_start_matches(['✔', '✓', '✗', '✘', '⚠', '!', ' ']).to_string()))
        })
        .collect()
}

/// Claude Desktop 的配置文件路径。
pub fn desktop_config_path(home: &Path) -> std::path::PathBuf {
    home.join("Library/Application Support/Claude/claude_desktop_config.json")
}

/// 把 Claude Desktop 里的 MCP 服务器导入到指定层;名称不合法或配置不完整的跳过。
///
/// 参数:`desktop` Desktop 配置文件;`home` / `project` / `scope` 目标。返回:成功导入的数量。
pub fn import_from_desktop(desktop: &Path, home: &Path, project: Option<&Path>, scope: Scope) -> std::io::Result<usize> {
    let text = std::fs::read_to_string(desktop).map_err(|e| std::io::Error::other(format!("读不到 {}:{e}", desktop.display())))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| std::io::Error::other(format!("Claude Desktop 配置不是合法 JSON:{e}")))?;
    let Some(map) = v["mcpServers"].as_object() else { return Ok(0) };
    let mut n = 0;
    for (name, cfg) in map {
        if upsert_mcp_raw(home, project, scope, name, cfg.clone()).is_ok() {
            n += 1;
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 解析连接检测输出() {
        let out = "Checking MCP server health…\n\n\
            claude.ai Claude Docs: https://api.anthropic.com/v1/pages/mcp - ✔ Connected\n\
            fs: npx -y server-fs . - ✗ Failed to connect\n\
            gh: https://x - ⚠ Needs authentication\n\
            odd: cmd - something else\n";
        let r = parse_mcp_list(out);
        assert_eq!(r.len(), 4);
        assert_eq!((r[0].0.as_str(), r[0].1), ("claude.ai Claude Docs", Health::Connected));
        assert_eq!(r[1].1, Health::Failed);
        assert_eq!(r[2].1, Health::NeedsAuth);
        assert_eq!(r[3].1, Health::Unknown);
        assert_eq!(r[1].2, "Failed to connect");
    }

    #[test]
    fn 从_desktop_导入() {
        let dir = std::env::temp_dir().join(format!("loom-desktop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let desktop = dir.join("claude_desktop_config.json");
        std::fs::write(&desktop, r#"{"mcpServers":{"fs":{"command":"npx","args":["-y","x"]},"坏 名":{"command":"x"},"empty":{}}}"#).unwrap();
        assert_eq!(import_from_desktop(&desktop, &dir, None, Scope::User).unwrap(), 1);
        let cfg = super::super::load(&dir, None);
        assert_eq!(cfg.mcp_servers.len(), 1);
        assert!(import_from_desktop(&dir.join("none.json"), &dir, None, Scope::User).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
