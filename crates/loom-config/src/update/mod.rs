//! 应用更新:读取发布源(GitHub Release 附带的 `appcast.json`),与当前版本比较;下载、校验、暂存与替换见 `install`。
//!
//! 发布源格式(与 OhMyAgent 一致,按通道分键,至少需要 `stable`):
//! `{ "stable": { "version": "0.2.0", "build": 12, "url": "https://…/LoomCode-0.2.0.dmg", "sha256": "…", "notes": "…" } }`
//!
//! @author ixxxxoooo

mod install;

pub use install::{StagedUpdate, download, launch_replacer, load_staged, take_install_failure};

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// 项目 GitHub 仓库。
pub const REPOSITORY_URL: &str = "https://github.com/ixxxxoooo/loomcode";
/// 默认发布源:最新正式版 Release 附带的 `appcast.json`(由 `scripts/publish.sh` 上传)。
pub const DEFAULT_FEED_URL: &str = "https://github.com/ixxxxoooo/loomcode/releases/latest/download/appcast.json";

/// 一个可用版本的描述。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub version: String,
    #[serde(default)]
    pub build: Option<u64>,
    pub url: String,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

/// 检查结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateResult {
    UpToDate,
    Available(UpdateInfo),
    Failed(String),
}

/// 比较点分版本号(忽略数字后的后缀,缺位按 0)。
///
/// 参数:`a`、`b` 两个版本号。返回:a 大于 b 为 1,小于为 -1,相等为 0。
pub fn compare(a: &str, b: &str) -> i32 {
    let parts = |v: &str| -> Vec<u64> { v.trim_start_matches('v').split('.').map(|p| p.chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap_or(0)).collect() };
    let (x, y) = (parts(a), parts(b));
    for i in 0..x.len().max(y.len()) {
        let (p, q) = (x.get(i).copied().unwrap_or(0), y.get(i).copied().unwrap_or(0));
        if p != q {
            return if p > q { 1 } else { -1 };
        }
    }
    0
}

/// 解析发布源并与当前版本比较。
///
/// 参数:`data` 发布源 JSON;`channel` 通道(不存在时回退 stable);`current` 当前版本;`build` 当前构建号。
/// 返回:检查结果;下载地址必须是 https。
pub fn evaluate(data: &[u8], channel: &str, current: &str, build: u64) -> UpdateResult {
    let Ok(feed) = serde_json::from_slice::<std::collections::BTreeMap<String, UpdateInfo>>(data) else {
        return UpdateResult::Failed("发布源格式不正确".into());
    };
    let Some(info) = feed.get(channel).or_else(|| feed.get("stable")) else {
        return UpdateResult::Failed("发布源缺少 stable 通道".into());
    };
    if !info.url.starts_with("https://") {
        return UpdateResult::Failed("发布源中的下载地址必须是 https".into());
    }
    let order = compare(&info.version, current);
    if order > 0 || (order == 0 && info.build.unwrap_or(0) > build) { UpdateResult::Available(info.clone()) } else { UpdateResult::UpToDate }
}

/// 拉取发布源并检查更新(阻塞,界面在后台线程调用)。
///
/// 参数:`feed_url` 发布源(空则用默认源,必须 https);`current` 当前版本;`build` 当前构建号。
pub fn check(feed_url: &str, current: &str, build: u64) -> UpdateResult {
    let url = if feed_url.trim().is_empty() { DEFAULT_FEED_URL } else { feed_url.trim() };
    if !url.starts_with("https://") {
        return UpdateResult::Failed("更新源必须使用 https".into());
    }
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(12)).build();
    match agent.get(url).call() {
        Ok(resp) => match resp.into_string() {
            Ok(body) => evaluate(body.as_bytes(), "stable", current, build),
            Err(e) => UpdateResult::Failed(format!("读取更新信息失败:{e}")),
        },
        Err(ureq::Error::Status(404, _)) => UpdateResult::Failed("还没有发布过版本(发布源不存在)".into()),
        Err(ureq::Error::Status(code, _)) => UpdateResult::Failed(format!("获取更新信息失败:HTTP {code}")),
        Err(e) => UpdateResult::Failed(format!("获取更新信息失败:{e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED: &str = r#"{"stable":{"version":"0.2.0","build":12,"url":"https://x/y.dmg","sha256":"ab","notes":"n"}}"#;

    #[test]
    fn compares_versions() {
        assert_eq!(compare("0.2.0", "0.1.9"), 1);
        assert_eq!(compare("v1.0", "1.0.0"), 0);
        assert_eq!(compare("0.9.9", "0.10.0"), -1);
        assert_eq!(compare("1.2.3-beta", "1.2.3"), 0);
    }

    #[test]
    fn detects_newer_version_and_build() {
        assert!(matches!(evaluate(FEED.as_bytes(), "stable", "0.1.0", 1), UpdateResult::Available(i) if i.version == "0.2.0"));
        assert_eq!(evaluate(FEED.as_bytes(), "stable", "0.2.0", 12), UpdateResult::UpToDate);
        assert!(matches!(evaluate(FEED.as_bytes(), "stable", "0.2.0", 3), UpdateResult::Available(_)));
        assert_eq!(evaluate(FEED.as_bytes(), "preview", "0.3.0", 0), UpdateResult::UpToDate);
    }

    #[test]
    fn rejects_bad_feeds() {
        assert!(matches!(evaluate(b"nope", "stable", "0.1.0", 0), UpdateResult::Failed(_)));
        assert!(matches!(evaluate(b"{}", "stable", "0.1.0", 0), UpdateResult::Failed(_)));
        let http = FEED.replace("https://", "http://");
        assert!(matches!(evaluate(http.as_bytes(), "stable", "0.1.0", 0), UpdateResult::Failed(_)));
    }
}
